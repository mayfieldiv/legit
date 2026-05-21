use chrono::{DateTime, Utc};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::Paragraph,
};

use crate::{
    app::model::Model,
    format::{format_age, format_size, truncate},
    github::rest::PR,
};

#[cfg(test)]
mod tests;

/// Render the PR list region. Renders the empty/loading placeholder, or one
/// row per PR with `#number | title | author | size | age` columns.
pub fn render(model: &Model, frame: &mut Frame<'_>, area: Rect, now: DateTime<Utc>) {
    if model.prs.is_empty() {
        let text = if model.loading {
            "Loading pull requests…"
        } else {
            "No open pull requests"
        };
        let placeholder = Paragraph::new(Line::from(text)).alignment(Alignment::Center);
        frame.render_widget(placeholder, area);
        return;
    }

    let width = area.width;
    let lines: Vec<Line<'_>> = visible_window(model, area.height as usize)
        .iter()
        .enumerate()
        .map(|(visible_idx, pr)| {
            let pr_index = model.scroll_offset + visible_idx;
            let selected = pr_index == model.selected;
            row_line(pr, width, now, selected)
        })
        .collect();
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

fn visible_window(model: &Model, height: usize) -> &[PR] {
    if height == 0 || model.prs.is_empty() {
        return &[];
    }
    let start = model.scroll_offset.min(model.prs.len());
    let end = (start + height).min(model.prs.len());
    &model.prs[start..end]
}

const PR_NUM_COL: usize = 5;
const AUTHOR_COL: usize = 14;
const SIZE_COL: usize = 6;
const AGE_COL_MIN: usize = 8;

fn row_line<'a>(pr: &'a PR, width: u16, now: DateTime<Utc>, selected: bool) -> Line<'a> {
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

    let fixed = PR_NUM_COL + AUTHOR_COL + SIZE_COL + AGE_COL_MIN;
    let title_col = width.saturating_sub(fixed).max(1);

    let title = truncate(&raw_title, title_col);
    let author = truncate(&author, AUTHOR_COL);
    let age_col = width.saturating_sub(PR_NUM_COL + title_col + AUTHOR_COL + SIZE_COL);

    let rendered = format!(
        "{num:<num_w$}{title:<title_w$}{author:<author_w$}{size:<size_w$}{age:<age_w$}",
        num = num,
        title = title,
        author = author,
        size = size,
        age = age,
        num_w = PR_NUM_COL,
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
