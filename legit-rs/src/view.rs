use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::model::Model;

pub fn view(model: &Model, frame: &mut Frame<'_>) {
    let area = frame.area();
    let [main, status] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .areas(area);

    let placeholder = Paragraph::new("legit-rs — no PRs yet")
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE));
    frame.render_widget(placeholder, main);

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
    frame.render_widget(Paragraph::new(status_line), status);
}
