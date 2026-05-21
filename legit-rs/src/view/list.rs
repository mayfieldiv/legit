use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    text::Line,
    widgets::Paragraph,
};

use crate::app::model::Model;

#[cfg(test)]
mod tests;

/// Render the PR list region. Empty list shows the "no open PRs" placeholder;
/// later cycles add the loading state, populated rows, and truncation.
pub fn render(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    if model.prs.is_empty() {
        let placeholder =
            Paragraph::new(Line::from("No open pull requests")).alignment(Alignment::Center);
        frame.render_widget(placeholder, area);
        return;
    }

    // Populated rendering follows in a later cycle.
    let placeholder = Paragraph::new("…").alignment(Alignment::Center);
    frame.render_widget(placeholder, area);
}
