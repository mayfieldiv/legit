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
    style::Style,
    text::{Line, Span},
};

use crate::{
    blocker::{ThreadKind, classify_thread},
    format::{check_row, checks_summary, format_age, sort_check_runs},
    github::rest::PR,
    github::types::{CheckRun, FullReviewThread},
    markdown::{self, Block},
    palette::DARK,
};

use super::{
    detail_items,
    model::{DetailState, Model},
};

/// Number of rows in the detail view's pinned header: title, meta, URL,
/// branch+mergeable/worktree, divider. Constant — the header draws from the
/// list PR, which is always available, so it shows even while the body fetch is
/// in flight.
pub(crate) const HEADER_HEIGHT: u16 = 5;

/// Fixed chrome rows the detail view reserves around the scrollable body: the
/// pinned `HEADER_HEIGHT` plus the 1-row status bar (the
/// `Constraint::Length(1)` in `view::detail::render`'s `Layout::vertical`).
/// The single source of truth shared by that layout and `update`'s scroll
/// clamp, which subtracts it from the terminal height to derive the same body
/// viewport. Mirrors how `Model::chrome_rows` is shared between
/// `sync_viewport` and `view::view`.
pub(crate) const CHROME_ROWS: u16 = HEADER_HEIGHT + 1;

/// Sentinel key under which the PR description's `<details>` expansion state
/// lives in `DetailState::expanded`. The description has no comment URL, and
/// every real key is a non-empty GitHub comment URL, so the empty string can't
/// collide with one.
pub(crate) const BODY_DETAILS_KEY: &str = "";

/// Parse the PR description to markdown blocks: the body's blocks, or a single
/// muted "No description." line when blank. Pure (no model/area): called once
/// on `Msg::PRDetailArrived` so the markdown is parsed a single time and the
/// blocks cached in `DetailState::body`, then flattened (per the body's
/// `<details>` expansion) per frame rather than re-parsed.
pub(crate) fn render_description_blocks(body: &str) -> Vec<Block> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        vec![Block::Lines(vec![Line::from(Span::styled(
            "No description.",
            Style::default().fg(DARK.muted),
        ))])]
    } else {
        markdown::render_blocks(trimmed)
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
            Style::default().fg(DARK.muted),
        ),
    ];
    if summary.failed > 0 {
        header_spans.push(Span::styled(
            format!(" · {} failed", summary.failed),
            Style::default().fg(DARK.failing),
        ));
    }
    if summary.pending > 0 {
        header_spans.push(Span::styled(
            format!(" · {} pending", summary.pending),
            Style::default().fg(DARK.pending),
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
        ThreadKind::Resolved => Span::styled(" ✓ resolved", Style::default().fg(DARK.passing)),
        ThreadKind::AwaitingReviewer => {
            Span::styled(" ◐ awaiting reviewer", Style::default().fg(DARK.accent))
        }
        ThreadKind::Unreplied => Span::styled(" ● unreplied", Style::default().fg(DARK.pending)),
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
    let author_color = if is_bot { DARK.muted } else { DARK.author };
    let mut spans = vec![Span::styled(
        author.to_owned(),
        Style::default().fg(author_color),
    )];
    if is_bot {
        spans.push(Span::styled(" [bot]", Style::default().fg(DARK.muted)));
    }
    spans.push(Span::styled(
        format!(" · {}", format_age(created_at, now)),
        Style::default().fg(DARK.muted),
    ));
    Line::from(spans)
}

/// The intro lines for a card section (Review Threads / Conversation): a
/// loading placeholder while its fetch hasn't landed (`counts` is `None`),
/// nothing once it arrived empty, and otherwise a blank separator plus a
/// header counting shown and filter-hidden items — with an all-hidden note
/// when the filters conceal every item. One builder for both sections so
/// their intros can't drift apart (the Conversation header once rendered a
/// dangling zero count, with no hidden tally, when every comment was
/// bot-filtered).
fn section_intro(
    title: &'static str,
    counts: Option<(usize, usize)>,
    loading: &'static str,
    all_hidden: &'static str,
) -> Vec<Line<'static>> {
    let muted = Style::default().fg(DARK.muted);
    let Some((total, visible)) = counts else {
        return vec![
            Line::from(""),
            Line::from(Span::styled(loading, Style::default().fg(DARK.pending))),
        ];
    };
    if total == 0 {
        return Vec::new();
    }
    let mut spans = vec![
        markdown::heading_span(2, title),
        Span::styled(format!(" {visible} shown"), muted),
    ];
    if total > visible {
        spans.push(Span::styled(
            format!(" · {} hidden", total - visible),
            muted,
        ));
    }
    let mut lines = vec![Line::from(""), Line::from(spans)];
    if visible == 0 {
        lines.push(Line::from(Span::styled(all_hidden, muted)));
    }
    lines
}

/// The detail body's complete content: every display line plus, for each
/// focusable item (index-aligned with `DetailItems::focusable`), the line
/// range its card occupies. One builder shared by the view (rendering,
/// focused border) and `update` (scroll clamp, scroll-into-view on focus
/// moves), so what's focusable, where it sits, and what's drawn can't drift.
pub(crate) struct DetailContent {
    pub lines: Vec<Line<'static>>,
    pub item_ranges: Vec<std::ops::Range<usize>>,
    /// Index of the most recently pushed card's bottom-border row, valid only
    /// while it is still the last line — the next `push_card` shares it as
    /// its own top-border row, so adjacent cards sit one row apart. Section
    /// headers and placeholders push lines without updating this, which
    /// naturally breaks the sharing across section boundaries.
    last_card_bottom: Option<usize>,
}

impl DetailContent {
    /// Append one focusable card, recording its line range. A card's focus
    /// index IS the number of ranges already recorded — cards are pushed in
    /// the exact order `DetailItems::focusable` flattens — so the card is
    /// focused when that count equals `focused_index`; the alignment can't
    /// drift because it's never stated twice.
    ///
    /// The card occupies the same rows focused or not (a top and bottom border
    /// row plus a 2-char left gutter per content row) — focused cards draw the
    /// border characters, unfocused cards draw spaces, so focus changes never
    /// shift the layout. A card directly following another card shares one
    /// separator row with it: this card's top-border row IS the previous
    /// card's bottom-border row. At most one of the two is focused, so the
    /// shared row carries that card's border — or stays blank. Mirrors the TS
    /// `FocusableCard`'s invisible-border trick and its `marginTop: -1`
    /// overlap (where the focused card's border wins via zIndex).
    fn push_card(
        &mut self,
        card: Vec<Line<'static>>,
        focused_index: usize,
        indent: usize,
        width: u16,
    ) {
        let focused = self.item_ranges.len() == focused_index;
        let pad = " ".repeat(indent);
        let border = Style::default().fg(DARK.separator);
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
        let shares_separator = self.last_card_bottom.is_some()
            && self.last_card_bottom == self.lines.len().checked_sub(1);
        let start = if shares_separator {
            let shared = self.lines.len() - 1;
            if focused {
                self.lines[shared] = top;
            }
            shared
        } else {
            self.lines.push(top);
            self.lines.len() - 1
        };
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
        self.last_card_bottom = Some(self.lines.len() - 1);
        self.item_ranges.push(start..self.lines.len());
    }
}

/// Build the full detail body content: the cached description (item 0), the CI
/// checks section, then the Review Threads and Conversation sections as
/// focusable cards filtered by `Model::detail_filters`. Sections whose fetch
/// hasn't landed render a loading placeholder; arrived-empty sections render
/// nothing. Each card's `<details>` groups fold or unfold per its entry in
/// `detail.expanded` (Enter toggles); a card body longer than
/// `COLLAPSED_CARD_BODY_ROWS` is additionally capped as an unconditional
/// backstop for pathological bodies.
pub(crate) fn detail_content(
    model: &Model,
    pr: &PR,
    description: &[Block],
    detail: &DetailState,
    width: u16,
    now: DateTime<Utc>,
) -> DetailContent {
    let focused_index = detail.focus.index();
    let key = pr.key();
    let items = detail_items::DetailItems::derive(
        model.enrichment.threads_for(&key),
        model.enrichment.comments_for(&key),
        model.detail_filters(),
    );
    let mut content = DetailContent {
        lines: Vec::new(),
        item_ranges: Vec::new(),
        last_card_bottom: None,
    };

    // Item 0: the body. Its `<details>` groups fold per the body's own
    // expansion entry (keyed by the URL-less `BODY_DETAILS_KEY` sentinel), then
    // the flattened lines wrap to the terminal width here at derivation time
    // (the parsed blocks are width-independent). Unstyled — no border even
    // focused, matching the TS DetailView where only thread/reply/comment
    // cards are framed.
    let body_expanded = detail.expanded.contains(BODY_DETAILS_KEY);
    let description = markdown::flatten_blocks(description, body_expanded);
    let description = crate::wrap::wrap_lines(description, width as usize);
    content.item_ranges.push(0..description.len());
    content.lines.extend(description);

    content.lines.extend(checks_section_lines(model, pr));

    // ── Review Threads ──
    content.lines.extend(section_intro(
        "Review Threads",
        items
            .threads
            .as_ref()
            .map(|section| (section.total, section.groups.len())),
        "Loading threads…",
        "All threads resolved or hidden.",
    ));
    for group in items.threads.iter().flat_map(|section| &section.groups) {
        let (thread, root) = (group.thread, group.root);
        let location = match thread.line {
            Some(line) => format!("{}:{line}", thread.path),
            None => thread.path.clone(),
        };
        let mut card = vec![
            Line::from(vec![
                Span::styled(location, Style::default().fg(DARK.accent)),
                thread_badge(thread),
            ]),
            comment_byline(&root.author, root.is_bot, root.created_at, now),
        ];
        card.extend(card_body(model, detail, &root.url, 0, width));
        content.push_card(card, focused_index, 0, width);

        // Replies: each visible comment after the root is its own indented
        // card with an ↳ byline (mirrors the TS `ReplyRow`).
        for comment in &group.replies {
            let mut byline =
                comment_byline(&comment.author, comment.is_bot, comment.created_at, now);
            byline
                .spans
                .insert(0, Span::styled("↳ ", Style::default().fg(DARK.muted)));
            let mut card = vec![byline];
            card.extend(card_body(model, detail, &comment.url, 2, width));
            content.push_card(card, focused_index, 2, width);
        }
    }

    // ── Conversation ──
    content.lines.extend(section_intro(
        "Conversation",
        items
            .comments
            .as_ref()
            .map(|section| (section.total, section.visible.len())),
        "Loading comments…",
        "All comments hidden.",
    ));
    for comment in items.comments.iter().flat_map(|section| &section.visible) {
        let mut card = vec![comment_byline(
            &comment.author,
            comment.is_bot,
            comment.created_at,
            now,
        )];
        card.extend(card_body(model, detail, &comment.url, 0, width));
        content.push_card(card, focused_index, 0, width);
    }

    debug_assert_eq!(
        content.item_ranges.len(),
        items.focusable_len(),
        "every focusable item must have a recorded line range"
    );
    content
}

/// Hard cap on a card's body rows. An unconditional backstop for pathological
/// bodies (multi-thousand-line bot dumps, pasted logs), not an everyday fold —
/// ordinary long comments render in full. Enter toggles the card's `<details>`
/// groups (the TS `details-store` port), not this cap, so a body past the cap
/// stays truncated; the cap only guards against a single comment swamping the
/// scroll height.
const COLLAPSED_CARD_BODY_ROWS: usize = 100;

/// One card's body lines: the parse-once cached markdown flattened for the
/// card's `<details>` expansion state, capped at `COLLAPSED_CARD_BODY_ROWS` as a
/// backstop, then wrapped to the card's content width — the full width minus the
/// indent and the 2-column gutter `push_card` prepends. Wrapping happens here at
/// derivation time (the parsed blocks are width-independent), so the scroll
/// clamp and card ranges measure exactly the rows the view paints. Bylines and
/// section headers deliberately don't wrap — they clip, like the TS `truncate`
/// rows.
fn card_body(
    model: &Model,
    detail: &DetailState,
    comment_url: &str,
    indent: usize,
    width: u16,
) -> Vec<Line<'static>> {
    let expanded = detail.expanded.contains(comment_url);
    let body = collapse_body(model.enrichment.rendered_body(comment_url, expanded));
    crate::wrap::wrap_lines(body, (width as usize).saturating_sub(indent + 2))
}

/// Cap a card's body at `COLLAPSED_CARD_BODY_ROWS` lines, appending a muted
/// marker advertising the hidden tail. An unconditional backstop — see
/// `COLLAPSED_CARD_BODY_ROWS`.
fn collapse_body(body: Vec<Line<'static>>) -> Vec<Line<'static>> {
    if body.len() <= COLLAPSED_CARD_BODY_ROWS {
        return body;
    }
    let hidden = body.len() - COLLAPSED_CARD_BODY_ROWS;
    let mut out = body;
    out.truncate(COLLAPSED_CARD_BODY_ROWS);
    out.push(Line::from(Span::styled(
        format!("… +{hidden} more lines (truncated)"),
        Style::default().fg(DARK.muted),
    )));
    out
}
