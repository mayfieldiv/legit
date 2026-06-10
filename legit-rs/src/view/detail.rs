//! Detail view: renders the full PR detail page when `Model::view_mode` is
//! `ViewMode::Detail(DetailState)`. Layout:
//!
//! - Pinned header (number + title, author + repo + created/updated/size +
//!   draft marker, full GitHub URL, head→base branch + mergeable, divider)
//! - Scrollable body: markdown-rendered PR description (via `markdown::render`)
//!   and CI checks, offset by `DetailState::scroll` (lines from the top).
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
    app::model::{DetailState, Model},
    blocker::{ThreadKind, classify_thread},
    format::{
        check_row, checks_summary, format_age, format_mergeable, format_size, sort_check_runs,
    },
    github::rest::PR,
    github::types::{CheckRun, FullReviewThread},
    markdown,
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

    // The detail area is split into: header, body (fills remaining), status bar.
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
    render_header(pr, frame, header_area, now);
    render_status_bar(frame, status_area);

    match &detail.body {
        None => render_loading(frame, body_area),
        Some(body) => render_body(model, pr, body, detail.scroll, frame, body_area, now),
    }
}

/// Number of rows in the pinned header: title, meta, URL, branch+mergeable,
/// divider. Constant — the header draws from the list PR, which is always
/// available, so it shows even while the body fetch is in flight.
const HEADER_HEIGHT: u16 = 5;

/// Fixed chrome rows the detail layout reserves around the scrollable body: the
/// pinned `HEADER_HEIGHT` plus the 1-row status bar (the `Constraint::Length(1)`
/// in `render`'s `Layout::vertical`). The single source of truth shared by this
/// module's layout and `update::clamp_detail_scroll`, which subtracts it from
/// the terminal height to derive the same body viewport. Mirrors how
/// `Model::chrome_rows` is shared between `sync_viewport` and `view::view`.
pub(crate) const CHROME_ROWS: u16 = HEADER_HEIGHT + 1;

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

/// Render the PR description to display lines: the markdown body, or a muted
/// "No description." placeholder when the body is blank. Pure (no model/area):
/// called once on `Msg::PRDetailArrived` so the markdown is parsed a single
/// time and the result cached in `DetailState::body`, not re-parsed per frame.
pub(crate) fn render_description_lines(body: &str) -> Vec<Line<'static>> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        vec![Line::from(Span::styled(
            "No description.",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        markdown::render(trimmed)
    }
}

/// Build the CI checks section lines: blank separator + bold header with
/// pass/fail/pending counts + one row per check (sorted failing-first, then
/// pending, then passed). Returns an empty `Vec` when checks haven't arrived
/// for this PR's commit or the check list is empty. Mirrors `summary::checks_lines`.
///
/// `pub(crate)` so the `update` scroll-clamp can measure these lines too: the
/// checks section is appended to the description per-frame (so late-arriving
/// checks show without a re-fetch), so the true content height — and thus the
/// max scroll offset — includes it.
pub(crate) fn checks_section_lines(model: &Model, pr: &PR) -> Vec<Line<'static>> {
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
    let mut sorted: Vec<&CheckRun> = checks.iter().collect();
    sort_check_runs(&mut sorted);
    lines.extend(sorted.into_iter().map(check_row));
    lines
}

// ── Review threads section ───────────────────────────────────────────────────

/// The coloured status badge for one thread (mirrors the TS `ThreadCard` badge).
fn thread_badge(thread: &FullReviewThread) -> Span<'static> {
    match classify_thread(thread) {
        ThreadKind::Resolved => Span::styled(" ✓ resolved", Style::default().fg(Color::Green)),
        ThreadKind::AwaitingReviewer => {
            Span::styled(" ◐ awaiting reviewer", Style::default().fg(Color::Cyan))
        }
        ThreadKind::Unreplied => Span::styled(" ● unreplied", Style::default().fg(Color::Yellow)),
    }
}

/// The `author · age` byline above a comment body. Bot authors are muted and
/// tagged `[bot]` (mirrors the TS `CommentRow` styling).
fn comment_byline(
    author: &str,
    is_bot: bool,
    created_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Line<'static> {
    let author_color = if is_bot {
        Color::DarkGray
    } else {
        Color::Green
    };
    let mut spans = vec![Span::styled(
        author.to_owned(),
        Style::default().fg(author_color),
    )];
    if is_bot {
        spans.push(Span::styled(" [bot]", Style::default().fg(Color::DarkGray)));
    }
    spans.push(Span::styled(
        format!(" · {}", format_age(created_at, now)),
        Style::default().fg(Color::DarkGray),
    ));
    Line::from(spans)
}

/// Build the Review Threads section lines: blank separator + bold header + one
/// card per thread (file:line + status badge, then the root comment's byline
/// and markdown body). Returns an empty `Vec` while threads haven't arrived
/// for this PR. Mirrors the TS `DetailView` threads section.
fn threads_section_lines(model: &Model, pr: &PR, now: DateTime<Utc>) -> Vec<Line<'static>> {
    // Absent key = the threads fetch hasn't landed yet (loading); an arrived
    // empty list renders no section at all.
    let Some(threads) = model.enrichment.review_threads.get(&pr.key()) else {
        return loading_placeholder("Loading threads…");
    };
    if threads.is_empty() {
        return Vec::new();
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(""));
    lines.push(Line::from(vec![markdown::heading_span(
        2,
        "Review Threads",
    )]));

    for thread in threads {
        let location = match thread.line {
            Some(line) => format!("{}:{line}", thread.path),
            None => thread.path.clone(),
        };
        lines.push(Line::from(vec![
            Span::styled(location, Style::default().fg(Color::Cyan)),
            thread_badge(thread),
        ]));
        if let Some(root) = thread.comments.first() {
            lines.push(comment_byline(
                &root.author,
                root.is_bot,
                root.created_at,
                now,
            ));
            lines.extend(markdown::render(&root.body));
        }
        // Replies: every comment after the root, indented with an ↳ byline
        // (mirrors the TS `ReplyRow`).
        for reply in thread.comments.iter().skip(1) {
            let mut byline = comment_byline(&reply.author, reply.is_bot, reply.created_at, now);
            byline.spans.insert(
                0,
                Span::styled("  ↳ ", Style::default().fg(Color::DarkGray)),
            );
            lines.push(byline);
            lines.extend(indent_lines(markdown::render(&reply.body), 4));
        }
    }
    lines
}

/// Build the Conversation section lines: blank separator + bold header + one
/// card per top-level issue comment (byline with bot styling + markdown body).
/// Returns an empty `Vec` while comments haven't arrived for this PR. Mirrors
/// the TS `DetailView` conversation section.
fn conversation_section_lines(model: &Model, pr: &PR, now: DateTime<Utc>) -> Vec<Line<'static>> {
    // Absent key = the comments fetch hasn't landed yet (loading); an arrived
    // empty list renders no section at all.
    let Some(comments) = model.enrichment.issue_comments.get(&pr.key()) else {
        return loading_placeholder("Loading comments…");
    };
    if comments.is_empty() {
        return Vec::new();
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(""));
    lines.push(Line::from(vec![markdown::heading_span(2, "Conversation")]));

    for comment in comments {
        lines.push(comment_byline(
            &comment.author,
            comment.is_bot,
            comment.created_at,
            now,
        ));
        lines.extend(markdown::render(&comment.body));
    }
    lines
}

/// A muted in-flight placeholder for a section whose fetch hasn't landed,
/// separated from the previous section by a blank line.
fn loading_placeholder(text: &'static str) -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        Line::from(Span::styled(text, Style::default().fg(Color::Yellow))),
    ]
}

/// Prefix every line with `pad` spaces (replies sit deeper than their thread
/// root, mirroring the TS reply-card indent).
fn indent_lines(lines: Vec<Line<'static>>, pad: usize) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|mut line| {
            line.spans.insert(0, Span::raw(" ".repeat(pad)));
            line
        })
        .collect()
}

/// Render the scrollable body: the pre-rendered `description` lines (cached in
/// `DetailState::body`) followed by the CI checks section, offset by `scroll`
/// rows from the top. `update` already clamps `scroll` to the true content
/// height; this render keeps a backstop clamp so a stale offset (e.g. a resize
/// between the keypress and this frame) can never show blank space past the
/// end.
fn render_body(
    model: &Model,
    pr: &PR,
    description: &[Line<'static>],
    scroll: u16,
    frame: &mut Frame<'_>,
    area: Rect,
    now: DateTime<Utc>,
) {
    let mut lines: Vec<Line<'static>> = description.to_vec();
    lines.extend(checks_section_lines(model, pr));
    lines.extend(threads_section_lines(model, pr, now));
    lines.extend(conversation_section_lines(model, pr, now));

    let content_lines = lines.len() as u16;
    let viewport_rows = area.height;
    let max_scroll = content_lines.saturating_sub(viewport_rows);
    let scroll = scroll.min(max_scroll);

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
