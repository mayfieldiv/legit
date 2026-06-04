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
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::model::Model;
use crate::blocker::Tier;

/// Placeholder text for a section whose enrichment hasn't arrived yet.
const LOADING: &str = "Loading…";

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
    let Some(pr) = model.list.selected_pr() else {
        let line = Line::from(Span::styled(
            "No PR selected",
            Style::default().fg(Color::Gray),
        ));
        frame.render_widget(Paragraph::new(line), area);
        return;
    };

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(smart_status_line(model, pr));
    lines.push(mergeable_line(pr));

    frame.render_widget(Paragraph::new(lines), area);
}

/// The smart-status reason line, coloured by tier (me-blocking magenta,
/// waiting-on-author yellow, needs-review gray). `Loading…` until the PR's
/// blocker has been derived (both threads and reviews arrived).
fn smart_status_line(model: &Model, pr: &crate::github::rest::PR) -> Line<'static> {
    match model.blockers.get(&pr.key()) {
        Some(result) => Line::from(Span::styled(
            result.reason.clone(),
            Style::default().fg(tier_color(result.tier)),
        )),
        None => loading_line(),
    }
}

/// The mergeable-state line. Mirrors the TS `formatMergeable`: `CONFLICTING` ->
/// "! conflict" (red), `MERGEABLE` -> "✓ mergeable" (green), anything else
/// (including `UNKNOWN`) -> "? merge unknown" (gray).
fn mergeable_line(pr: &crate::github::rest::PR) -> Line<'static> {
    let (text, color) = match pr.mergeable.as_str() {
        "CONFLICTING" => ("! conflict", Color::Red),
        "MERGEABLE" => ("✓ mergeable", Color::Green),
        _ => ("? merge unknown", Color::Gray),
    };
    Line::from(Span::styled(text, Style::default().fg(color)))
}

/// Theme colour for a smart-status tier. Mirrors the TS `blockerTierColor`.
fn tier_color(tier: Tier) -> Color {
    match tier {
        Tier::MeBlocking => Color::Magenta,
        Tier::WaitingOnAuthor => Color::Yellow,
        Tier::NeedsReview => Color::Gray,
    }
}

/// A muted `Loading…` placeholder line for a not-yet-arrived section.
fn loading_line() -> Line<'static> {
    Line::from(Span::styled(LOADING, Style::default().fg(Color::Gray)))
}
