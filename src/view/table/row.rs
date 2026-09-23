//! The table's cell renderer: a row is a list of cells, each fitted to its
//! column and joined by one-column gaps, with an optional Selected Row fill
//! under the whole line.

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

use crate::format::truncate;

/// Columns between adjacent cells.
pub const GAP: usize = 1;

/// One cell: its spans, fitted to `width` at render time — truncated with an
/// ellipsis when they overflow, space-padded when they don't.
pub struct Cell {
    pub spans: Vec<Span<'static>>,
    pub width: usize,
}

impl Cell {
    /// A single-style cell.
    pub fn text(text: impl Into<String>, width: usize, style: Style) -> Self {
        Self {
            spans: vec![Span::styled(text.into(), style)],
            width,
        }
    }
}

/// Lay cells out left to right with one-column gaps. Spans past a cell's
/// budget are dropped and the one straddling it is truncated with an
/// ellipsis; a short cell is space-padded. A `fill` paints the Selected Row's
/// band under the whole line — gaps and trailing padding included — while
/// every span keeps its own foreground: a continuous band, not inverted video
/// (ADR 0005).
pub fn render_cells(cells: Vec<Cell>, fill: Option<Color>) -> Line<'static> {
    let mut spans = Vec::with_capacity(cells.len() * 3);
    for (i, cell) in cells.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" ".repeat(GAP)));
        }
        let mut used = 0;
        for span in cell.spans {
            let remaining = cell.width.saturating_sub(used);
            if remaining == 0 {
                break;
            }
            let text = truncate(&span.content, remaining);
            used += text.width();
            spans.push(Span::styled(text, span.style));
        }
        if used < cell.width {
            spans.push(Span::raw(" ".repeat(cell.width - used)));
        }
    }
    let line = Line::from(spans);
    match fill {
        Some(color) => line.style(Style::default().bg(color)),
        None => line,
    }
}
