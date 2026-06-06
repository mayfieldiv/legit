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

/// Render each line to its concatenated text (one string per `Line`), for
/// assertions on line structure: what lands on which line, where the
/// blank separators are.
fn line_texts(lines: &[Line<'_>]) -> Vec<String> {
    lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect()
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
fn nested_strong_keeps_outer_bold_after_inner_end() {
    // CommonMark nests Strong tags for "**a **b** c**": the inner End
    // must not clear the outer bold, so " c" still renders bold.
    let lines = render("**a **b** c**");
    let tail = find_span(&lines, " c").expect("trailing text span not found");
    assert!(
        tail.style.add_modifier.contains(Modifier::BOLD),
        "text after a nested strong must keep the outer bold: {tail:?}"
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
fn nested_emphasis_keeps_outer_italic_after_inner_end() {
    // Same nesting rule as Strong: "*a *b* c*" nests Emphasis tags, and
    // the inner End must not clear the outer italic for " c".
    let lines = render("*a *b* c*");
    let tail = find_span(&lines, " c").expect("trailing text span not found");
    assert!(
        tail.style.add_modifier.contains(Modifier::ITALIC),
        "text after a nested emphasis must keep the outer italic: {tail:?}"
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
fn loose_list_separates_items_with_single_blank_line() {
    // A loose list (blank line between items) wraps each item's content
    // in a Paragraph, which already flushes the line and emits the blank
    // separator. The Item end must not add a spurious extra blank.
    let lines = render("- foo\n\n- bar");
    assert_eq!(
        line_texts(&lines),
        vec!["• foo", "", "• bar", ""],
        "loose list items must be separated by exactly one blank line"
    );
}

#[test]
fn nested_list_indents_child_items() {
    // Asserts full line structure, not just substrings: the parent item
    // and the indented child must land on separate lines (a nested list
    // opens mid-item, before the parent's line has been flushed).
    let lines = render("- outer\n  - inner");
    assert_eq!(
        line_texts(&lines),
        vec!["• outer", "  • inner", ""],
        "nested item must be on its own, indented line"
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
fn text_after_nested_blockquote_stays_barred_and_muted() {
    // "> outer" / "> > inner" / "> trailing" nests BlockQuote tags. The
    // inner End(BlockQuote) must only decrement the depth, leaving the
    // outer quote active so the trailing paragraph keeps its `│ ` bar and
    // muted text. With the old bool, the inner End cleared the flag and the
    // trailing paragraph lost its bar/muting; each End also emitted a blank,
    // so nesting doubled the internal separators.
    //
    // The trailing single blank after the whole quote is the same one a
    // flat quote produces (one from the paragraph End, one from the
    // outermost blockquote End) — nesting must not add to it. We assert the
    // nested output equals a flat three-paragraph quote so any future change
    // to the trailing-blank convention stays consistent across both.
    let nested = render("> outer\n>\n> > inner\n>\n> trailing");
    assert_eq!(
        line_texts(&nested),
        vec!["│ outer", "", "│ inner", "", "│ trailing", "", ""],
        "nested quote must bar every line with a single blank between paragraphs"
    );
    let flat = render("> outer\n>\n> inner\n>\n> trailing");
    assert_eq!(
        line_texts(&nested),
        line_texts(&flat),
        "nesting a quote must not change the blank-line structure"
    );

    // The trailing paragraph's text must remain muted (outer quote active).
    let trailing = find_span(&nested, "trailing").expect("trailing span");
    assert_eq!(
        trailing.style.fg,
        Some(MUTED),
        "text after a nested quote must stay muted: {trailing:?}"
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

#[test]
fn image_alt_with_inline_code_stays_inside_placeholder() {
    // Inline code inside image alt text emits a Code event between
    // Start(Image) and End(Image); it must be captured into the alt
    // buffer, not pushed to the line as a stray span outside the
    // placeholder.
    let lines = render("![click `code`](http://x.test/i.png)");
    assert_eq!(
        line_texts(&lines),
        vec!["[image: click code]", ""],
        "code in alt must render inside the placeholder, not before it"
    );
}

#[test]
fn image_nested_in_link_preserves_outer_link_url() {
    // pulldown-cmark emits: Start(Link href) -> Start(Image img) ->
    // Text(alt) -> End(Image) -> End(Link).
    // Before the fix, Start(Image) overwrote link_url with the image URL,
    // so end_link found None and emitted no "(http://href)".
    let lines = render("[![alt](http://img)](http://href)");
    let text = all_text(&lines);
    // The outer link's URL must appear in the output.
    assert!(
        text.contains("http://href"),
        "outer link url was dropped: {text:?}"
    );
    // The image placeholder must appear.
    assert!(
        text.contains("[image: alt]"),
        "image placeholder missing: {text:?}"
    );
}
