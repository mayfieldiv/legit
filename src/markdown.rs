//! Pure markdown renderer. Port of the TS `src/lib/markdown.tsx`: maps
//! `pulldown-cmark` events to ratatui `Line`/`Span` with the same feature set
//! as the TS implementation.
//!
//! Rendering is two-staged so `<details>` groups can fold without re-parsing.
//! `render_blocks(source)` parses to a `Vec<Block>` — runs of display lines,
//! with each `<details>...</details>` captured as a collapsible `Details`
//! block (port of the TS `groupDetailsBlocks`). `flatten_blocks(blocks,
//! expanded)` then resolves that to `Vec<Line<'static>>` for a given card-wide
//! expansion state. A body with no `<details>` yields exactly one `Lines`
//! block, so its flattened output is identical to the old flat renderer.
//! Each `Line` owns its spans (all strings are `'static`), so callers have no
//! lifetime dependency on the source string.
//!
//! Supported block types: headings (h1–h3+ with decreasing visual weight),
//! paragraphs, fenced code blocks (language tag + visual delineation),
//! ordered and unordered lists (nested), blockquotes, and thematic breaks.
//! Supported inline styles: bold, italic, inline code, links (text + URL),
//! images ([image: alt/url] fallback).

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

mod details_html;
use details_html::{DetailsToken, tokenize_details};

use crate::palette::DARK;

// ── Colour roles ──────────────────────────────────────────────────────────────
//
// Markdown reads three roles from the curated palette: `accent` (headings,
// section labels, the `<details>` marker), `muted` (code fences, blockquote
// bar, thematic breaks, link/image suffixes) and `code` (code-block bodies and
// inline code). Rendered colours are baked into the `Line`s at parse time and
// cached in the model, so this pure renderer reads the single `DARK` instance
// rather than taking a per-call palette argument.

// ── Public API ────────────────────────────────────────────────────────────────

/// A rendered markdown body as a sequence of blocks. Most content is a single
/// `Lines` run; a `<details>...</details>` group becomes a `Details` block so
/// the detail view can fold and unfold it without re-parsing. The renderer
/// ignores every other HTML construct (as the prior flat renderer did), so a
/// body with no `<details>` yields exactly one `Lines` block.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    /// A run of fully-rendered display lines (no toggle).
    Lines(Vec<Line<'static>>),
    /// A collapsible `<details>` group: its `<summary>` text plus the inner
    /// content as nested blocks (which may hold further `Details` groups).
    Details {
        summary: String,
        children: Vec<Block>,
    },
}

/// Parse markdown into blocks, grouping each `<details>...</details>` run into a
/// `Details` block (port of the TS `groupDetailsBlocks`). The content between a
/// `<details>` open and its matching `</details>` is rendered as its own block
/// sequence, so it keeps full markdown formatting. An unclosed `<details>`
/// folds the rest of the document into the group, and a stray `</details>` is
/// ignored — matching the TS reference. Non-details HTML is dropped, exactly as
/// the prior flat renderer dropped all HTML.
pub fn render_blocks(source: &str) -> Vec<Block> {
    // A stack of open frames: index 0 is the document, each deeper entry an
    // open `<details>`. Non-HTML events feed the innermost frame's render
    // context; a `<details>` open/close pushes/pops a frame.
    let mut frames: Vec<Frame> = vec![Frame::document()];
    let mut html = String::new();
    let mut in_html_block = false;
    for event in Parser::new_ext(source, Options::empty()) {
        match event {
            // HTML arrives one block at a time (`Start`/`End(HtmlBlock)` with
            // one or more `Html` events between); accumulate the whole block so
            // a `<details>` and its `<summary>` on separate lines are seen as
            // one string before tokenising.
            Event::Start(Tag::HtmlBlock) => {
                in_html_block = true;
                html.clear();
            }
            Event::End(TagEnd::HtmlBlock) => {
                in_html_block = false;
                apply_html_tokens(&html, &mut frames);
            }
            Event::Html(text) if in_html_block => html.push_str(&text),
            other => frames
                .last_mut()
                .expect("frame stack always has the document frame")
                .ctx
                .handle(other),
        }
    }
    // Fold any unclosed `<details>` into their parents (malformed input — the TS
    // grouping likewise runs to the document end), then finish the document.
    while frames.len() > 1 {
        close_top_frame(&mut frames);
    }
    frames.pop().expect("the document frame").finish()
}

/// Flatten rendered blocks to display lines. Each `Details` group renders one
/// summary line — `▶ summary` collapsed, `▼ summary` expanded — and, when
/// `expanded`, its children indented two columns beneath it. `expanded` is the
/// card-wide toggle-all state Enter drives (the TS `toggleAll`): one boolean
/// per card, so every group in a card folds and unfolds together. Trailing
/// blank lines are trimmed so a card ends at its last content row (the layout
/// owns spacing between cards); internal blank separators between blocks are
/// preserved.
pub fn flatten_blocks(blocks: &[Block], expanded: bool) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    flatten_into(blocks, expanded, 0, &mut out);
    while out.last().is_some_and(line_is_blank) {
        out.pop();
    }
    out
}

/// Whether any block in the sequence is a `<details>` group — the test the
/// Enter handler uses to no-op on a card with nothing to toggle (matching the
/// TS `toggleAll` early-return). A nested group always sits inside a top-level
/// one, so a shallow scan suffices.
pub fn has_details(blocks: &[Block]) -> bool {
    blocks.iter().any(|b| matches!(b, Block::Details { .. }))
}

fn flatten_into(blocks: &[Block], expanded: bool, depth: usize, out: &mut Vec<Line<'static>>) {
    for (i, block) in blocks.iter().enumerate() {
        // One blank row separates adjacent sibling blocks, the same single
        // separator the renderer puts between every other block. `Frame::flush`
        // strips each block's own trailing blank, so this is the sole separator
        // at a `<details>` seam — without it a group would render flush against
        // its neighbours while plain blocks keep their breathing room. (Two
        // `Lines` blocks are never adjacent — a flush only ever precedes a
        // `Details` push — so this never doubles an existing internal blank.)
        if i > 0 {
            out.push(Line::default());
        }
        match block {
            Block::Lines(lines) => out.extend(lines.iter().map(|line| indent_line(line, depth))),
            Block::Details { summary, children } => {
                out.push(summary_line(summary, expanded, depth));
                if expanded {
                    flatten_into(children, expanded, depth + 1, out);
                }
            }
        }
    }
}

/// The `▶`/`▼ summary` line for a `<details>` group, indented for nesting. The
/// marker takes the accent colour; the summary text is bold (mirrors the TS
/// `MdDetails` styling).
fn summary_line(summary: &str, expanded: bool, depth: usize) -> Line<'static> {
    let marker = if expanded { "▼ " } else { "▶ " };
    let indent = "  ".repeat(depth);
    Line::from(vec![
        Span::styled(
            format!("{indent}{marker}"),
            Style::default().fg(DARK.accent),
        ),
        Span::styled(
            summary.to_owned(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ])
}

/// Clone a rendered line, prepending `depth` levels of two-space indent (the TS
/// `paddingLeft={2}` per nesting level). Zero depth returns the line unchanged.
fn indent_line(line: &Line<'static>, depth: usize) -> Line<'static> {
    if depth == 0 {
        return line.clone();
    }
    let mut cloned = line.clone();
    cloned.spans.insert(0, Span::raw("  ".repeat(depth)));
    cloned
}

/// True when the line renders as visually empty (no non-whitespace content).
fn line_is_blank(line: &Line<'_>) -> bool {
    line.spans.iter().all(|span| span.content.trim().is_empty())
}

// ── `<details>` grouping ────────────────────────────────────────────────────────

/// One open frame in `render_blocks`: the document, or an open `<details>`.
/// Holds the blocks finished so far plus a render context accumulating the
/// current run of plain lines (flushed to a `Lines` block at each boundary).
struct Frame {
    /// The `<summary>` text for a `<details>` frame; `None` for the document.
    summary: Option<String>,
    blocks: Vec<Block>,
    ctx: RenderCtx,
}

impl Frame {
    fn document() -> Self {
        Self {
            summary: None,
            blocks: Vec::new(),
            ctx: RenderCtx::default(),
        }
    }

    fn details(summary: Option<String>) -> Self {
        Self {
            summary,
            blocks: Vec::new(),
            ctx: RenderCtx::default(),
        }
    }

    /// Move the render context's accumulated lines into a `Lines` block
    /// (trailing blank separators trimmed, as the old flat renderer trimmed the
    /// whole document) and reset the context. An empty run adds no block.
    fn flush(&mut self) {
        let mut lines = std::mem::take(&mut self.ctx).lines;
        while lines.last().is_some_and(line_is_blank) {
            lines.pop();
        }
        if !lines.is_empty() {
            self.blocks.push(Block::Lines(lines));
        }
    }

    /// Flush pending lines, then append `block` after them so ordering holds.
    fn push_block(&mut self, block: Block) {
        self.flush();
        self.blocks.push(block);
    }

    /// Finish a `<details>` frame into its `Details` block, defaulting the
    /// summary to "Details" when the markup carried none.
    fn into_details(mut self) -> Block {
        self.flush();
        Block::Details {
            summary: self.summary.unwrap_or_else(|| "Details".to_owned()),
            children: self.blocks,
        }
    }

    /// Finish the document frame into its block list.
    fn finish(mut self) -> Vec<Block> {
        self.flush();
        self.blocks
    }
}

/// Pop the innermost open `<details>` frame, finish it into a `Details` block,
/// and append it to its parent. Caller guarantees `frames.len() > 1` (an open
/// `<details>` always sits above the document frame).
fn close_top_frame(frames: &mut Vec<Frame>) {
    let child = frames.pop().expect("a frame to close above the document");
    let block = child.into_details();
    frames
        .last_mut()
        .expect("a parent frame for every child")
        .push_block(block);
}

/// Apply one accumulated HTML block's `<details>` tokens to the frame stack:
/// each open flushes the current frame's pending lines and pushes a new
/// `<details>` frame; each close finalises the top frame into its parent. A
/// stray close (none open) is ignored. Other HTML in the block adds nothing.
fn apply_html_tokens(html: &str, frames: &mut Vec<Frame>) {
    for token in tokenize_details(html) {
        match token {
            DetailsToken::Open(summary) => {
                frames
                    .last_mut()
                    .expect("frame stack always has the document frame")
                    .flush();
                frames.push(Frame::details(summary));
            }
            DetailsToken::Close if frames.len() > 1 => close_top_frame(frames),
            DetailsToken::Close => {}
        }
    }
}

/// Return a `Span` that renders `text` as a heading at `depth`, prepending the
/// appropriate `# ` prefix (e.g. `"## "` for depth 2). Applies `heading_style`
/// so the accent colour and bold rule live in one place. The returned span is
/// `'static` (text is converted to an owned `String`).
///
/// Use this when constructing heading spans outside the markdown renderer (e.g.
/// synthesised section headers in other views) so that any future change to
/// heading styling only needs to be made here.
pub fn heading_span(depth: u8, text: impl Into<String>) -> Span<'static> {
    let prefix = "#".repeat(depth as usize);
    Span::styled(format!("{prefix} {}", text.into()), heading_style(depth))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Return the ratatui style for a heading at the given depth: accent always;
/// bold for h1/h2 (depth <= 2), plain accent for h3+.
fn heading_style(depth: u8) -> Style {
    let s = Style::default().fg(DARK.accent);
    if depth <= 2 {
        s.add_modifier(Modifier::BOLD)
    } else {
        s
    }
}

// ── Internal state machine ────────────────────────────────────────────────────

/// Tracks which inline modifiers are currently active. Bold and italic are
/// nesting depths, not booleans: CommonMark nests `Strong`/`Emphasis` tags
/// (e.g. `**a **b** c**`), so the inner `End` must decrement rather than
/// clear, leaving the outer style active. A modifier applies whenever its
/// depth is non-zero.
#[derive(Default, Clone)]
struct InlineStyle {
    bold: u32,
    italic: u32,
    /// Destination URL for the innermost Link tag, set on Start(Link) and
    /// consumed (taken) on End(Link) to emit " (url)".
    link_url: Option<String>,
    /// Destination URL for the current Image tag, set on Start(Image) and
    /// consumed (taken) on End(Image) as the fallback when alt text is empty.
    /// Kept separate from `link_url` so that a nested image-in-link does not
    /// overwrite the enclosing link's URL.
    image_url: Option<String>,
}

impl InlineStyle {
    fn to_ratatui(&self) -> Style {
        let mut s = Style::default();
        if self.bold > 0 {
            s = s.add_modifier(Modifier::BOLD);
        }
        if self.italic > 0 {
            s = s.add_modifier(Modifier::ITALIC);
        }
        s
    }
}

/// The current block context that accumulates spans before they are flushed as
/// a `Line`. Most block types (paragraph, heading, list item, blockquote
/// paragraph) build up a single line of spans; code blocks bypass this and
/// emit lines directly.
#[derive(Default)]
struct RenderCtx {
    lines: Vec<Line<'static>>,
    /// Spans accumulating for the current line.
    current: Vec<Span<'static>>,
    /// Active inline styling: bold/italic as nesting depths (not booleans, so
    /// nested emphasis is handled correctly — see `InlineStyle`), plus the
    /// pending link/image URLs.
    inline: InlineStyle,
    /// Heading depth currently being rendered (`1`–`6`), or `None` outside a
    /// heading block. Set in `start_heading`, cleared in `End(TagEnd::Heading)`.
    /// Scopes heading-specific styling (accent colour, h1/h2 bold) explicitly
    /// so it never leaks into subsequent paragraphs or lists, and replaces the
    /// former approach of overloading `inline.bold` to carry heading weight.
    in_heading: Option<u8>,
    /// One entry per open list, innermost last — a single stack that encodes
    /// both nesting and per-level numbering. Each entry holds the NEXT counter
    /// to emit for that level: `Some(n)` = ordered (the next item renders `n.`
    /// then increments), `None` = unordered (renders `•`). Nesting depth is
    /// therefore `list_stack.len() - 1`; the stack being empty means we are
    /// outside any list.
    list_stack: Vec<Option<u64>>,
    /// Blockquote nesting depth (not a bool): CommonMark nests `BlockQuote`
    /// tags, so the inner `End` must decrement rather than clear, leaving the
    /// outer quote active for any trailing text (same bug class as bold/italic
    /// being depths). Muting applies whenever depth > 0. The leading `│ ` bar
    /// is emitted once per blockquote paragraph regardless of depth — it does
    /// not repeat per level, matching the simplest reading of the TS reference
    /// which wraps each paragraph in a single `│ <InlineNodes/>`. The trailing
    /// blank line is emitted only when depth returns to 0, so a nested quote
    /// does not double the blank separators.
    blockquote_depth: u32,
    /// Whether we are inside a code block and collecting its text.
    in_code_block: bool,
    /// Language tag for the current fenced code block (empty = no language).
    code_lang: String,
    /// Accumulates alt text for the current image. Kept separate from
    /// `current` (the shared line buffer) so that inline content that
    /// preceded the image on the same line is never swallowed into the
    /// placeholder. Set to `Some("")` on `Start(Image)`, appended to by
    /// text-event children, consumed and cleared by `end_image`.
    image_alt: Option<String>,
}

impl RenderCtx {
    /// Flush the accumulated span buffer as a new `Line` and reset it.
    fn flush_line(&mut self) {
        let spans = std::mem::take(&mut self.current);
        self.lines.push(Line::from(spans));
    }

    /// Push a span onto the current line buffer.
    fn push_span(&mut self, span: Span<'static>) {
        self.current.push(span);
    }

    /// Emit a blank separator line. Used between block elements to preserve
    /// visual breathing room matching the TS layout (each block is its own
    /// `<box>`).
    fn blank_line(&mut self) {
        self.lines.push(Line::default());
    }

    fn handle<'a>(&mut self, event: Event<'a>) {
        match event {
            // ── Block opens ──────────────────────────────────────────────────
            Event::Start(Tag::Heading { level, .. }) => {
                self.start_heading(level);
            }
            // Emit the blockquote bar once at the start of the paragraph so
            // all inline runs (text, code, links, images) that follow share
            // the same leading `│ ` — matching the TS reference that wraps
            // the entire paragraph in a single `│ <InlineNodes/>`.
            Event::Start(Tag::Paragraph) if self.blockquote_depth > 0 => {
                self.push_span(Span::styled("│ ", Style::default().fg(DARK.muted)));
            }
            Event::Start(Tag::Paragraph) => {}
            Event::Start(Tag::CodeBlock(kind)) => {
                self.start_code_block(kind);
            }
            Event::Start(Tag::List(start)) => {
                // `start` is the parser's first ordinal for an ordered list, or
                // `None` for an unordered one — exactly the per-level "next
                // counter" we want to push.
                self.list_stack.push(start);
            }
            Event::Start(Tag::Item) => {
                self.start_item();
            }
            Event::Start(Tag::BlockQuote(_)) => {
                self.blockquote_depth += 1;
            }
            // ── Block closes ─────────────────────────────────────────────────
            Event::End(TagEnd::Heading(_)) => {
                self.flush_line();
                self.blank_line();
                self.in_heading = None;
            }
            Event::End(TagEnd::Paragraph) => {
                self.flush_line();
                self.blank_line();
            }
            Event::End(TagEnd::CodeBlock) => {
                self.end_code_block();
            }
            Event::End(TagEnd::List(_)) => {
                self.list_stack.pop();
                // Blank line after the outermost list — unless the last item
                // was loose and its Paragraph end already emitted one.
                if self.list_stack.is_empty()
                    && self.lines.last().is_none_or(|l| !l.spans.is_empty())
                {
                    self.blank_line();
                }
            }
            // Only flush when spans are pending: in a loose list the item's
            // Paragraph child already flushed the line (and emitted the blank
            // separator), so an unconditional flush would push a spurious
            // extra blank line.
            Event::End(TagEnd::Item) if !self.current.is_empty() => {
                self.flush_line();
            }
            Event::End(TagEnd::Item) => {}
            Event::End(TagEnd::BlockQuote(_)) => {
                // Decrement so an inner quote's End leaves the outer quote
                // active (its trailing paragraph stays barred and muted).
                self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
                // Emit the trailing blank separator only when we leave the
                // outermost quote, so nested quotes don't double the blanks.
                if self.blockquote_depth == 0 {
                    self.blank_line();
                }
            }
            // ── Inline opens ─────────────────────────────────────────────────
            Event::Start(Tag::Strong) => {
                self.inline.bold += 1;
            }
            Event::Start(Tag::Emphasis) => {
                self.inline.italic += 1;
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                self.inline.link_url = Some(dest_url.into_string());
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                // Stash the image URL separately so we can fall back to it
                // when alt is empty, without clobbering any enclosing link's
                // URL that may already be in `link_url`.
                self.inline.image_url = Some(dest_url.into_string());
                // Open a dedicated alt buffer; text children will append here
                // instead of the shared line buffer. image_alt being Some is
                // the authoritative signal that we are inside an image tag.
                self.image_alt = Some(String::new());
            }
            // ── Inline closes ────────────────────────────────────────────────
            Event::End(TagEnd::Strong) => {
                self.inline.bold = self.inline.bold.saturating_sub(1);
            }
            Event::End(TagEnd::Emphasis) => {
                self.inline.italic = self.inline.italic.saturating_sub(1);
            }
            Event::End(TagEnd::Link) => {
                self.end_link();
            }
            Event::End(TagEnd::Image) => {
                self.end_image();
            }
            // ── Leaf events ──────────────────────────────────────────────────
            Event::Text(text) => {
                self.handle_text(text.into_string());
            }
            Event::Code(text) => {
                // Inside image alt text, Code events arrive between
                // Start(Image) and End(Image) just like Text events; capture
                // them into the alt buffer (mirroring handle_text) so the
                // code text renders inside the placeholder, not as a stray
                // span before it.
                if let Some(buf) = &mut self.image_alt {
                    buf.push_str(&text);
                    return;
                }
                // Inline code inside a heading keeps the heading style (accent
                // + bold for h1/h2) for consistency with the TS single-span
                // behavior. Inside a blockquote, apply muted foreground so the
                // inline code reads as part of the quoted text. Otherwise use
                // code colour with no bold/italic.
                let style = if let Some(depth) = self.in_heading {
                    heading_style(depth)
                } else if self.blockquote_depth > 0 {
                    Style::default().fg(DARK.muted)
                } else {
                    Style::default().fg(DARK.code)
                };
                self.push_span(Span::styled(text.into_string(), style));
            }
            Event::Rule => {
                self.lines.push(Line::from(Span::styled(
                    "────────────────────────────────────────",
                    Style::default().fg(DARK.muted),
                )));
                self.blank_line();
            }
            // Within a paragraph, soft breaks become spaces; hard breaks
            // flush the current line. Guards skip the no-op when the buffer
            // is already empty (avoids pushing a leading space at line start).
            Event::SoftBreak if !self.current.is_empty() => {
                self.push_span(Span::raw(" "));
            }
            Event::HardBreak if !self.current.is_empty() => {
                self.flush_line();
            }
            // Ignore everything else (HTML, task markers, footnotes, …).
            _ => {}
        }
    }

    // ── Heading ───────────────────────────────────────────────────────────────

    fn start_heading(&mut self, level: HeadingLevel) {
        // Prefix: "# ", "## ", "### " — same as TS `"#".repeat(depth) + " "`.
        let depth = match level {
            HeadingLevel::H1 => 1,
            HeadingLevel::H2 => 2,
            HeadingLevel::H3 => 3,
            HeadingLevel::H4 => 4,
            HeadingLevel::H5 => 5,
            HeadingLevel::H6 => 6,
        };
        let prefix = format!("{} ", "#".repeat(depth));
        // h1/h2: bold + accent; h3+: accent only (less prominent).
        self.push_span(Span::styled(prefix, heading_style(depth as u8)));
        // Record the heading depth so handle_text and Event::Code can apply
        // accent (+ bold for h1/h2) to all children of this heading. The TS
        // wraps the entire heading — prefix and children — in one accented bold
        // span; we replicate that by consulting in_heading from the text/code
        // handlers rather than overloading inline.bold.
        self.in_heading = Some(depth as u8);
    }

    // ── Code block ───────────────────────────────────────────────────────────

    fn start_code_block(&mut self, kind: CodeBlockKind<'_>) {
        self.in_code_block = true;
        self.code_lang = match kind {
            CodeBlockKind::Fenced(lang) => lang.into_string(),
            CodeBlockKind::Indented => String::new(),
        };
        // Opening fence: ```lang (muted), or just ``` when no language.
        let fence = if self.code_lang.is_empty() {
            "```".to_owned()
        } else {
            format!("```{}", self.code_lang)
        };
        self.lines.push(Line::from(Span::styled(
            fence,
            Style::default().fg(DARK.muted),
        )));
    }

    fn end_code_block(&mut self) {
        self.in_code_block = false;
        // Closing fence.
        self.lines.push(Line::from(Span::styled(
            "```",
            Style::default().fg(DARK.muted),
        )));
        self.blank_line();
    }

    // ── List item ─────────────────────────────────────────────────────────────

    fn start_item(&mut self) {
        // A nested list opens mid-item, before the parent item's line has
        // been flushed (its Item end comes after the whole sublist). Flush
        // the pending parent line so this item starts on its own line.
        if !self.current.is_empty() {
            self.flush_line();
        }
        let depth = self.list_stack.len().saturating_sub(1);
        let indent = "  ".repeat(depth);
        // The innermost open list's entry decides the bullet and, for ordered
        // lists, carries the next counter to emit and increment in place.
        let bullet = match self.list_stack.last_mut() {
            Some(Some(counter)) => {
                let n = *counter;
                *counter += 1;
                format!("{n}. ")
            }
            _ => "• ".to_owned(),
        };
        self.push_span(Span::raw(format!("{indent}{bullet}")));
    }

    // ── Link / Image ──────────────────────────────────────────────────────────

    fn end_link(&mut self) {
        // Append the URL in muted parens after the link text that has already
        // been pushed by Text events.
        if let Some(url) = self.inline.link_url.take() {
            let base = self.inline.to_ratatui();
            self.push_span(Span::styled(format!(" ({url})"), base.fg(DARK.muted)));
        }
    }

    fn end_image(&mut self) {
        // Images render as "[image: alt]" or "[image: url]" when alt is empty.
        // Alt text was accumulated into the dedicated `image_alt` buffer (not
        // the shared line buffer `current`), so preceding inline content on the
        // same line is never swallowed into the placeholder.
        let alt = self.image_alt.take().unwrap_or_default();
        let fallback = self.inline.image_url.take().unwrap_or_default();
        let label = if alt.is_empty() { fallback } else { alt };
        self.push_span(Span::styled(
            format!("[image: {label}]"),
            Style::default().fg(DARK.muted),
        ));
    }

    // ── Text ──────────────────────────────────────────────────────────────────

    fn handle_text(&mut self, text: String) {
        if self.in_code_block {
            // Code block body: one line per physical newline. The text event
            // contains the entire block including trailing newline.
            for line_text in text.lines() {
                self.lines.push(Line::from(Span::styled(
                    line_text.to_owned(),
                    Style::default().fg(DARK.code),
                )));
            }
            return;
        }
        if self.image_alt.is_some() {
            // Accumulate alt text into the dedicated buffer; consumed by
            // end_image. Writing to `image_alt` (not the shared `current`)
            // ensures preceding inline content on the same line is never
            // swallowed into the placeholder.
            if let Some(buf) = &mut self.image_alt {
                buf.push_str(&text);
            }
            return;
        }
        if self.blockquote_depth > 0 {
            // The `│ ` bar was already emitted once as a leading span when the
            // blockquote paragraph opened (see Start(Paragraph) handler).
            // Here we only apply the muted foreground; bold/italic from the
            // inline stack flow through to_ratatui() as usual.
            let style = self.inline.to_ratatui().fg(DARK.muted);
            self.push_span(Span::styled(text, style));
            return;
        }
        // Inside a heading: accent always; bold for h1/h2 (depth <= 2). The TS
        // wraps the entire heading (prefix + children) in one accented bold
        // span, so we match that here via the explicit in_heading depth rather
        // than heuristically sniffing the first span's colour.
        if let Some(depth) = self.in_heading {
            self.push_span(Span::styled(text, heading_style(depth)));
            return;
        }
        // Normal inline text outside any heading.
        let style = self.inline.to_ratatui();
        self.push_span(Span::styled(text, style));
    }
}

#[cfg(test)]
mod tests;
