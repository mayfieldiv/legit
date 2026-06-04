//! Detail view: renders the full PR detail page when `Model::view_mode` is
//! `ViewMode::Detail(key)`. Layout:
//!
//! - Pinned header (number + title, author + repo + created/updated/size +
//!   draft marker, full GitHub URL, head→base branch + mergeable, divider)
//! - Scrollable body: markdown-rendered PR description (via `markdown::render`)
//! - CI checks section: summary line + per-check rows (icon from `check_icon`)
//! - Status bar: "esc back  r refresh" hints

use chrono::{DateTime, Utc};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{
    app::model::Model,
    format::{check_icon, checks_summary, format_age, format_size, sort_check_runs},
    github::rest::PRDetail,
    markdown,
};

#[cfg(test)]
mod tests;

/// Render the detail view into the full frame area.
pub fn render(model: &Model, frame: &mut Frame<'_>, area: Rect, now: DateTime<Utc>) {
    // The detail area is split into: header, body (fills remaining), status bar.
    let [header_area, body_area, status_area] = Layout::vertical([
        Constraint::Length(header_height(model)),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);

    render_status_bar(frame, status_area);

    match &model.detail {
        None => render_loading(frame, body_area),
        Some(detail) => {
            render_header(detail, frame, header_area, now);
            render_body(model, detail, frame, body_area);
        }
    }
}

/// Number of rows in the pinned header. Fixed at 5: title, meta, URL,
/// branch+mergeable, divider. If the detail hasn't arrived yet we still
/// reserve these rows for the loading placeholder.
fn header_height(model: &Model) -> u16 {
    if model.detail.is_some() { 5 } else { 0 }
}

/// Render the "Loading PR detail…" placeholder while the fetch is in flight.
fn render_loading(frame: &mut Frame<'_>, area: Rect) {
    let line = Line::from(Span::styled(
        "Loading PR detail…",
        Style::default().fg(Color::Yellow),
    ));
    frame.render_widget(Paragraph::new(line), area);
}

// ── Header ────────────────────────────────────────────────────────────────────

fn render_header(detail: &PRDetail, frame: &mut Frame<'_>, area: Rect, now: DateTime<Utc>) {
    let pr = &detail.pr;

    // Row 0: #number title (bold)
    let title_text = format!("#{} {}", pr.number, pr.title);
    let title_line = Line::from(Span::styled(
        title_text,
        Style::default().add_modifier(Modifier::BOLD),
    ));

    // Row 1: author · repo · created X · updated Y · +A/-D [draft]
    let mut meta_spans = vec![
        Span::styled(pr.author.clone(), Style::default().fg(Color::Green)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled(pr.repo_slug.clone(), Style::default().fg(Color::Cyan)),
        Span::styled(" · created ", Style::default().fg(Color::DarkGray)),
        Span::raw(format_age(pr.created_at, now)),
        Span::styled(" · updated ", Style::default().fg(Color::DarkGray)),
        Span::raw(format_age(pr.updated_at, now)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::raw(format_size(pr.additions, pr.deletions)),
    ];
    if pr.is_draft {
        meta_spans.push(Span::styled(" draft", Style::default().fg(Color::Yellow)));
    }
    let meta_line = Line::from(meta_spans);

    // Row 2: full GitHub URL
    let url = format!("https://github.com/{}/pull/{}", pr.repo_slug, pr.number);
    let url_line = Line::from(Span::styled(url, Style::default().fg(Color::Cyan)));

    // Row 3: head → base  ·  mergeable state
    let (merge_text, merge_color) = format_mergeable(&pr.mergeable);
    let branch_line = Line::from(vec![
        Span::styled(pr.head_ref.clone(), Style::default().fg(Color::Cyan)),
        Span::styled(" → ", Style::default().fg(Color::DarkGray)),
        Span::styled(pr.base_ref.clone(), Style::default().fg(Color::Cyan)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled(merge_text, Style::default().fg(merge_color)),
    ]);

    // Row 4: divider
    let divider_line = Line::from(Span::styled(
        "─".repeat(area.width as usize),
        Style::default().fg(Color::DarkGray),
    ));

    let lines = vec![title_line, meta_line, url_line, branch_line, divider_line];
    frame.render_widget(Paragraph::new(lines), area);
}

/// Mirrors the TS `formatMergeable`: text + colour for the mergeable state.
fn format_mergeable(mergeable: &str) -> (&'static str, Color) {
    match mergeable {
        "MERGEABLE" => ("✓ mergeable", Color::Green),
        "CONFLICTING" => ("! conflict", Color::Red),
        _ => ("? merge unknown", Color::Gray),
    }
}

// ── Body (scrollable description + checks) ───────────────────────────────────

fn render_body(model: &Model, detail: &PRDetail, frame: &mut Frame<'_>, area: Rect) {
    let pr = &detail.pr;

    // Build the lines for the scrollable body: description + checks section.
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Description (markdown-rendered body, or placeholder when empty)
    let trimmed = detail.body.trim();
    if trimmed.is_empty() {
        lines.push(Line::from(Span::styled(
            "No description.",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        lines.extend(markdown::render(trimmed));
    }

    // CI Checks section (only when checks have arrived for this PR's commit)
    if let Some(checks) = model.enrichment.checks_for(pr) {
        if !checks.is_empty() {
            lines.push(Line::from(""));

            let summary = checks_summary(checks);
            let mut header_spans: Vec<Span<'static>> = vec![
                Span::styled(
                    "## CI Checks",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" {}/{} passed", summary.passed, summary.total),
                    Style::default().fg(Color::DarkGray),
                ),
            ];
            if summary.failed > 0 {
                header_spans.push(Span::styled(
                    format!(" · {} failed", summary.failed),
                    Style::default().fg(Color::Red),
                ));
            }
            if summary.pending > 0 {
                header_spans.push(Span::styled(
                    format!(" · {} pending", summary.pending),
                    Style::default().fg(Color::Yellow),
                ));
            }
            lines.push(Line::from(header_spans));

            // All check rows, sorted (failing first, then pending, then passed).
            let mut sorted: Vec<&crate::github::types::CheckRun> = checks.iter().collect();
            sort_check_runs(&mut sorted);
            for check in sorted {
                let (icon, color) = check_icon(check);
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(icon, Style::default().fg(color)),
                    Span::raw(format!(" {}", check.name)),
                ]));
            }
        }
    }

    frame.render_widget(Paragraph::new(lines), area);
}

// ── Status bar ────────────────────────────────────────────────────────────────

fn render_status_bar(frame: &mut Frame<'_>, area: Rect) {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let hints = Line::from(vec![
        Span::styled("esc", bold),
        Span::raw(" back  "),
        Span::styled("r", bold),
        Span::raw(" refresh"),
    ]);
    frame.render_widget(Paragraph::new(hints), area);
}
