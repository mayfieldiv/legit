use crate::{
    app::ticket_list::{DependencySummary, QueueTier, RowMarker, TicketSummary},
    color::repo_color,
    format::fetched_age_spans,
    palette::Palette,
    ticket::{Mode, TicketState},
};
use chrono::{DateTime, Utc};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

pub fn content_lines(
    summary: &TicketSummary,
    width: usize,
    now: DateTime<Utc>,
    palette: &Palette,
) -> Vec<Line<'static>> {
    let row = &summary.row;
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let state = match &row.marker {
        None => "Frontier".to_owned(),
        Some(RowMarker::Claimed(Some(who))) => format!("claimed by {who}"),
        Some(RowMarker::Claimed(None)) => "claimed".to_owned(),
        Some(RowMarker::After(_) | RowMarker::UnknownDependency(_)) => "blocked".to_owned(),
    };
    let mode = match row.ty.mode() {
        Mode::Afk => "AFK",
        Mode::Hitl => "HITL",
        Mode::Either => "Either",
    };
    let mut lines = vec![
        Line::from(Span::styled(
            format!("{} {}", row.display_ref, row.title),
            bold,
        )),
        Line::from(vec![
            Span::styled(row.repo.clone(), Style::default().fg(repo_color(&row.repo))),
            Span::raw(format!(" · {}", summary.effort)),
        ]),
        Line::from(summary.destination.clone().unwrap_or_default()),
        Line::from(Span::styled(
            format!("{} · {mode}", row.ty.0),
            Style::default().fg(palette.mode(row.ty.mode())),
        )),
        Line::from(Span::styled(
            state,
            Style::default().fg(match row.tier() {
                QueueTier::Frontier => palette.frontier,
                QueueTier::Claimed => palette.claimed,
                QueueTier::Blocked => palette.blocked,
            }),
        )),
        Line::from(fetched_age_spans(row.fetch.fetched_at, now, palette)),
        Line::default(),
        Line::from(Span::styled("waits on (↑)", bold)),
    ];
    dependencies(&mut lines, &summary.waits_on, false, palette);
    lines.push(Line::default());
    lines.push(Line::from(Span::styled("blocks (↓)", bold)));
    dependencies(&mut lines, &summary.blocks, true, palette);
    lines.push(Line::default());
    lines.push(Line::from(format!("p  {}", summary.handoff_prompt)));
    crate::wrap::wrap_lines(lines, width)
}

fn dependencies(
    lines: &mut Vec<Line<'static>>,
    dependencies: &[DependencySummary],
    downstream: bool,
    palette: &Palette,
) {
    if dependencies.is_empty() {
        lines.push(Line::from(Span::styled(
            "none",
            Style::default().fg(palette.muted),
        )));
    }
    for dependency in dependencies {
        match dependency {
            DependencySummary::Unknown(raw) => lines.push(Line::from(Span::styled(
                format!("{raw} — can't find or read"),
                Style::default().fg(palette.blocked),
            ))),
            DependencySummary::Known {
                display_ref,
                title,
                state,
                qualifier,
            } => {
                let closed = *state == TicketState::Closed;
                let color = if closed {
                    palette.muted
                } else if downstream {
                    palette.blocks
                } else {
                    palette.blocked
                };
                lines.push(Line::from(Span::styled(
                    format!("{}{display_ref} {title}", if closed { "✓ " } else { "" }),
                    Style::default().fg(color),
                )));
                if let Some(qualifier) = qualifier {
                    lines.push(Line::from(Span::styled(
                        qualifier.text.clone(),
                        Style::default()
                            .fg(qualifier.repo.as_deref().map_or(palette.muted, repo_color)),
                    )));
                }
            }
        }
    }
}
