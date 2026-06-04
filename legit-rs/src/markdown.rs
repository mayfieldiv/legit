//! Pure markdown renderer. Port of the TS `src/lib/markdown.tsx`: maps
//! `pulldown-cmark` events to ratatui `Line`/`Span` with the same feature set
//! as the TS implementation.
//!
//! The public entry point is `render(source: &str) -> Vec<Line<'static>>`.
//! Each call to `render` allocates owned strings (via `into_static` on
//! `CowStr`), so the caller has no lifetime dependency on the source string.
//!
//! Supported block types: headings (h1–h3+ with decreasing visual weight),
//! paragraphs, fenced code blocks (language tag + visual delineation),
//! ordered and unordered lists (nested), blockquotes, and thematic breaks.
//! Supported inline styles: bold, italic, inline code, links (text + URL),
//! images ([image: alt/url] fallback).

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

// ── Colour constants ──────────────────────────────────────────────────────────

/// Accent colour (TS `theme.accent` #61AFEF): headings, section labels.
const ACCENT: Color = Color::Cyan;
/// Muted colour (TS `theme.muted` #5C6370): code fences, blockquote bar,
/// thematic breaks, image placeholders.
const MUTED: Color = Color::DarkGray;
/// Code colour (TS `theme.code` #7EC8D3): code block body, inline code.
const CODE: Color = Color::LightCyan;

// ── Public API ────────────────────────────────────────────────────────────────

/// Render a markdown string to a list of ratatui lines ready to be passed to
/// a `Paragraph` or collected into a `Text`. Each `Line` owns its spans
/// (all strings are `'static`), so the returned `Vec` is independent of the
/// source lifetime.
pub fn render(source: &str) -> Vec<Line<'static>> {
    let parser = Parser::new_ext(source, Options::empty());
    let mut ctx = RenderCtx::default();
    ctx.process(parser);
    ctx.lines
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Return the ratatui style for a heading at the given depth: accent always;
/// bold for h1/h2 (depth <= 2), plain accent for h3+.
fn heading_style(depth: u8) -> Style {
    let s = Style::default().fg(ACCENT);
    if depth <= 2 {
        s.add_modifier(Modifier::BOLD)
    } else {
        s
    }
}

// ── Internal state machine ────────────────────────────────────────────────────

/// Tracks which inline modifiers are currently active. Each modifier is a
/// simple boolean; they stack correctly because we only need to know whether
/// *any* ancestor activates bold or italic, not the exact depth.
#[derive(Default, Clone)]
struct InlineStyle {
    bold: bool,
    italic: bool,
    /// Inside an image tag — text children become the alt text. Alt text is
    /// accumulated into `RenderCtx::image_alt` (not the shared line buffer)
    /// so that preceding inline content on the same line is not swallowed.
    image: bool,
    /// Inside a link tag — accumulate text, emit as "text (url)" on End.
    /// Also holds the image URL stashed at Start(Image) as a fallback when
    /// the alt text is empty.
    link_url: Option<String>,
}

impl InlineStyle {
    fn to_ratatui(&self) -> Style {
        let mut s = Style::default();
        if self.bold {
            s = s.add_modifier(Modifier::BOLD);
        }
        if self.italic {
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
    /// Active inline style stack (simple boolean flags, not a true stack).
    inline: InlineStyle,
    /// Heading depth currently being rendered (`1`–`6`), or `None` outside a
    /// heading block. Set in `start_heading`, cleared in `End(TagEnd::Heading)`.
    /// Scopes heading-specific styling (accent colour, h1/h2 bold) explicitly
    /// so it never leaks into subsequent paragraphs or lists, and replaces the
    /// former approach of overloading `inline.bold` to carry heading weight.
    in_heading: Option<u8>,
    /// Nesting depth for lists; 0 = top-level.
    list_depth: u32,
    /// Bullet/counter for the current list nesting level. `None` = unordered,
    /// `Some(n)` = ordered starting at n.
    list_stack: Vec<Option<u64>>,
    /// Counter for the current ordered list item (1-based, incremented on
    /// each `Start(Item)`).
    item_counters: Vec<u64>,
    /// Whether we are inside a blockquote. When true, `Start(Paragraph)`
    /// emits the `│ ` bar once as a leading muted span; subsequent inline
    /// runs (text, code, links) flow through normally with muted foreground.
    in_blockquote: bool,
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

    /// Drive all parser events through the state machine.
    fn process<'a>(&mut self, parser: impl Iterator<Item = Event<'a>>) {
        for event in parser {
            self.handle(event);
        }
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
            Event::Start(Tag::Paragraph) if self.in_blockquote => {
                self.push_span(Span::styled("│ ", Style::default().fg(MUTED)));
            }
            Event::Start(Tag::Paragraph) => {}
            Event::Start(Tag::CodeBlock(kind)) => {
                self.start_code_block(kind);
            }
            Event::Start(Tag::List(start)) => {
                self.list_stack.push(start);
                self.item_counters.push(start.unwrap_or(0));
                self.list_depth += 1;
            }
            Event::Start(Tag::Item) => {
                self.start_item();
            }
            Event::Start(Tag::BlockQuote(_)) => {
                self.in_blockquote = true;
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
                self.item_counters.pop();
                if self.list_depth > 0 {
                    self.list_depth -= 1;
                }
                // Blank line after the outermost list.
                if self.list_depth == 0 {
                    self.blank_line();
                }
            }
            Event::End(TagEnd::Item) => {
                self.flush_line();
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                self.in_blockquote = false;
                self.blank_line();
            }
            // ── Inline opens ─────────────────────────────────────────────────
            Event::Start(Tag::Strong) => {
                self.inline.bold = true;
            }
            Event::Start(Tag::Emphasis) => {
                self.inline.italic = true;
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                self.inline.link_url = Some(dest_url.into_string());
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                self.inline.image = true;
                // Stash the URL so we can fall back to it when alt is empty.
                self.inline.link_url = Some(dest_url.into_string());
                // Open a dedicated alt buffer; text children will append here
                // instead of the shared line buffer.
                self.image_alt = Some(String::new());
            }
            // ── Inline closes ────────────────────────────────────────────────
            Event::End(TagEnd::Strong) => {
                self.inline.bold = false;
            }
            Event::End(TagEnd::Emphasis) => {
                self.inline.italic = false;
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
                // Inline code inside a heading keeps the heading style (accent
                // + bold for h1/h2) for consistency with the TS single-span
                // behavior. Inside a blockquote, apply muted foreground so the
                // inline code reads as part of the quoted text. Otherwise use
                // code colour with no bold/italic.
                let style = if let Some(depth) = self.in_heading {
                    heading_style(depth)
                } else if self.in_blockquote {
                    Style::default().fg(MUTED)
                } else {
                    Style::default().fg(CODE)
                };
                self.push_span(Span::styled(text.into_string(), style));
            }
            Event::Rule => {
                self.lines.push(Line::from(Span::styled(
                    "────────────────────────────────────────",
                    Style::default().fg(MUTED),
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
        self.lines
            .push(Line::from(Span::styled(fence, Style::default().fg(MUTED))));
    }

    fn end_code_block(&mut self) {
        self.in_code_block = false;
        // Closing fence.
        self.lines
            .push(Line::from(Span::styled("```", Style::default().fg(MUTED))));
        self.blank_line();
    }

    // ── List item ─────────────────────────────────────────────────────────────

    fn start_item(&mut self) {
        let depth = self.list_depth.saturating_sub(1) as usize;
        let indent = "  ".repeat(depth);
        let is_ordered = self.list_stack.last().map(|s| s.is_some()).unwrap_or(false);
        let bullet = if is_ordered {
            // Increment the counter for this depth.
            let counter = self.item_counters.last_mut().unwrap();
            let n = *counter;
            *counter += 1;
            format!("{n}. ")
        } else {
            "• ".to_owned()
        };
        self.push_span(Span::raw(format!("{indent}{bullet}")));
    }

    // ── Link / Image ──────────────────────────────────────────────────────────

    fn end_link(&mut self) {
        // Append the URL in muted parens after the link text that has already
        // been pushed by Text events.
        if let Some(url) = self.inline.link_url.take() {
            let base = self.inline.to_ratatui();
            self.push_span(Span::styled(format!(" ({url})"), base.fg(MUTED)));
        }
    }

    fn end_image(&mut self) {
        // Images render as "[image: alt]" or "[image: url]" when alt is empty.
        // Alt text was accumulated into the dedicated `image_alt` buffer (not
        // the shared line buffer `current`), so preceding inline content on the
        // same line is never swallowed into the placeholder.
        let alt = self.image_alt.take().unwrap_or_default();
        let fallback = self.inline.link_url.take().unwrap_or_default();
        let label = if alt.is_empty() { fallback } else { alt };
        self.push_span(Span::styled(
            format!("[image: {label}]"),
            Style::default().fg(MUTED),
        ));
        self.inline.image = false;
    }

    // ── Text ──────────────────────────────────────────────────────────────────

    fn handle_text(&mut self, text: String) {
        if self.in_code_block {
            // Code block body: one line per physical newline. The text event
            // contains the entire block including trailing newline.
            for line_text in text.lines() {
                self.lines.push(Line::from(Span::styled(
                    line_text.to_owned(),
                    Style::default().fg(CODE),
                )));
            }
            return;
        }
        if self.inline.image {
            // Accumulate alt text into the dedicated buffer; consumed by
            // end_image. Writing to `image_alt` (not the shared `current`)
            // ensures preceding inline content on the same line is never
            // swallowed into the placeholder.
            if let Some(buf) = &mut self.image_alt {
                buf.push_str(&text);
            }
            return;
        }
        if self.in_blockquote {
            // The `│ ` bar was already emitted once as a leading span when the
            // blockquote paragraph opened (see Start(Paragraph) handler).
            // Here we only apply the muted foreground; bold/italic from the
            // inline stack flow through to_ratatui() as usual.
            let style = self.inline.to_ratatui().fg(MUTED);
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use ratatui::style::Modifier;
    use ratatui::text::{Line, Span};

    use super::{ACCENT, CODE, MUTED, render};

    /// Collect all span contents from all lines as a flat string (for
    /// substring-searching assertions that don't care about span boundaries).
    fn all_text(lines: &[Line<'_>]) -> String {
        lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect::<Vec<_>>()
            .join("")
    }

    /// Find the first span whose content contains `needle`, searching across
    /// all lines.
    fn find_span<'a>(lines: &'a [Line<'_>], needle: &str) -> Option<&'a Span<'a>> {
        lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content.contains(needle))
    }

    // ── Headings ─────────────────────────────────────────────────────────────

    #[test]
    fn h1_heading_is_bold_and_accented() {
        let lines = render("# Hello");
        // The heading line must contain the "#" prefix and "Hello".
        let text = all_text(&lines);
        assert!(text.contains("# "), "missing h1 prefix: {text:?}");
        assert!(text.contains("Hello"), "missing heading text: {text:?}");

        // The prefix span must be bold + accent.
        let prefix = find_span(&lines, "# ").expect("prefix span not found");
        assert_eq!(
            prefix.style.fg,
            Some(ACCENT),
            "h1 prefix should be accent colour"
        );
        assert!(
            prefix.style.add_modifier.contains(Modifier::BOLD),
            "h1 prefix should be bold"
        );
    }

    #[test]
    fn h2_heading_is_bold_and_accented() {
        let lines = render("## Section");
        let text = all_text(&lines);
        assert!(text.contains("## "), "missing h2 prefix: {text:?}");

        let prefix = find_span(&lines, "## ").expect("prefix span not found");
        assert_eq!(prefix.style.fg, Some(ACCENT));
        assert!(prefix.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn h3_heading_is_accented_not_bold() {
        let lines = render("### Sub");
        let text = all_text(&lines);
        assert!(text.contains("### "), "missing h3 prefix: {text:?}");

        let prefix = find_span(&lines, "### ").expect("prefix span not found");
        assert_eq!(prefix.style.fg, Some(ACCENT));
        // h3+ should NOT be bold — less visual weight than h1/h2.
        assert!(
            !prefix.style.add_modifier.contains(Modifier::BOLD),
            "h3 prefix should not be bold"
        );
    }

    #[test]
    fn heading_bold_does_not_leak_into_following_paragraph() {
        // h1 followed by a paragraph: the paragraph text must not be bold.
        let lines = render("# Title\n\nsome paragraph");
        let para_span = find_span(&lines, "some paragraph").expect("para span not found");
        assert!(
            !para_span.style.add_modifier.contains(Modifier::BOLD),
            "paragraph after h1 must not be bold (heading bold leaked): {para_span:?}"
        );
        assert_eq!(
            para_span.style.fg, None,
            "paragraph after h1 must not be accent-coloured: {para_span:?}"
        );
    }

    #[test]
    fn heading_text_is_accented() {
        // The text child of an h1 heading must carry accent colour and bold.
        let lines = render("# Hello world");
        let text_span = find_span(&lines, "Hello world").expect("heading text span not found");
        assert_eq!(
            text_span.style.fg,
            Some(ACCENT),
            "h1 text must be accent: {text_span:?}"
        );
        assert!(
            text_span.style.add_modifier.contains(Modifier::BOLD),
            "h1 text must be bold: {text_span:?}"
        );
    }

    #[test]
    fn h3_text_is_accented_not_bold() {
        // h3 text child: accent, no bold.
        let lines = render("### Sub section");
        let text_span = find_span(&lines, "Sub section").expect("heading text span not found");
        assert_eq!(
            text_span.style.fg,
            Some(ACCENT),
            "h3 text must be accent: {text_span:?}"
        );
        assert!(
            !text_span.style.add_modifier.contains(Modifier::BOLD),
            "h3 text must not be bold: {text_span:?}"
        );
    }

    // ── Paragraph with inline styles ─────────────────────────────────────────

    #[test]
    fn paragraph_bold_span_has_bold_modifier() {
        let lines = render("This is **bold** text.");
        let text = all_text(&lines);
        assert!(text.contains("bold"), "missing bold text: {text:?}");

        let bold_span = find_span(&lines, "bold").expect("bold span not found");
        assert!(
            bold_span.style.add_modifier.contains(Modifier::BOLD),
            "bold text must have BOLD modifier"
        );
    }

    #[test]
    fn paragraph_italic_span_has_italic_modifier() {
        let lines = render("This is *italic* text.");
        let text = all_text(&lines);
        assert!(text.contains("italic"), "missing italic text: {text:?}");

        let italic_span = find_span(&lines, "italic").expect("italic span not found");
        assert!(
            italic_span.style.add_modifier.contains(Modifier::ITALIC),
            "italic text must have ITALIC modifier"
        );
    }

    #[test]
    fn paragraph_inline_code_has_code_colour() {
        let lines = render("Use `foo()` here.");
        let text = all_text(&lines);
        assert!(text.contains("foo()"), "missing code text: {text:?}");

        let code_span = find_span(&lines, "foo()").expect("code span not found");
        assert_eq!(
            code_span.style.fg,
            Some(CODE),
            "inline code must have code colour"
        );
    }

    // ── Code block ───────────────────────────────────────────────────────────

    #[test]
    fn fenced_code_block_shows_language_tag_in_fence() {
        let lines = render("```rust\nfn main() {}\n```");
        let text = all_text(&lines);
        // Opening fence must include the language.
        assert!(text.contains("```rust"), "missing opening fence: {text:?}");
        // Code body must appear.
        assert!(text.contains("fn main()"), "missing code body: {text:?}");
        // Closing fence must appear.
        assert!(
            text.matches("```").count() >= 2,
            "missing closing fence: {text:?}"
        );

        // Opening fence is muted.
        let fence = find_span(&lines, "```rust").expect("opening fence span not found");
        assert_eq!(fence.style.fg, Some(MUTED), "fence must be muted");

        // Code body is code colour.
        let body = find_span(&lines, "fn main()").expect("body span not found");
        assert_eq!(body.style.fg, Some(CODE), "code body must be code colour");
    }

    #[test]
    fn fenced_code_block_without_language_shows_plain_fence() {
        let lines = render("```\nhello\n```");
        let text = all_text(&lines);
        assert!(text.contains("```"), "missing fence: {text:?}");
        assert!(text.contains("hello"), "missing code body: {text:?}");

        let fence = find_span(&lines, "```").expect("fence span");
        assert_eq!(fence.style.fg, Some(MUTED));
    }

    // ── Lists ─────────────────────────────────────────────────────────────────

    #[test]
    fn unordered_list_uses_bullet_character() {
        let lines = render("- alpha\n- beta");
        let text = all_text(&lines);
        assert!(text.contains('•'), "missing bullet: {text:?}");
        assert!(text.contains("alpha"), "missing item text: {text:?}");
        assert!(text.contains("beta"));
    }

    #[test]
    fn ordered_list_uses_numeric_prefix() {
        let lines = render("1. first\n2. second");
        let text = all_text(&lines);
        assert!(text.contains("1. "), "missing first counter: {text:?}");
        assert!(text.contains("2. "), "missing second counter: {text:?}");
        assert!(text.contains("first"));
        assert!(text.contains("second"));
    }

    #[test]
    fn nested_list_indents_child_items() {
        let source = "- outer\n  - inner";
        let lines = render(source);
        let text = all_text(&lines);
        assert!(text.contains("outer"), "missing outer: {text:?}");
        assert!(text.contains("inner"), "missing inner: {text:?}");
        // Nested item must have extra indent ("  •").
        assert!(
            text.contains("  •"),
            "nested item must be indented: {text:?}"
        );
    }

    // ── Blockquote ───────────────────────────────────────────────────────────

    #[test]
    fn blockquote_prefixes_text_with_bar() {
        let lines = render("> quoted text");
        let text = all_text(&lines);
        // Bar and text appear in order (may be separate spans).
        assert!(text.contains("│ "), "missing blockquote bar: {text:?}");
        assert!(
            text.contains("quoted text"),
            "missing blockquote body: {text:?}"
        );

        // The bar span must be muted.
        let bar = find_span(&lines, "│ ").expect("blockquote bar span");
        assert_eq!(bar.style.fg, Some(MUTED));
        // The text span must be muted.
        let body = find_span(&lines, "quoted text").expect("blockquote text span");
        assert_eq!(body.style.fg, Some(MUTED));
    }

    #[test]
    fn blockquote_inline_formatting_no_repeated_bar() {
        // A blockquote with bold, italic, and inline-code inside: the bar must
        // appear exactly once at the line start; inline styles render normally.
        let lines = render("> some **bold** and `code` text");
        let text = all_text(&lines);

        // Bar appears exactly once.
        assert_eq!(
            text.matches("│").count(),
            1,
            "bar must appear exactly once: {text:?}"
        );

        // Inline content is present.
        assert!(text.contains("some"), "missing plain text: {text:?}");
        assert!(text.contains("bold"), "missing bold text: {text:?}");
        assert!(text.contains("code"), "missing code text: {text:?}");

        // Bold span retains bold modifier (with muted foreground from blockquote).
        let bold_span = find_span(&lines, "bold").expect("bold span");
        assert!(
            bold_span.style.add_modifier.contains(Modifier::BOLD),
            "bold text must have BOLD modifier: {bold_span:?}"
        );

        // Inline code is muted (not CODE colour) inside a blockquote.
        let code_span = find_span(&lines, "code").expect("code span");
        assert_eq!(
            code_span.style.fg,
            Some(MUTED),
            "inline code in blockquote must be muted: {code_span:?}"
        );
    }

    #[test]
    fn image_preceded_by_text_does_not_swallow_preceding_content() {
        // Preceding "Hello " must remain in `current`; only "alt" goes into
        // the image placeholder.
        let lines = render("Hello ![alt](https://example.com/img.png) world");
        let text = all_text(&lines);
        assert!(
            text.contains("Hello"),
            "preceding text was swallowed: {text:?}"
        );
        assert!(
            text.contains("[image: alt]"),
            "image placeholder missing or wrong: {text:?}"
        );
        assert!(text.contains("world"), "trailing text missing: {text:?}");
        // "Hello" must NOT appear inside the image placeholder.
        assert!(
            !text.contains("[image: Hello"),
            "preceding text was folded into image: {text:?}"
        );
    }

    #[test]
    fn link_then_image_on_same_line() {
        // Link text and URL must not be folded into the image placeholder.
        let lines = render("[docs](http://d) and ![img](http://i)");
        let text = all_text(&lines);
        assert!(text.contains("docs"), "link text missing: {text:?}");
        assert!(text.contains("http://d"), "link url missing: {text:?}");
        assert!(
            text.contains("[image: img]"),
            "image placeholder missing: {text:?}"
        );
        // The image placeholder must not contain the link text or URL.
        assert!(
            !text.contains("[image: docs"),
            "link text folded into image: {text:?}"
        );
    }

    // ── Thematic break ───────────────────────────────────────────────────────

    #[test]
    fn thematic_break_emits_muted_rule_line() {
        let lines = render("before\n\n---\n\nafter");
        let text = all_text(&lines);
        assert!(
            text.contains('─'),
            "thematic break must contain box-drawing chars: {text:?}"
        );
        // The rule span must be muted.
        let rule = find_span(&lines, "────").expect("rule span not found");
        assert_eq!(rule.style.fg, Some(MUTED));
    }

    // ── Link ─────────────────────────────────────────────────────────────────

    #[test]
    fn link_shows_text_and_url_in_parens() {
        let lines = render("[legit](https://github.com/mayfieldiv/legit)");
        let text = all_text(&lines);
        assert!(text.contains("legit"), "missing link text: {text:?}");
        assert!(
            text.contains("https://github.com/mayfieldiv/legit"),
            "missing link url: {text:?}"
        );
        // URL must appear in muted parens.
        let url_span =
            find_span(&lines, "https://github.com/mayfieldiv/legit").expect("url span not found");
        assert_eq!(url_span.style.fg, Some(MUTED));
    }

    // ── Image ─────────────────────────────────────────────────────────────────

    #[test]
    fn image_renders_as_placeholder_with_alt_text() {
        let lines = render("![my diagram](https://example.com/img.png)");
        let text = all_text(&lines);
        assert!(
            text.contains("[image: my diagram]"),
            "missing image placeholder: {text:?}"
        );
        let span = find_span(&lines, "[image:").expect("image span");
        assert_eq!(span.style.fg, Some(MUTED));
    }

    #[test]
    fn image_falls_back_to_url_when_alt_is_empty() {
        let lines = render("![](https://example.com/img.png)");
        let text = all_text(&lines);
        assert!(
            text.contains("[image: https://example.com/img.png]"),
            "image without alt should fall back to url: {text:?}"
        );
    }
}
