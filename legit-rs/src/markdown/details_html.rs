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
        let open = lower[i..].find("<details").map(|p| i + p);
        let close = lower[i..].find("</details").map(|p| i + p);
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
    let open = lower[from..].find("<details").map(|p| from + p);
    let close = lower[from..].find("</details").map(|p| from + p);
    open.into_iter().chain(close).min().unwrap_or(lower.len())
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
