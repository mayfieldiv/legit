use crate::{
    app::{ticket_list::TicketSummary, ticket_summary_layout},
    format::pad_to_width,
    palette::Palette,
};
use chrono::{DateTime, Utc};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

pub(super) fn render(
    summary: Option<&TicketSummary>,
    frame: &mut Frame<'_>,
    area: Rect,
    now: DateTime<Utc>,
    palette: &Palette,
) {
    let Some(summary) = summary else {
        return;
    };
    let lines =
        ticket_summary_layout::content_lines(summary, usize::from(area.width), now, palette);
    let height = lines.len();
    let viewport = usize::from(area.height);
    let max_scroll = height.saturating_sub(viewport);
    let scroll = summary.scroll().min(max_scroll);
    frame.render_widget(
        Paragraph::new(lines.into_iter().skip(scroll).collect::<Vec<_>>()),
        area,
    );
    if scroll < max_scroll && area.height > 0 {
        let remaining = height.saturating_sub(scroll + viewport.saturating_sub(1));
        let text = pad_to_width(
            &format!("+{remaining} more ↓ · PgDn / wheel"),
            usize::from(area.width),
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                text,
                Style::default().fg(palette.muted),
            ))),
            Rect {
                y: area.y + area.height - 1,
                height: 1,
                ..area
            },
        );
    }
}
