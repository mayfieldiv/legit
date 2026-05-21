use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::model::Model;

pub mod list;

pub fn view(model: &Model, frame: &mut Frame<'_>) {
    let area = frame.area();
    let [main, status] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .areas(area);

    list::render(model, frame, main);
    render_status(model, frame, status);
}

fn render_status(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let status_line = if let Some(error) = &model.last_error {
        Line::from(vec![
            Span::styled("error: ", Style::default().fg(Color::Red)),
            Span::styled(error.as_str(), Style::default().fg(Color::Yellow)),
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
