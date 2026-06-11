//! Display-width word wrap for styled ratatui lines.
//!
//! The detail layout derives every display row before rendering — the scroll
//! clamp, the card line ranges, and the painted frame all share one line
//! list — so wrapping must happen at derivation time too: `Paragraph::wrap`
//! at render time would re-flow text into rows the measurements never saw.
//! Greedy word wrap: fragments keep their span's style, the whitespace run a
//! row breaks on is dropped, and a word wider than the full width hard-splits
//! at character boundaries.

use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Wrap every line to `width` display columns. `width == 0` (an unmeasured
/// terminal) passes lines through untouched.
pub fn wrap_lines(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return lines;
    }
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        wrap_line(line, width, &mut out);
    }
    out
}

/// One whitespace or non-whitespace run from a span, carrying its style.
struct Token {
    text: String,
    style: ratatui::style::Style,
    width: usize,
    is_space: bool,
}

fn wrap_line(line: Line<'static>, width: usize, out: &mut Vec<Line<'static>>) {
    if line.width() <= width {
        out.push(line);
        return;
    }

    let tokens: Vec<Token> = line
        .spans
        .iter()
        .flat_map(|span| {
            split_runs(&span.content).map(|(is_space, run)| Token {
                text: run.to_owned(),
                style: span.style,
                width: run.width(),
                is_space,
            })
        })
        .collect();

    let mut row: Vec<Span<'static>> = Vec::new();
    let mut row_width = 0usize;
    for token in tokens {
        if row_width + token.width <= width {
            row.push(Span::styled(token.text, token.style));
            row_width += token.width;
            continue;
        }
        if token.is_space {
            // The row breaks on this whitespace run: drop it, the
            // continuation row starts at column 0.
            flush_row(&mut row, out);
            row_width = 0;
            continue;
        }
        if row_width > 0 {
            flush_row(&mut row, out);
            row_width = 0;
        }
        if token.width <= width {
            row.push(Span::styled(token.text, token.style));
            row_width = token.width;
            continue;
        }
        // A single word wider than the whole row: hard-split it at character
        // boundaries, flushing full rows as they fill.
        let mut fragment = String::new();
        for ch in token.text.chars() {
            let ch_width = ch.width().unwrap_or(0);
            if row_width + ch_width > width {
                row.push(Span::styled(std::mem::take(&mut fragment), token.style));
                flush_row(&mut row, out);
                row_width = 0;
            }
            fragment.push(ch);
            row_width += ch_width;
        }
        if !fragment.is_empty() {
            row.push(Span::styled(fragment, token.style));
        }
    }
    if !row.is_empty() {
        flush_row(&mut row, out);
    }
}

/// Flush `row` as one output line, dropping the whitespace the break landed
/// after — invisible at a row end, and it would distort the row's width.
fn flush_row(row: &mut Vec<Span<'static>>, out: &mut Vec<Line<'static>>) {
    while row.last().is_some_and(|s| s.content.trim().is_empty()) {
        row.pop();
    }
    out.push(Line::from(std::mem::take(row)));
}

/// Split text into alternating whitespace / non-whitespace runs.
fn split_runs(text: &str) -> impl Iterator<Item = (bool, &str)> {
    let mut rest = text;
    std::iter::from_fn(move || {
        let first = rest.chars().next()?;
        let is_space = first.is_whitespace();
        let end = rest
            .find(|c: char| c.is_whitespace() != is_space)
            .unwrap_or(rest.len());
        let (run, tail) = rest.split_at(end);
        rest = tail;
        Some((is_space, run))
    })
}

#[cfg(test)]
mod tests;
