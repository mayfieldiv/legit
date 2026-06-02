use chrono::{DateTime, Utc};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::grouping::Grouping;
use crate::app::model::{Model, StatusKind};

pub mod list;

/// Short label for the active grouping mode, shown in the status-bar `g` hint.
fn grouping_label(model: &Model) -> &'static str {
    match model.list.grouping() {
        Grouping::SmartStatus => "smart-status",
        Grouping::Repo => "repo",
        Grouping::None => "none",
    }
}

pub fn view(model: &Model, frame: &mut Frame<'_>, now: DateTime<Utc>) {
    let area = frame.area();
    let [tabs, main, status] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(area);

    render_tabs(model, frame, tabs);
    list::render(model, frame, main, now);
    render_status(model, frame, status);
}

/// The Repo Tab bar: `All` plus one tab per Tracked Repo, the active tab
/// bracketed and accented (`[All]  acme/web `), matching the TS tab bar.
fn render_tabs(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let repos = model.tracked_repos();
    let active = model.active_tab.min(repos.len());
    let labels = std::iter::once("All".to_owned()).chain(repos);
    let mut spans = Vec::new();
    for (i, label) in labels.enumerate() {
        let (text, style) = if i == active {
            (
                format!("[{label}]"),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            (format!(" {label} "), Style::default())
        };
        spans.push(Span::styled(text, style));
        spans.push(Span::raw(" "));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_status(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    // Left: a network-activity indicator (only while requests are in flight or
    // queued) followed by key hints.
    let mut left = Vec::new();
    let stats = model.network_stats;
    if stats.in_flight > 0 || stats.waiting > 0 {
        let indicator = if stats.waiting > 0 {
            format!(
                "[{} in flight, {} waiting] ",
                stats.in_flight, stats.waiting
            )
        } else {
            format!("[{} in flight] ", stats.in_flight)
        };
        left.push(Span::styled(indicator, Style::default().fg(Color::Cyan)));
    }
    left.push(Span::styled(
        "q",
        Style::default().add_modifier(Modifier::BOLD),
    ));
    left.push(Span::raw(" quit  "));
    left.push(Span::styled(
        "g",
        Style::default().add_modifier(Modifier::BOLD),
    ));
    left.push(Span::raw(format!(" group: {}", grouping_label(model))));
    left.push(Span::raw("  "));
    left.push(Span::styled(
        "h/l",
        Style::default().add_modifier(Modifier::BOLD),
    ));
    left.push(Span::raw(" tabs"));
    frame.render_widget(Paragraph::new(Line::from(left)), area);

    // Right: a hard list-load failure takes precedence; otherwise the transient
    // status message (info / success / error). Rendered right-aligned over the
    // same row so it sits opposite the hints.
    if let Some(failure) = model.list.failure() {
        let line = Line::from(vec![
            Span::styled("error: ", Style::default().fg(Color::Red)),
            Span::styled(failure.to_owned(), Style::default().fg(Color::Yellow)),
        ]);
        frame.render_widget(Paragraph::new(line).alignment(Alignment::Right), area);
    } else if let Some(status) = &model.status {
        let color = match status.kind {
            StatusKind::Info => Color::Gray,
            StatusKind::Success => Color::Green,
            StatusKind::Error => Color::Red,
        };
        let line = Line::from(Span::styled(
            status.text.clone(),
            Style::default().fg(color),
        ));
        frame.render_widget(Paragraph::new(line).alignment(Alignment::Right), area);
    }
}

#[allow(dead_code)]
fn centered_placeholder(text: &str) -> Paragraph<'_> {
    Paragraph::new(text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE))
}
