//! The ticket surface: the effort rail beside the tier-grouped ticket queue,
//! under its own app header and above the shared status bar.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{app::model::Model, palette::Palette};

pub fn render(model: &Model, frame: &mut Frame<'_>, area: Rect, palette: &Palette) {
    let [header, main, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);
    render_header(frame, header, palette);
    frame.render_widget(
        Paragraph::new(Line::from("No efforts found")).alignment(Alignment::Center),
        main,
    );
    render_status(model, frame, status, palette);
}

fn render_header(frame: &mut Frame<'_>, area: Rect, palette: &Palette) {
    let bold = |color| Style::default().fg(color).add_modifier(Modifier::BOLD);
    let line = Line::from(vec![
        Span::styled("legit", bold(palette.accent)),
        Span::raw(" — "),
        Span::styled("Tickets", bold(palette.accent)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_status(model: &Model, frame: &mut Frame<'_>, area: Rect, palette: &Palette) {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let left = Line::from(vec![
        Span::styled("j/k", bold),
        Span::raw(" nav  "),
        Span::styled("t", bold),
        Span::raw(" PRs  "),
        Span::styled("q", bold),
        Span::raw(" quit"),
    ]);
    frame.render_widget(Paragraph::new(left), area);
    super::render_status_overlay(model, frame, area, palette);
}
