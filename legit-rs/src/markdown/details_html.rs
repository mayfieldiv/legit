//! `<details>`/`<summary>` discovery in raw HTML blocks.
//!
//! pulldown-cmark passes block HTML through as opaque text, so the markdown
//! renderer has to find `<details>` opens, `</details>` closes, and the summary
//! text itself. This module is that string-scanning, and nothing more: it knows
//! about tags, not about the frame stack that consumes the tokens (see
//! `markdown::apply_html_tokens`). The project has no HTML-parser dependency —
//! pulling in one to locate three tag names would dwarf the work — so the scan
//! is hand-rolled, kept here behind a small surface and exercised in isolation.

/// A `<details>` boundary found in an accumulated HTML block.
#[derive(Debug, PartialEq)]
pub(super) enum DetailsToken {
    /// `<details ...>` plus its `<summary>` text, if present.
    Open(Option<String>),
    /// `</details>`.
    Close,
}

/// Scan one accumulated HTML block for `<details>` opens and `</details>`
/// closes in document order. For each open, the immediately-following
/// `<summary>...</summary>` text (up to the next details boundary) becomes the
/// group's summary. Case-insensitive on tag names; all other markup is ignored.
pub(super) fn tokenize_details(html: &str) -> Vec<DetailsToken> {
    let lower = html.to_ascii_lowercase();
    let mut tokens = Vec::new();
    let mut i = 0usize;
    while i < html.len() {
        let open = find_tag(&lower, i, "<details");
        let close = find_tag(&lower, i, "</details");
        let (is_open, at) = match (open, close) {
            (Some(o), Some(c)) => (o <= c, o.min(c)),
            (Some(o), None) => (true, o),
            (None, Some(c)) => (false, c),
            (None, None) => break,
        };
        // Advance past this tag's '>' (or to the end if it is unterminated).
        let tag_end = lower[at..].find('>').map_or(html.len(), |p| at + p + 1);
        if is_open {
            // The summary lives between this tag and the next details boundary.
            let next = next_details_boundary(&lower, tag_end);
            let summary = extract_summary(&html[tag_end..next], &lower[tag_end..next]);
            tokens.push(DetailsToken::Open(summary));
        } else {
            tokens.push(DetailsToken::Close);
        }
        i = tag_end;
    }
    tokens
}

/// The byte offset of the next `<details`/`</details` at or after `from`, or
/// the string length when there is none.
fn next_details_boundary(lower: &str, from: usize) -> usize {
    let open = find_tag(lower, from, "<details");
    let close = find_tag(lower, from, "</details");
    open.into_iter().chain(close).min().unwrap_or(lower.len())
}

/// The byte offset of the next `needle` (a tag prefix such as `"<details"` or
/// `"</details"`) at or after `from` in `lower` whose following character is a
/// tag-name boundary (`>`, `/`, whitespace, or end of input). This rejects
/// look-alike element names like `<detailsFoo>` or `</details-bar>`, matching
/// the TS reference's exact tag-name matching. `lower` must already be
/// ASCII-lowercased so the lowercase `needle` compares case-insensitively;
/// `needle` is ASCII, so `at + needle.len()` lands on a char boundary.
fn find_tag(lower: &str, from: usize, needle: &str) -> Option<usize> {
    let mut search = from;
    while let Some(rel) = lower[search..].find(needle) {
        let at = search + rel;
        let after = at + needle.len();
        match lower[after..].chars().next() {
            None => return Some(at),
            Some(c) if c == '>' || c == '/' || c.is_whitespace() => return Some(at),
            // A look-alike prefix (e.g. `<detailsfoo`); keep scanning past it.
            _ => search = after,
        }
    }
    None
}

/// Extract and clean the `<summary>` text from an HTML segment, or `None` when
/// there is no summary (the caller defaults to "Details"). `segment` is the raw
/// slice; `lower` is its lowercased twin for case-insensitive tag matching.
fn extract_summary(segment: &str, lower: &str) -> Option<String> {
    let open = lower.find("<summary")?;
    let content = lower[open..].find('>').map(|p| open + p + 1)?;
    let end = lower[content..]
        .find("</summary>")
        .map_or(segment.len(), |p| content + p);
    let text = clean_summary_text(&segment[content..end]);
    (!text.is_empty()).then_some(text)
}

/// Reduce a `<summary>`'s inner HTML to plain text: strip tags, decode the
/// handful of entities GitHub emits, and collapse whitespace (mirrors the TS
/// `collectHastText`, which keeps text nodes and drops element formatting).
fn clean_summary_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_tag = false;
    for ch in raw.chars() {
        match ch {
            '<' => in_tag = true,
            // Only a '>' inside a tag closes it; a literal '>' in text content
            // (e.g. a summary reading "a > b") is preserved, not consumed.
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    // Decode `&amp;` last so an already-escaped entity isn't double-decoded.
    let decoded = out
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}
