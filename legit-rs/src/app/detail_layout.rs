//! The detail body's layout derivation: every display line of the scrollable
//! body (description, CI checks, Review Threads and Conversation cards) plus
//! the line range each focusable card occupies.
//!
//! Pure derivation with no painting — `view::detail` renders the lines it
//! produces, while `update` measures them (scroll clamp, scroll-into-view on
//! focus moves). Living beside `detail_items` in the app layer keeps the
//! reducer free of any dependency on the view modules: what's focusable, where
//! it sits, and what's drawn all derive from one place and can't drift.

use chrono::{DateTime, Utc};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use crate::{
    blocker::{ThreadKind, classify_thread},
    format::{check_row, checks_summary, format_age, sort_check_runs},
    github::rest::PR,
    github::types::{CheckRun, FullReviewThread},
    markdown,
};

use super::{
    detail_items,
    model::{DetailState, Model},
};

/// Number of rows in the detail view's pinned header: title, meta, URL,
/// branch+mergeable, divider. Constant — the header draws from the list PR,
/// which is always available, so it shows even while the body fetch is in
/// flight.
pub(crate) const HEADER_HEIGHT: u16 = 5;

/// Fixed chrome rows the detail view reserves around the scrollable body: the
/// pinned `HEADER_HEIGHT` plus the 1-row status bar (the
/// `Constraint::Length(1)` in `view::detail::render`'s `Layout::vertical`).
/// The single source of truth shared by that layout and `update`'s scroll
/// clamp, which subtracts it from the terminal height to derive the same body
/// viewport. Mirrors how `Model::chrome_rows` is shared between
/// `sync_viewport` and `view::view`.
pub(crate) const CHROME_ROWS: u16 = HEADER_HEIGHT + 1;

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
/// Part of the measured layout: the checks section is appended to the
/// description per-frame (so late-arriving checks show without a re-fetch),
/// so the true content height — and thus the max scroll offset — includes it.
fn checks_section_lines(model: &Model, pr: &PR) -> Vec<Line<'static>> {
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

/// A muted in-flight placeholder for a section whose fetch hasn't landed,
/// separated from the previous section by a blank line.
fn loading_placeholder(text: &'static str) -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        Line::from(Span::styled(text, Style::default().fg(Color::Yellow))),
    ]
}

/// The detail body's complete content: every display line plus, for each
/// focusable item (index-aligned with `detail_items::focusable_items`), the
/// line range its card occupies. One builder shared by the view (rendering,
/// focused border) and `update` (scroll clamp, scroll-into-view on focus
/// moves), so what's focusable, where it sits, and what's drawn can't drift.
pub(crate) struct DetailContent {
    pub lines: Vec<Line<'static>>,
    pub item_ranges: Vec<std::ops::Range<usize>>,
}

impl DetailContent {
    /// Append one focusable card, recording its line range. The card occupies
    /// the same rows focused or not (a top and bottom border row plus a 2-char
    /// left gutter per content row) — focused cards draw the border characters,
    /// unfocused cards draw spaces, so focus changes never shift the layout.
    /// Mirrors the TS `FocusableCard`'s invisible-border trick.
    fn push_card(&mut self, card: Vec<Line<'static>>, focused: bool, indent: usize, width: u16) {
        let start = self.lines.len();
        let pad = " ".repeat(indent);
        let border = Style::default().fg(Color::DarkGray);
        // Horizontal rule spanning the row minus the indent and two corners.
        let rule = "─".repeat((width as usize).saturating_sub(indent + 2));
        let (top, bottom) = if focused {
            (
                Line::from(Span::styled(format!("{pad}╭{rule}╮"), border)),
                Line::from(Span::styled(format!("{pad}╰{rule}╯"), border)),
            )
        } else {
            (Line::from(""), Line::from(""))
        };
        self.lines.push(top);
        for mut line in card {
            let gutter = if focused {
                Span::styled(format!("{pad}│ "), border)
            } else {
                Span::raw(format!("{pad}  "))
            };
            line.spans.insert(0, gutter);
            self.lines.push(line);
        }
        self.lines.push(bottom);
        self.item_ranges.push(start..self.lines.len());
    }
}

/// Build the full detail body content: the cached description (item 0), the CI
/// checks section, then the Review Threads and Conversation sections as
/// focusable cards filtered by `Model::detail_filters`. Sections whose fetch
/// hasn't landed render a loading placeholder; arrived-empty sections render
/// nothing. Card bodies longer than `COLLAPSED_CARD_BODY_ROWS` collapse unless
/// their URL is in `detail.expanded` (Enter toggles).
pub(crate) fn detail_content(
    model: &Model,
    pr: &PR,
    description: &[Line<'static>],
    detail: &DetailState,
    width: u16,
    now: DateTime<Utc>,
) -> DetailContent {
    let focused_index = detail.focused_index;
    let key = pr.key();
    let filters = model.detail_filters();
    let threads = model.enrichment.threads_for(&key);
    let comments = model.enrichment.comments_for(&key);
    let mut content = DetailContent {
        lines: Vec::new(),
        item_ranges: Vec::new(),
    };

    // The cards come straight from the focusable-items sequence, so the
    // recorded ranges align with the focus indices by construction.
    let items =
        detail_items::focusable_items(threads.unwrap_or(&[]), comments.unwrap_or(&[]), filters);

    // Item 0: the body. Unstyled — no border even focused, matching the TS
    // DetailView where only thread/reply/comment cards are framed.
    content.item_ranges.push(0..description.len());
    content.lines.extend_from_slice(description);

    content.lines.extend(checks_section_lines(model, pr));

    // ── Review Threads ──
    match threads {
        None => content
            .lines
            .extend(loading_placeholder("Loading threads…")),
        Some([]) => {}
        Some(threads) => {
            let visible = detail_items::visible_threads(threads, filters).len();
            content.lines.push(Line::from(""));
            content.lines.push(threads_header(threads.len(), visible));
            if visible == 0 {
                content.lines.push(Line::from(Span::styled(
                    "All threads resolved or hidden.",
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
    }
    for (index, item) in items.iter().enumerate() {
        let focused = index == focused_index;
        match item {
            detail_items::FocusableItem::Thread { thread, root } => {
                let location = match thread.line {
                    Some(line) => format!("{}:{line}", thread.path),
                    None => thread.path.clone(),
                };
                let mut card = vec![
                    Line::from(vec![
                        Span::styled(location, Style::default().fg(Color::Cyan)),
                        thread_badge(thread),
                    ]),
                    comment_byline(&root.author, root.is_bot, root.created_at, now),
                ];
                card.extend(collapse_body(
                    markdown::render(&root.body),
                    detail.expanded.contains(&root.url),
                ));
                content.push_card(card, focused, 0, width);
            }
            // Replies: each visible comment after the root is its own indented
            // card with an ↳ byline (mirrors the TS `ReplyRow`).
            detail_items::FocusableItem::Reply { comment } => {
                let mut byline =
                    comment_byline(&comment.author, comment.is_bot, comment.created_at, now);
                byline
                    .spans
                    .insert(0, Span::styled("↳ ", Style::default().fg(Color::DarkGray)));
                let mut card = vec![byline];
                card.extend(collapse_body(
                    markdown::render(&comment.body),
                    detail.expanded.contains(&comment.url),
                ));
                content.push_card(card, focused, 2, width);
            }
            detail_items::FocusableItem::Body | detail_items::FocusableItem::Comment { .. } => {}
        }
    }

    // ── Conversation ──
    match comments {
        None => content
            .lines
            .extend(loading_placeholder("Loading comments…")),
        Some([]) => {}
        Some(comments) => {
            content.lines.push(Line::from(""));
            content.lines.push(conversation_header(
                detail_items::visible_comments(comments, filters).len(),
            ));
        }
    }
    for (index, item) in items.iter().enumerate() {
        if let detail_items::FocusableItem::Comment { comment } = item {
            let mut card = vec![comment_byline(
                &comment.author,
                comment.is_bot,
                comment.created_at,
                now,
            )];
            card.extend(collapse_body(
                markdown::render(&comment.body),
                detail.expanded.contains(&comment.url),
            ));
            content.push_card(card, index == focused_index, 0, width);
        }
    }

    debug_assert_eq!(
        content.item_ranges.len(),
        items.len(),
        "every focusable item must have a recorded line range"
    );
    content
}

/// Rows of a card's markdown body shown while collapsed. Long comment bodies
/// (bot dumps, pasted logs) would otherwise dominate the page; Enter expands
/// the focused card (the TUI stand-in for the TS `details-store` toggle).
const COLLAPSED_CARD_BODY_ROWS: usize = 6;

/// Cap a card's body at `COLLAPSED_CARD_BODY_ROWS` lines unless expanded,
/// appending a muted marker advertising the hidden tail.
fn collapse_body(body: Vec<Line<'static>>, expanded: bool) -> Vec<Line<'static>> {
    if expanded || body.len() <= COLLAPSED_CARD_BODY_ROWS {
        return body;
    }
    let hidden = body.len() - COLLAPSED_CARD_BODY_ROWS;
    let mut out = body;
    out.truncate(COLLAPSED_CARD_BODY_ROWS);
    out.push(Line::from(Span::styled(
        format!("… +{hidden} more lines — enter expands"),
        Style::default().fg(Color::DarkGray),
    )));
    out
}

/// The Review Threads section header: visible count plus a hidden count when
/// the resolved/bot filters are concealing threads.
fn threads_header(total: usize, visible: usize) -> Line<'static> {
    let mut spans = vec![
        markdown::heading_span(2, "Review Threads"),
        Span::styled(
            format!(" {visible} shown"),
            Style::default().fg(Color::DarkGray),
        ),
    ];
    let hidden = total - visible;
    if hidden > 0 {
        spans.push(Span::styled(
            format!(" · {hidden} hidden"),
            Style::default().fg(Color::DarkGray),
        ));
    }
    Line::from(spans)
}

/// The Conversation section header with its visible-comment count.
fn conversation_header(visible: usize) -> Line<'static> {
    let plural = if visible == 1 { "" } else { "s" };
    Line::from(vec![
        markdown::heading_span(2, "Conversation"),
        Span::styled(
            format!(" {visible} comment{plural}"),
            Style::default().fg(Color::DarkGray),
        ),
    ])
}
