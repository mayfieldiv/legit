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
    let show_repo = should_show_repo_column(model);
    let pr_num_col = pr_num_col_width(&visible);
    let size_col = size_col_width(&visible);
    let lines: Vec<Line<'_>> = pr_list
        .visible_rows()
        .map(|(row, selected)| match row {
            DisplayRow::Header(label) => header_line(label, width),
            DisplayRow::Pr(index) => {
                let pr = &prs[*index];
                let blocker = model.blockers.get(&pr.key());
                row_line(
                    pr, blocker, width, pr_num_col, size_col, show_repo, now, selected,
                )
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

fn row_line<'a>(
    pr: &'a PR,
    blocker: Option<&BlockerResult>,
    width: u16,
    pr_num_col: usize,
    size_col: usize,
    show_repo: bool,
    now: DateTime<Utc>,
    selected: bool,
) -> Line<'a> {
    let width = width as usize;
    let num = format!("#{}", pr.number);
    let raw_title = if pr.is_draft {
        format!("[draft] {}", pr.title)
    } else {
        pr.title.clone()
    };
    let author = pr.author.clone();
    let repo = format_repo_short(&pr.repo_slug);
    let size = format_size(pr.additions, pr.deletions);
    let age = format_age(pr.created_at, now);
    // The smart-status reason renders as a short trailing hint; "…" while the
    // PR's enrichment (and thus its blocker) is still being derived.
    let reason = blocker.map(|b| b.reason.as_str()).unwrap_or("…");

    let column_count = 6 + usize::from(show_repo);
    let fixed = pr_num_col
        + usize::from(show_repo) * REPO_COL
        + AUTHOR_COL
        + size_col
        + AGE_COL
        + REASON_COL
        + (column_count - 1) * GAP;
    let title_col = width.saturating_sub(fixed).max(1);

    let title = truncate(&raw_title, title_col);
    let author = truncate_middle(&author, AUTHOR_COL);
    let repo = truncate_middle(repo, REPO_COL);
    let reason = truncate(reason, REASON_COL);
    let age = truncate(&age, AGE_COL);

    let mut spans = vec![cell(
        &num,
        pr_num_col,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )];
    push_gap(&mut spans);
    if show_repo {
        // Magenta is this port's mapping for the TS selfHighlight colour the
        // repo cell uses (the same mapping as the me-blocking tier).
        spans.push(cell(&repo, REPO_COL, Style::default().fg(Color::Magenta)));
        push_gap(&mut spans);
    }
    spans.push(cell(&title, title_col, Style::default()));
    push_gap(&mut spans);
    spans.push(cell(&author, AUTHOR_COL, Style::default().fg(Color::Green)));
    push_gap(&mut spans);
    spans.push(cell(&size, size_col, Style::default()));
    push_gap(&mut spans);
    spans.push(cell(&age, AGE_COL, Style::default()));
    push_gap(&mut spans);
    spans.push(cell(
        &reason,
        REASON_COL,
        // Gray while the blocker is still being derived (the "…" placeholder).
        Style::default().fg(blocker.map_or(Color::Gray, |b| super::tier_color(b.tier))),
    ));

    let line = Line::from(spans);
    if selected {
        line.style(Style::default().add_modifier(Modifier::REVERSED))
    } else {
        line
    }
}

fn push_gap<'a>(spans: &mut Vec<Span<'a>>) {
    spans.push(Span::raw(" ".repeat(GAP)));
}

fn cell<'a>(text: &str, width: usize, style: Style) -> Span<'a> {
    Span::styled(pad_to_width(text, width), style)
}
