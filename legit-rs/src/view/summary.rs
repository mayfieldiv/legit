//! The right-side summary panel for the selected PR. Renders, top to bottom:
//! smart-status reason (coloured by tier) -> mergeable state -> reviews summary
//! -> threads summary -> CI checks summary -> file-category size breakdown ->
//! worktree path placeholder -> footer GitHub URL. Sections whose enrichment
//! hasn't arrived render a "Loading…" placeholder so the panel fills in
//! reactively as the per-PR fan-out lands.
//!
//! Panel width is a function of the terminal width: hidden below 80 columns,
//! 36 columns at 80-139, 50 columns at >=140. `panel_width` is the single
//! source of truth shared by `view::view` (which splits the main area) and the
//! tests.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::model::Model;

#[cfg(test)]
mod tests;

/// Below this width the summary panel is hidden entirely — the list takes the
/// whole row.
const MIN_WIDTH_FOR_PANEL: u16 = 80;
/// At this width and above the panel widens from 36 to 50 columns.
const WIDE_WIDTH: u16 = 140;
/// Panel width in the narrow band (80-139 columns).
const NARROW_PANEL_WIDTH: u16 = 36;
/// Panel width at >=140 columns.
const WIDE_PANEL_WIDTH: u16 = 50;

/// The summary panel's width for a given terminal width, or `None` when the
/// terminal is too narrow to show it (the list then takes the whole row).
pub fn panel_width(total_cols: u16) -> Option<u16> {
    if total_cols < MIN_WIDTH_FOR_PANEL {
        None
    } else if total_cols < WIDE_WIDTH {
        Some(NARROW_PANEL_WIDTH)
    } else {
        Some(WIDE_PANEL_WIDTH)
    }
}

/// Render the summary panel into `area`. Assumes `area` is the panel's region
/// (already split off the list by the caller).
pub fn render(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let Some(_pr) = model.list.selected_pr() else {
        let line = Line::from(Span::styled(
            "No PR selected",
            Style::default().fg(Color::Gray),
        ));
        frame.render_widget(Paragraph::new(line), area);
        return;
    };
    let _ = Modifier::BOLD;
}
