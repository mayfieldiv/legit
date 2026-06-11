use chrono::{DateTime, Utc};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{
    app::grouping::DisplayRow,
    app::model::Model,
    blocker::BlockerResult,
    format::{format_age, format_repo_short, format_size, pad_to_width, truncate, truncate_middle},
    github::rest::PR,
};

#[cfg(test)]
mod tests;

/// Render the PR list region. Renders the empty/loading placeholder, or the
/// grouped display rows: a header per group (`── Me blocking `) followed by one
/// `#number | title | author | size | age | reason` row per PR.
pub fn render(model: &Model, frame: &mut Frame<'_>, area: Rect, now: DateTime<Utc>) {
    let pr_list = &model.list;
    if pr_list.visible_is_empty() {
        // A filter that hid everything beats the fetch-state placeholders;
        // both checks are judged against the active tab's scope (a repo tab
        // shows its own repo's PRs and listing state, the All tab any repo).
        let scope = model.active_scope();
        let text = if pr_list.filter_hid_everything(scope.as_deref()) {
            "No matching PRs"
        } else if pr_list.is_loading(scope.as_deref()) {
            "Loading pull requests…"
        } else {
            "No open pull requests"
        };
        let placeholder = Paragraph::new(Line::from(text)).alignment(Alignment::Center);
        frame.render_widget(placeholder, area);
        return;
    }

    let width = area.width;
    let prs = pr_list.prs();
    // Size columns to the visible PRs only, so an off-tab PR's wide number or
    // diff size can't widen this tab's columns.
    let visible: Vec<&PR> = pr_list.visible_pr_indices().map(|i| &prs[i]).collect();
    let layout = RowLayout {
        width: usize::from(width),
        pr_num_col: pr_num_col_width(&visible),
        size_col: size_col_width(&visible),
        show_repo: should_show_repo_column(model),
    };
    let lines: Vec<Line<'_>> = pr_list
        .visible_rows()
        .map(|(row, selected)| match row {
            DisplayRow::Header(label) => header_line(label, width),
            DisplayRow::Pr(index) => {
                let pr = &prs[*index];
                row_line(pr, model.blockers.get(&pr.key()), &layout, now, selected)
            }
        })
        .collect();
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

const PR_NUM_COL_MIN: usize = 5;
const AUTHOR_COL: usize = 14;
const REPO_COL: usize = 14;
const SIZE_COL_MIN: usize = 6;
const AGE_COL: usize = 6;
/// Width reserved for the trailing smart-status reason hint.
const REASON_COL: usize = 24;
const GAP: usize = 1;

/// Whether the All tab shows the repo column. Keys off the tracked-repo count
/// (mirroring the TS `showRepo`) rather than the repo spread of the visible
/// PRs: a structural condition stays put while PRs stream in or a filter
/// narrows the list, so the columns never shift mid-read.
fn should_show_repo_column(model: &Model) -> bool {
    model.active_scope().is_none() && model.tracked_repos().len() > 1
}

/// Width of the `#<number>` column, sized to fit the widest visible PR number.
/// Floored at `PR_NUM_COL_MIN` so single-digit-PR repos still get a
/// readable two-column gap; widens uniformly once PR numbers cross 5 chars
/// (e.g. `#12345`) so the title column doesn't drift row-by-row.
fn pr_num_col_width(prs: &[&PR]) -> usize {
    let widest = prs
        .iter()
        .map(|pr| format!("#{}", pr.number).chars().count())
        .max()
        .unwrap_or(0);
    widest.max(PR_NUM_COL_MIN)
}

/// Width of the `+A/-D` size column, sized to fit the widest visible size
/// string. Floored at `SIZE_COL_MIN` so the minimum `+0/-0` form sits in a
/// stable column; `format_size` has no upper bound (PRs can touch thousands
/// of lines) so a fixed width would clip otherwise.
fn size_col_width(prs: &[&PR]) -> usize {
    let widest = prs
        .iter()
        .map(|pr| format_size(pr.additions, pr.deletions).chars().count())
        .max()
        .unwrap_or(0);
    widest.max(SIZE_COL_MIN)
}

/// A group header row: `── <label> ` in the accent colour, padded to the row
/// width. Visually distinct from PR rows (the leading rule and colour).
fn header_line(label: &str, width: u16) -> Line<'static> {
    let text = pad_to_width(&format!("── {label} "), width as usize);
    Line::from(Span::styled(
        text,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ))
}

/// Per-render layout shared by every PR row: the row width and the column
/// sizing derived from the visible PRs (widest number/size, repo column on a
/// multi-repo All tab).
struct RowLayout {
    width: usize,
    pr_num_col: usize,
    size_col: usize,
    show_repo: bool,
}

/// One PR's display row. The fixed-width cells are built as data — (text,
/// width, style) in display order — so the leftover-title-width math and the
/// rendered spans derive from the same list and can't drift apart.
fn row_line(
    pr: &PR,
    blocker: Option<&BlockerResult>,
    layout: &RowLayout,
    now: DateTime<Utc>,
    selected: bool,
) -> Line<'static> {
    let title = if pr.is_draft {
        format!("[draft] {}", pr.title)
    } else {
        pr.title.clone()
    };
    // The smart-status reason renders as a short trailing hint; "…" (in gray)
    // while the PR's enrichment (and thus its blocker) is still being derived.
    let reason = blocker.map(|b| b.reason.as_str()).unwrap_or("…");
    let reason_color = blocker.map_or(Color::Gray, |b| super::tier_color(b.tier));

    let mut cells: Vec<(String, usize, Style)> = vec![(
        format!("#{}", pr.number),
        layout.pr_num_col,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )];
    if layout.show_repo {
        // Magenta is this port's mapping for the TS selfHighlight colour the
        // repo cell uses (the same mapping as the me-blocking tier).
        let repo = truncate_middle(format_repo_short(&pr.repo_slug), REPO_COL);
        cells.push((repo, REPO_COL, Style::default().fg(Color::Magenta)));
    }
    let title_slot = cells.len();
    cells.push((
        truncate_middle(&pr.author, AUTHOR_COL),
        AUTHOR_COL,
        Style::default().fg(Color::Green),
    ));
    cells.push((
        format_size(pr.additions, pr.deletions),
        layout.size_col,
        Style::default(),
    ));
    cells.push((
        truncate(&format_age(pr.created_at, now), AGE_COL),
        AGE_COL,
        Style::default(),
    ));
    cells.push((
        truncate(reason, REASON_COL),
        REASON_COL,
        Style::default().fg(reason_color),
    ));

    // The title takes whatever the fixed cells and the inter-cell gaps leave.
    // With the title joining, every fixed cell borders exactly one gap, so
    // `cells.len()` (still the fixed count here) is also the gap count.
    let fixed: usize = cells.iter().map(|(_, w, _)| *w).sum::<usize>() + cells.len() * GAP;
    let title_col = layout.width.saturating_sub(fixed).max(1);
    cells.insert(
        title_slot,
        (truncate(&title, title_col), title_col, Style::default()),
    );

    let mut spans = Vec::with_capacity(cells.len() * 2 - 1);
    for (i, (text, width, style)) in cells.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" ".repeat(GAP)));
        }
        spans.push(Span::styled(pad_to_width(&text, width), style));
    }

    let line = Line::from(spans);
    if selected {
        line.style(Style::default().add_modifier(Modifier::REVERSED))
    } else {
        line
    }
}
