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
use unicode_width::UnicodeWidthStr;

use crate::{
    blocker::{ThreadKind, classify_thread},
    format::{
        CHECK_INDENT, check_cell_spans, check_cell_width, checks_summary, format_age,
        overflow_line, sorted_check_runs,
    },
    github::rest::PR,
    github::types::FullReviewThread,
    markdown::{self, Block},
    palette::DARK,
};

use super::{
    detail_items,
    model::{DetailState, Model},
};

/// The fixed rows of the detail view's pinned header that are always present:
/// title, meta, branch+mergeable/worktree, URL, divider. The Label Chip band
/// (zero or more wrapped rows) sits above the divider and is added on top of
/// this base by [`header_height`] when the PR has labels.
pub(crate) const HEADER_BASE_HEIGHT: u16 = 5;

/// The chrome rows a label-less PR reserves around the scrollable body: the
/// fixed [`HEADER_BASE_HEIGHT`] plus the 1-row status bar. Equals
/// `chrome_rows(pr, width)` whenever the PR has no Label Chips (its chip band
/// is then zero rows), so it is the base case [`chrome_rows`] names and the
/// fallback `update`'s scroll clamp uses for a PR no longer in the list.
pub(crate) const BASE_CHROME_ROWS: u16 = HEADER_BASE_HEIGHT + 1;

/// Total rows in the detail view's pinned header for `pr` at `width`: the fixed
/// [`HEADER_BASE_HEIGHT`] plus the number of rows the PR's Label Chips wrap onto
/// (none when it has no labels). The header draws from the list PR, which is
/// always available, so it shows even while the body fetch is in flight.
///
/// The single source of truth for the header's height, shared by
/// `view::detail`'s layout, `update`'s scroll clamp ([`chrome_rows`]), and its
/// mouse hit-testing, so the three can't disagree on where the body begins.
pub(crate) fn header_height(pr: &PR, width: u16) -> u16 {
    HEADER_BASE_HEIGHT.saturating_add(chip_band_rows(pr, width))
}

/// The number of header rows the PR's Label Chips occupy at `width` — zero when
/// the PR has no labels. The chips wrap with the same packer the chip render
/// uses, so the reserved band always matches what is painted.
fn chip_band_rows(pr: &PR, width: u16) -> u16 {
    if pr.labels.is_empty() {
        return 0;
    }
    let rows = crate::chip::chip_rows(&pr.labels, usize::from(width)).len();
    u16::try_from(rows).unwrap_or(u16::MAX)
}

/// Fixed chrome rows the detail view reserves around the scrollable body for
/// `pr` at `width`: the pinned [`header_height`] plus the 1-row status bar (the
/// `Constraint::Length(1)` in `view::detail::render`'s `Layout::vertical`).
/// The single source of truth shared by that layout and `update`'s scroll
/// clamp, which subtracts it from the terminal height to derive the same body
/// viewport. Mirrors how `Model::chrome_rows` is shared between
/// `sync_viewport` and `view::view`.
pub(crate) fn chrome_rows(pr: &PR, width: u16) -> u16 {
    header_height(pr, width).saturating_add(1)
}

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

/// Upper bound on the detail checks grid's column count. The grid grows as many
/// columns as the body width fits (see [`grid_columns`]), but stops here so an
/// ultrawide terminal stays scannable rather than smearing checks across the
/// whole row. The narrower summary panel always uses one column (its own
/// `summary::checks_lines`).
pub(crate) const MAX_GRID_COLUMNS: usize = 6;

/// Rows of checks the detail grid draws before the remainder collapses into a
/// `+N more` overflow line. The visible cap is `columns × MAX_GRID_ROWS`, so a
/// wider terminal (more columns) shows more checks while the vertical footprint
/// stays bounded.
pub(crate) const MAX_GRID_ROWS: usize = 4;

/// Blank columns between two adjacent grid columns. The column stride is the
/// widest check cell plus this gap, so the columns stay packed near the left
/// instead of being spread to the body's edges on a wide terminal.
const CHECKS_GRID_GAP: usize = 2;

/// How many grid columns of `cell_width`-wide check cells fit in `width`: the
/// [`CHECK_INDENT`] sits in front, then each column takes the cell plus a
/// [`CHECKS_GRID_GAP`] — except the last, which needs no trailing gap. Clamped
/// to at least one column and at most [`MAX_GRID_COLUMNS`].
fn grid_columns(width: u16, cell_width: usize) -> usize {
    let stride = (cell_width + CHECKS_GRID_GAP).max(1);
    let usable = usize::from(width).saturating_sub(CHECK_INDENT.len());
    // n columns occupy n·stride − GAP (the last column drops its trailing gap),
    // so the largest n that fits is ⌊(usable + GAP) / stride⌋.
    let fit = (usable + CHECKS_GRID_GAP) / stride;
    fit.clamp(1, MAX_GRID_COLUMNS)
}

/// Build the CI checks section lines: blank separator + bold header with
/// pass/fail/pending counts (over ALL checks) + a grid of checks of any outcome,
/// ordered failing-first then slowest, with a `+N more` overflow line beyond the
/// visible cap. The grid grows as many columns as the body width fits (capped at
/// [`MAX_GRID_COLUMNS`]) and shows `columns × MAX_GRID_ROWS` checks, so a wide
/// terminal shows more. Returns an empty `Vec` when checks haven't arrived for
/// this PR's commit or the check list is empty. Shares the `sorted_check_runs`
/// ordering and `check_cell_spans` content with `summary::checks_lines`; only
/// the column count and cap differ.
///
/// Part of the measured layout: the checks section is appended to the
/// description per-frame (so late-arriving checks show without a re-fetch),
/// so the true content height — and thus the max scroll offset — includes it.
fn checks_section_lines(model: &Model, pr: &PR, width: u16) -> Vec<Line<'static>> {
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

    // Checks of any outcome, ordered failing-first then slowest, laid out
    // row-major into as many columns as the body width fits. The header counts
    // above still tally ALL checks.
    let sorted = sorted_check_runs(checks);
    // Size the columns to the content: the widest check cell (plus a gap) is the
    // column stride, so the columns sit packed near the left rather than flung
    // to the body's edges on a wide terminal. Measure the widest over only the
    // checks that could be shown (a top-priority prefix) so a long name ranked
    // past the cap can't widen — and so thin out — the visible columns.
    let candidate = &sorted[..sorted.len().min(MAX_GRID_COLUMNS * MAX_GRID_ROWS)];
    let widest_cell = candidate
        .iter()
        .map(|c| check_cell_width(c))
        .max()
        .unwrap_or(0);
    let columns = grid_columns(width, widest_cell);
    let stride = widest_cell + CHECKS_GRID_GAP;

    // Show `columns × MAX_GRID_ROWS` checks; the rest collapse into `+N more`.
    let cap = columns * MAX_GRID_ROWS;
    let overflow = sorted.len().saturating_sub(cap);
    let visible = &sorted[..sorted.len().min(cap)];

    for row in visible.chunks(columns) {
        // Two-space indent in front of the first column so the grid matches the
        // summary panel's single column (the shared `check_row` look) — see
        // `CHECK_INDENT`.
        let mut spans: Vec<Span<'static>> = vec![Span::raw(CHECK_INDENT)];
        for (col, check) in row.iter().enumerate() {
            // The grid sizes its columns to the widest cell, so cells fit by
            // construction — no truncation (a long `workflow / job` instead
            // reduces the column count via `grid_columns`).
            let cell = check_cell_spans(check, usize::MAX);
            // Pad every cell but the row's last out to the column stride so the
            // next column's content aligns. The trailing cell is left unpadded.
            if col + 1 < row.len() {
                let used = cell.iter().map(|s| s.content.width()).sum::<usize>();
                spans.extend(cell);
                if used < stride {
                    spans.push(Span::raw(" ".repeat(stride - used)));
                }
            } else {
                spans.extend(cell);
            }
        }
        lines.push(Line::from(spans));
    }
    if overflow > 0 {
        lines.push(overflow_line(overflow, DARK.muted));
    }
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

    content.lines.extend(checks_section_lines(model, pr, width));

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::rest::Label;
    use crate::github::types::PRState;

    fn pr_with_labels(labels: Vec<Label>) -> PR {
        PR {
            number: 1,
            repo_slug: "acme/web".to_owned(),
            title: "t".to_owned(),
            author: "a".to_owned(),
            created_at: DateTime::UNIX_EPOCH,
            updated_at: DateTime::UNIX_EPOCH,
            additions: 0,
            deletions: 0,
            is_draft: false,
            labels,
            requested_reviewers: Vec::new(),
            assignees: Vec::new(),
            review_decision: String::new(),
            mergeable: "UNKNOWN".to_owned(),
            last_commit_date: None,
            head_commit_sha: None,
            review_status_loaded: false,
            head_ref: "h".to_owned(),
            base_ref: "b".to_owned(),
            head_repository_owner: "acme".to_owned(),
            state: PRState::Open,
        }
    }

    fn label(name: &str) -> Label {
        Label {
            name: name.to_owned(),
            color: None,
        }
    }

    #[test]
    fn header_height_is_the_base_when_the_pr_has_no_labels() {
        let pr = pr_with_labels(Vec::new());
        assert_eq!(header_height(&pr, 80), HEADER_BASE_HEIGHT);
        assert_eq!(chrome_rows(&pr, 80), BASE_CHROME_ROWS);
    }

    #[test]
    fn header_height_grows_by_the_label_chip_band() {
        let pr = pr_with_labels(vec![label("a"), label("bb")]);
        // " a " (3) + gap (1) + " bb " (4) = 8 columns; at width 80 the two chips
        // share one band row, so the header is the base plus one.
        assert_eq!(header_height(&pr, 80), HEADER_BASE_HEIGHT + 1);
        // At a narrow width the chips wrap onto two band rows, growing the header
        // and the chrome the scroll clamp reserves in lockstep.
        assert_eq!(header_height(&pr, 4), HEADER_BASE_HEIGHT + 2);
        assert_eq!(chrome_rows(&pr, 4), BASE_CHROME_ROWS + 2);
    }
}
