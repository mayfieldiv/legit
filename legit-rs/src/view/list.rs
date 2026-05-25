use chrono::{DateTime, Utc};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::Paragraph,
};

use crate::{
    app::pr_list::{Phase, PrList},
    format::{format_age, format_size, truncate},
    github::rest::PR,
};

#[cfg(test)]
mod tests;

/// Render the PR list region. Renders the empty/loading placeholder, or one
/// row per PR with `#number | title | author | size | age` columns.
pub fn render(pr_list: &PrList, frame: &mut Frame<'_>, area: Rect, now: DateTime<Utc>) {
    if pr_list.prs().is_empty() {
        let text = match pr_list.phase() {
            Phase::Loading => "Loading pull requests…",
            _ => "No open pull requests",
        };
        let placeholder = Paragraph::new(Line::from(text)).alignment(Alignment::Center);
        frame.render_widget(placeholder, area);
        return;
    }

    let width = area.width;
    let selected = pr_list.selected();
    let pr_num_col = pr_num_col_width(pr_list.prs());
    let lines: Vec<Line<'_>> = pr_list
        .visible_rows()
        .map(|(pr_index, pr)| row_line(pr, width, pr_num_col, now, pr_index == selected))
        .collect();
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

const PR_NUM_COL_MIN: usize = 5;
const AUTHOR_COL: usize = 14;
const SIZE_COL: usize = 6;
const AGE_COL_MIN: usize = 8;

/// Width of the `#<number>` column, sized to fit the widest PR number in the
/// list. Floored at `PR_NUM_COL_MIN` so single-digit-PR repos still get a
/// readable two-column gap; widens uniformly once PR numbers cross 5 chars
/// (e.g. `#12345`) so the title column doesn't drift row-by-row.
fn pr_num_col_width(prs: &[PR]) -> usize {
    let widest = prs
        .iter()
        .map(|pr| format!("#{}", pr.number).chars().count())
        .max()
        .unwrap_or(0);
    widest.max(PR_NUM_COL_MIN)
}

fn row_line<'a>(
    pr: &'a PR,
    width: u16,
    pr_num_col: usize,
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

    let fixed = pr_num_col + AUTHOR_COL + SIZE_COL + AGE_COL_MIN;
    let title_col = width.saturating_sub(fixed).max(1);

    let title = truncate(&raw_title, title_col);
    let author = truncate(&author, AUTHOR_COL);
    let age_col = width.saturating_sub(pr_num_col + title_col + AUTHOR_COL + SIZE_COL);

    let rendered = format!(
        "{num:<num_w$}{title:<title_w$}{author:<author_w$}{size:<size_w$}{age:<age_w$}",
        num = num,
        title = title,
        author = author,
        size = size,
        age = age,
        num_w = pr_num_col,
        title_w = title_col,
        author_w = AUTHOR_COL,
        size_w = SIZE_COL,
        age_w = age_col,
    );

    let line = Line::from(rendered);
    if selected {
        line.style(Style::default().add_modifier(Modifier::REVERSED))
    } else {
        line
    }
}
