//! Detail view: renders the full PR detail page when `Model::view_mode` is
//! `ViewMode::Detail(key)`. Layout:
//!
//! - Pinned header (number + title, author + repo + created/updated/size +
//!   draft marker, full GitHub URL, head→base branch + mergeable, divider)
//! - Scrollable body: markdown-rendered PR description (via `markdown::render`)
//!   and CI checks, offset by `Model::detail_scroll` (lines from the top).
//!   Scroll keys: `j`/`k`/arrows (1 line), PageDown/PageUp (10 lines).
//! - CI checks section: summary line + per-check rows (icon from `check_icon`)
//! - Status bar: "esc back  j/k scroll  r refresh" hints

use chrono::{DateTime, Utc};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{
    app::model::{Model, ViewMode},
    format::{
        check_icon, checks_summary, format_age, format_mergeable, format_size, sort_check_runs,
    },
    github::rest::PR,
    markdown,
};

#[cfg(test)]
mod tests;

/// Render the detail view into the full frame area. The PR is sourced from
/// `model.list` via the key in `ViewMode::Detail(key)` so it carries the
/// enriched values (mergeable, head_commit_sha, etc.) written by
/// `Msg::ReviewStatusArrived` rather than a de-enriched copy.
pub fn render(model: &Model, frame: &mut Frame<'_>, area: Rect, now: DateTime<Utc>) {
    // Resolve the PR key from the current view mode. If somehow called while
    // not in Detail mode (shouldn't happen), fall back to the loading layout.
    let key = match &model.view_mode {
        ViewMode::Detail(key) => key,
        _ => {
            render_loading(frame, frame.area());
            return;
        }
    };

    // Look up the enriched PR from the list. If it has been removed (e.g.
    // a list refresh completed between navigation and this render), show the
    // loading placeholder so the view degrades gracefully.
    let Some(pr) = model.list.pr(key) else {
        render_loading(frame, frame.area());
        return;
    };

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
        Some(body) => {
            render_header(pr, frame, header_area, now);
            render_body(model, pr, body, frame, body_area);
        }
    }
}

/// Number of rows in the pinned header. Fixed at 5: title, meta, URL,
/// branch+mergeable, divider. If the body hasn't arrived yet we still
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

fn render_header(pr: &PR, frame: &mut Frame<'_>, area: Rect, now: DateTime<Utc>) {
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

// ── Body (scrollable description + checks) ───────────────────────────────────

/// Build the CI checks section lines: blank separator + bold header with
/// pass/fail/pending counts + one row per check (sorted failing-first, then
/// pending, then passed). Returns an empty `Vec` when checks haven't arrived
/// for this PR's commit or the check list is empty. Mirrors `summary::checks_lines`.
fn checks_section_lines(model: &Model, pr: &crate::github::rest::PR) -> Vec<Line<'static>> {
    let Some(checks) = model.enrichment.checks_for(pr) else {
        return Vec::new();
    };
    if checks.is_empty() {
        return Vec::new();
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(""));

    let summary = checks_summary(checks);
    let mut header_spans: Vec<Span<'static>> = vec![
        // Use the canonical markdown heading helper so the accent colour and
        // bold rule stay in one place (markdown::heading_style).
        markdown::heading_span(2, "CI Checks"),
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
    lines
}

/// Render the scrollable body: markdown description + CI checks, offset by
/// `model.detail_scroll` rows from the top, clamped so the scroll offset
/// never exceeds `content_lines - viewport_rows` (the last screenful).
fn render_body(model: &Model, pr: &PR, body: &str, frame: &mut Frame<'_>, area: Rect) {
    // Build the lines for the scrollable body: description + checks section.
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Description (markdown-rendered body, or placeholder when empty)
    let trimmed = body.trim();
    if trimmed.is_empty() {
        lines.push(Line::from(Span::styled(
            "No description.",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        lines.extend(markdown::render(trimmed));
    }

    lines.extend(checks_section_lines(model, pr));

    // Clamp the scroll so the user can't scroll past the last line of content.
    // The model holds the raw offset (the update layer cannot know the rendered
    // line count), so we clamp here at render time before passing it to the
    // Paragraph widget. This keeps detail_scroll as the source of intent and
    // prevents blank space beyond the end of the body.
    let content_lines = lines.len() as u16;
    let viewport_rows = area.height;
    let max_scroll = content_lines.saturating_sub(viewport_rows);
    let scroll = model.detail_scroll.min(max_scroll);

    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), area);
}

// ── Status bar ────────────────────────────────────────────────────────────────

fn render_status_bar(frame: &mut Frame<'_>, area: Rect) {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let hints = Line::from(vec![
        Span::styled("esc", bold),
        Span::raw(" back  "),
        Span::styled("j/k", bold),
        Span::raw(" scroll  "),
        Span::styled("r", bold),
        Span::raw(" refresh"),
    ]);
    frame.render_widget(Paragraph::new(hints), area);
}
