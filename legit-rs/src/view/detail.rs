//! Detail view: paints the full PR detail page when `Model::view_mode` is
//! `ViewMode::Detail(DetailState)`. Layout:
//!
//! - Pinned header (number + title, author + repo + created/updated/size +
//!   draft marker, full GitHub URL, head→base branch + mergeable, divider)
//! - Scrollable body (offset by `DetailState::scroll`): the lines derived by
//!   `app::detail_layout::detail_content` — markdown-rendered PR description,
//!   the CI checks section, then the Review Threads and Conversation sections
//!   as focusable cards. `j`/`k`/arrows move the focus card-to-card (the
//!   scroll follows); PageDown/PageUp scroll raw (10 lines).
//! - The focused card draws a rounded border; unfocused cards reserve the same
//!   rows/columns with blanks so focus changes never shift the layout.
//! - Status bar: key hints, plus the shared right-aligned status overlay
//!   (`view::render_status_overlay`) so transient errors show here too
//!
//! All content derivation (which lines exist, where each card sits) lives in
//! `app::detail_layout`; this module only splits the frame and paints.

use chrono::{DateTime, Utc};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{
    app::detail_layout::{HEADER_HEIGHT, detail_content},
    app::model::{DetailState, Model},
    format::{format_age, format_mergeable, format_size},
    github::rest::PR,
};

#[cfg(test)]
mod tests;

/// Render the detail view into the full frame area. The PR is sourced from
/// `model.list` via `detail.key` so it carries the enriched values (mergeable,
/// head_commit_sha, etc.) written by `Msg::ReviewStatusArrived` rather than a
/// de-enriched copy.
pub fn render(
    model: &Model,
    detail: &DetailState,
    frame: &mut Frame<'_>,
    area: Rect,
    now: DateTime<Utc>,
) {
    // Look up the enriched PR from the list. If it has been removed (e.g.
    // a list refresh completed between navigation and this render), show the
    // loading placeholder so the view degrades gracefully.
    let Some(pr) = model.list.pr(&detail.key) else {
        render_loading(frame, area);
        return;
    };

    // The detail area is split into: header, body (fills remaining), status
    // bar. The header and status-bar rows are what `detail_layout::CHROME_ROWS`
    // accounts for when `update` derives the body viewport.
    let [header_area, body_area, status_area] = Layout::vertical([
        Constraint::Length(HEADER_HEIGHT),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);

    // The header is built entirely from the list PR, which is always available
    // here, so draw it immediately — matching the TS reference, which shows the
    // header at once and only the body waits on the fetch. The loading
    // placeholder occupies just the body area until the body arrives.
    render_header(model, pr, frame, header_area, now);
    render_status_bar(model, frame, status_area);

    match &detail.body {
        None => render_loading(frame, body_area),
        Some(body) => render_body(model, pr, body, detail, frame, body_area, now),
    }
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

fn render_header(model: &Model, pr: &PR, frame: &mut Frame<'_>, area: Rect, now: DateTime<Utc>) {
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
    let url_line = Line::from(Span::styled(
        pr.key().html_url(),
        Style::default().fg(Color::Cyan),
    ));

    // Row 3: head → base  ·  mergeable state
    let (merge_text, merge_color) = format_mergeable(&pr.mergeable);
    let branch_line = Line::from(vec![
        Span::styled(pr.head_ref.clone(), Style::default().fg(Color::Cyan)),
        Span::styled(" → ", Style::default().fg(Color::DarkGray)),
        Span::styled(pr.base_ref.clone(), Style::default().fg(Color::Cyan)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled(merge_text, Style::default().fg(merge_color)),
    ]);

    // Row 4: worktree path when detected; blank otherwise so the divider stays
    // pinned to the final header row.
    let worktree_line = model
        .worktree_for_pr(pr)
        .map(|entry| {
            super::worktree_line(
                &entry.path,
                usize::from(area.width).saturating_sub(" worktree: ".len() + 1),
            )
        })
        .unwrap_or_else(|| Line::from(""));

    // Row 5: divider
    let divider_line = Line::from(Span::styled(
        "─".repeat(area.width as usize),
        Style::default().fg(Color::DarkGray),
    ));

    let lines = vec![
        title_line,
        meta_line,
        url_line,
        branch_line,
        worktree_line,
        divider_line,
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

// ── Body (scrollable description + checks + cards) ───────────────────────────

/// Render the scrollable body: the full `detail_content` (description, checks,
/// thread and conversation cards), offset by `scroll` rows from the top.
/// `update` already clamps `scroll` to the true content height; this render
/// keeps a backstop clamp so a stale offset (e.g. a resize between the
/// keypress and this frame) can never show blank space past the end.
fn render_body(
    model: &Model,
    pr: &PR,
    description: &[Line<'static>],
    detail: &DetailState,
    frame: &mut Frame<'_>,
    area: Rect,
    now: DateTime<Utc>,
) {
    let content = detail_content(model, pr, description, detail, area.width, now);

    let max_scroll = content.lines.len().saturating_sub(usize::from(area.height));
    let scroll = detail.scroll.min(max_scroll);
    // The one place the usize scroll meets ratatui's u16: saturate, so content
    // past 65535 lines pins at the cap instead of wrapping back to the top.
    let scroll = u16::try_from(scroll).unwrap_or(u16::MAX);

    frame.render_widget(Paragraph::new(content.lines).scroll((scroll, 0)), area);
}

// ── Status bar ────────────────────────────────────────────────────────────────

fn render_status_bar(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let resolved_hint = if model.show_resolved {
        " hide resolved  "
    } else {
        " show resolved  "
    };
    let bots_hint = if model.show_bot_comments {
        " hide bots  "
    } else {
        " show bots  "
    };
    let hints = Line::from(vec![
        Span::styled("esc", bold),
        Span::raw(" back  "),
        Span::styled("j/k", bold),
        Span::raw(" focus  "),
        Span::styled("o", bold),
        Span::raw(" open  "),
        Span::styled("t", bold),
        Span::raw(resolved_hint),
        Span::styled("b", bold),
        Span::raw(bots_hint),
        Span::styled("r", bold),
        Span::raw(" refresh  "),
        Span::styled("w", bold),
        Span::raw(" worktree"),
    ]);
    frame.render_widget(Paragraph::new(hints), area);
    super::render_status_overlay(model, frame, area);
}
