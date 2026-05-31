use chrono::{DateTime, Utc};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::model::Model;

pub mod list;

pub fn view(model: &Model, frame: &mut Frame<'_>, now: DateTime<Utc>) {
    let area = frame.area();
    let [main, status] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .areas(area);

    list::render(&model.list, frame, main, now);
    render_status(model, frame, status);
}

fn render_status(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    // PR list errors take priority — they're what the user just tried to do.
    // Generic command errors come next; the keymap hint is the fallback.
    let active_error = model.list.failure().or(model.last_error.as_deref());
    let status_line = if let Some(error) = active_error {
        Line::from(vec![
            Span::styled("error: ", Style::default().fg(Color::Red)),
            Span::styled(error, Style::default().fg(Color::Yellow)),
        ])
    } else {
        Line::from(vec![
            Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" quit"),
        ])
    };
    frame.render_widget(Paragraph::new(status_line), area);
}

#[allow(dead_code)]
fn centered_placeholder(text: &str) -> Paragraph<'_> {
    Paragraph::new(text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE))
}
