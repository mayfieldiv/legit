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
use crate::git_remote::RepoInfo;

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
    // The filter chip row only exists while the filter is editing/applied, so
    // the list gets the row back when it's inactive (mirrored by
    // `Model::sync_viewport`, which must agree on the chrome row count).
    let filter_visible = model.list.filter().is_visible();
    let mut constraints = vec![Constraint::Length(1)];
    if filter_visible {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Min(1));
    constraints.push(Constraint::Length(1));
    let rects = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    render_tabs(model, frame, rects[0]);
    let mut next = 1;
    if filter_visible {
        render_filter_chip(model, frame, rects[next]);
        next += 1;
    }
    list::render(model, frame, rects[next], now);
    render_status(model, frame, rects[next + 1]);
}

/// The filter chip above the list: `/text` plus a block cursor while editing;
/// just the accented text once applied, so it reads as a sticky chip.
fn render_filter_chip(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let filter = model.list.filter();
    let mut spans = vec![
        Span::styled(
            "/",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(filter.text().to_owned()),
    ];
    if filter.is_editing() {
        spans.push(Span::styled("█", Style::default().fg(Color::Cyan)));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The Repo Tab bar: `All` plus one tab per Tracked Repo, the active tab
/// bracketed and accented (`[All]  acme/web `), matching the TS tab bar.
fn render_tabs(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let repos = model.tracked_repos();
    let active = model.active_tab.min(repos.len());
    let labels = std::iter::once("All".to_owned()).chain(repos.iter().map(RepoInfo::slug));
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
    // queued) followed by key hints — the filter editor's own hints while it
    // is open, the normal-mode hints otherwise.
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
    let bold = Style::default().add_modifier(Modifier::BOLD);
    if model.list.filter().is_editing() {
        left.push(Span::styled("enter", bold));
        left.push(Span::raw(" apply  "));
        left.push(Span::styled("esc", bold));
        left.push(Span::raw(" clear"));
    } else {
        left.push(Span::styled("q", bold));
        left.push(Span::raw(" quit  "));
        left.push(Span::styled("g", bold));
        left.push(Span::raw(format!(" group: {}", grouping_label(model))));
        left.push(Span::raw("  "));
        left.push(Span::styled("h/l", bold));
        left.push(Span::raw(" tabs  "));
        left.push(Span::styled("/", bold));
        left.push(Span::raw(" filter"));
    }
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
