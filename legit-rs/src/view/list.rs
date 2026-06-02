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
    format::{format_age, format_size, pad_to_width, truncate},
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
        let text = if pr_list.is_loading(None) {
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
    let pr_num_col = pr_num_col_width(&visible);
    let size_col = size_col_width(&visible);
    let lines: Vec<Line<'_>> = pr_list
        .visible_rows()
        .map(|(row, selected)| match row {
            DisplayRow::Header(label) => header_line(label, width),
            DisplayRow::Pr(index) => {
                let pr = &prs[*index];
                let reason = model.blockers.get(&pr.key()).map(|b| b.reason.as_str());
                row_line(pr, reason, width, pr_num_col, size_col, now, selected)
            }
        })
        .collect();
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

const PR_NUM_COL_MIN: usize = 5;
const AUTHOR_COL: usize = 14;
const SIZE_COL_MIN: usize = 6;
const AGE_COL: usize = 6;
/// Width reserved for the trailing smart-status reason hint.
const REASON_COL: usize = 24;

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
    reason: Option<&str>,
    width: u16,
    pr_num_col: usize,
    size_col: usize,
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
    let size = format_size(pr.additions, pr.deletions);
    let age = format_age(pr.created_at, now);
    // The smart-status reason renders as a short trailing hint; "…" while the
    // PR's enrichment (and thus its blocker) is still being derived.
    let reason = reason.unwrap_or("…");

    let fixed = pr_num_col + AUTHOR_COL + size_col + AGE_COL + REASON_COL;
    let title_col = width.saturating_sub(fixed).max(1);

    let title = truncate(&raw_title, title_col);
    let author = truncate(&author, AUTHOR_COL);
    let reason = truncate(reason, REASON_COL);
    let age_col = width.saturating_sub(pr_num_col + title_col + AUTHOR_COL + size_col + REASON_COL);
    // Truncate, not just pad: in a narrow terminal `age_col` can saturate to 0,
    // and `pad_to_width` returns the string untouched when it already meets the
    // width — so without this the full age would overflow into the reason cell.
    let age = truncate(&age, age_col);

    // Pad each column by display width (not char count) so rows with wide
    // glyphs in the title/author stay aligned with ASCII rows.
    let rendered = format!(
        "{}{}{}{}{}{}",
        pad_to_width(&num, pr_num_col),
        pad_to_width(&title, title_col),
        pad_to_width(&author, AUTHOR_COL),
        pad_to_width(&size, size_col),
        pad_to_width(&age, age_col),
        pad_to_width(&reason, REASON_COL),
    );

    let line = Line::from(rendered);
    if selected {
        line.style(Style::default().add_modifier(Modifier::REVERSED))
    } else {
        line
    }
}
