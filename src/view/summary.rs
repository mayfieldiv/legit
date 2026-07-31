//! Painting for the right-side summary panel. The complete content layout
//! lives in `app::summary_layout`, where both this renderer and the reducer can
//! consume it without making state transitions depend on view modules.

use chrono::{DateTime, Utc};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{
    app::{model::Model, summary_layout},
    format::pad_to_width,
    palette::Palette,
};

#[cfg(test)]
mod tests;

/// Render the summary panel into `area`. Assumes `area` is the panel's region
/// (already split off the list by the caller).
pub fn render(
    model: &Model,
    frame: &mut Frame<'_>,
    area: Rect,
    now: DateTime<Utc>,
    palette: &Palette,
) {
    let Some(pr) = model.list.selected_pr() else {
        let line = Line::from(Span::styled(
            "No PR selected",
            Style::default().fg(palette.muted),
        ));
        frame.render_widget(Paragraph::new(line), area);
        return;
    };

    let lines = summary_layout::content_lines(model, pr, now, usize::from(area.width));
    let content_height = lines.len();
    let viewport = usize::from(area.height);
    let max_scroll = content_height.saturating_sub(viewport);
    let scroll = model.summary.offset().min(max_scroll);
    let paragraph_scroll = u16::try_from(scroll).unwrap_or(u16::MAX);

    frame.render_widget(Paragraph::new(lines).scroll((paragraph_scroll, 0)), area);

    // Replace the last visible content row with an explicit affordance while
    // anything remains below it. Count the overwritten row too: it is hidden
    // until the user scrolls, just like the content originally below the
    // viewport. At the last screenful every content row is visible and the
    // affordance disappears.
    if scroll < max_scroll && area.height > 0 {
        let visible_content_rows = viewport.saturating_sub(1);
        let remaining = content_height.saturating_sub(scroll + visible_content_rows);
        let indicator_area = Rect {
            y: area.y + area.height - 1,
            height: 1,
            ..area
        };
        let indicator_text = pad_to_width(
            &format!("+{remaining} more ↓"),
            usize::from(indicator_area.width),
        );
        let indicator = Line::from(Span::styled(
            indicator_text,
            Style::default().fg(palette.muted),
        ));
        frame.render_widget(Paragraph::new(indicator), indicator_area);
    }
}
