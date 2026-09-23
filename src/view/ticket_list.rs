//! The ticket surface: the effort rail beside the tier-grouped ticket queue,
//! under its own app header and above the shared status bar (spec §6.1–§6.2,
//! the prototype's "Efforts pane" layout).

use chrono::{DateTime, Utc};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

use super::table::{Column, ColumnBounds, Columns, FillReserves, Table};
use crate::{
    app::{
        model::Model,
        ticket_list::{
            DiscoveryUnit, EffortCard, ModeFilter, QueueContentWidths, QueueRow, QueueTier,
            RailCard, RowMarker, TicketList, TicketRow,
        },
        ticket_list_layout::{DIVIDER_WIDTH, rail_width, summary_width},
    },
    color::repo_color,
    format::{
        REFRESH_GLYPH, fetched_age_spans, format_age, pad_to_width, truncate, truncate_middle,
    },
    palette::Palette,
    ticket::{EffortSource, Mode},
};

#[cfg(test)]
mod tests;

mod summary;

pub fn render(
    model: &Model,
    frame: &mut Frame<'_>,
    area: Rect,
    now: DateTime<Utc>,
    palette: &Palette,
) {
    let [header, tabs, filters, main, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);
    render_header(model, frame, header, palette);
    super::render_tabs(model, frame, tabs, palette);
    let tickets = &model.tickets;
    let main = if let Some(width) = summary_width(main.width) {
        let [remaining, divider, panel] = Layout::horizontal([
            Constraint::Min(0),
            Constraint::Length(DIVIDER_WIDTH),
            Constraint::Length(width),
        ])
        .areas(main);
        render_divider(frame, divider, palette);
        summary::render(tickets.selected_summary(), frame, panel, now, palette);
        remaining
    } else {
        main
    };
    let rail_width = rail_width(main.width);
    render_filters(tickets, frame, filters, rail_width.is_none(), palette);
    if tickets.rail().next().is_none() {
        let text = if tickets.is_loading() {
            "Loading efforts…"
        } else {
            "No efforts found"
        };
        frame.render_widget(
            Paragraph::new(Line::from(text)).alignment(Alignment::Center),
            main,
        );
    } else {
        match rail_width {
            Some(rail_width) => {
                let [rail, divider, queue] = Layout::horizontal([
                    Constraint::Length(rail_width),
                    Constraint::Length(DIVIDER_WIDTH),
                    Constraint::Min(1),
                ])
                .areas(main);
                render_rail(tickets, frame, rail, now, palette);
                render_divider(frame, divider, palette);
                render_queue(tickets, frame, queue, false, now, palette);
            }
            None => render_queue(tickets, frame, main, true, now, palette),
        }
    }
    render_status(model, frame, status, palette);
}

const MODE_CHIPS: [(ModeFilter, &str); 3] = [
    (ModeFilter::All, "All"),
    (ModeFilter::Afk, "AFK"),
    (ModeFilter::Hitl, "HITL"),
];

/// The Mode chips, the Either legend, and the active effort. Beside the rail
/// the effort trails, since its highlighted card already names it; with the
/// rail hidden this row is the only place the effort is named, so it leads,
/// truncated to what the tightened chips leave it.
fn render_filters(
    tickets: &TicketList,
    frame: &mut Frame<'_>,
    area: Rect,
    rail_hidden: bool,
    palette: &Palette,
) {
    let chip = |filter: ModeFilter, text: String| {
        Span::styled(
            text,
            Style::default().fg(if filter == tickets.mode_filter() {
                palette.accent
            } else {
                palette.muted
            }),
        )
    };
    let spans = if rail_hidden {
        let mut tail = vec![Span::raw(" · ")];
        for (i, (filter, label)) in MODE_CHIPS.into_iter().enumerate() {
            if i > 0 {
                tail.push(Span::raw(" "));
            }
            let text = if filter == tickets.mode_filter() {
                format!("[{label}]")
            } else {
                label.to_owned()
            };
            tail.push(chip(filter, text));
        }
        tail.push(Span::raw(" · * Either"));
        let tail_width: usize = tail.iter().map(Span::width).sum();
        let effort = truncate(
            tickets.effort_filter_label(),
            usize::from(area.width).saturating_sub(tail_width),
        );
        let mut spans = vec![Span::raw(effort)];
        spans.extend(tail);
        spans
    } else {
        let mut spans = vec![Span::raw("Mode ")];
        for (filter, label) in MODE_CHIPS {
            let text = if filter == tickets.mode_filter() {
                format!("[{label}] ")
            } else {
                format!(" {label}  ")
            };
            spans.push(chip(filter, text));
        }
        spans.push(Span::raw(format!(
            " · * Either · {}",
            tickets.effort_filter_label()
        )));
        spans
    };
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_header(model: &Model, frame: &mut Frame<'_>, area: Rect, palette: &Palette) {
    let (efforts, frontier) = model
        .tickets
        .rail()
        .filter_map(|card| match card {
            RailCard::Effort(card) => Some(card),
            RailCard::Failure { .. } | RailCard::Incomplete { .. } => None,
        })
        .fold((0, 0), |(efforts, frontier), card| {
            let on_frontier = card
                .outcome
                .as_ref()
                .map_or(0, |summary| summary.counts.frontier);
            (efforts + 1, frontier + on_frontier)
        });
    let noun = if efforts == 1 { "effort" } else { "efforts" };
    let bold = |color| Style::default().fg(color).add_modifier(Modifier::BOLD);
    let line = Line::from(vec![
        Span::styled("legit", bold(palette.accent)),
        Span::raw(" — "),
        Span::styled("Tickets", bold(palette.accent)),
        Span::raw(format!(" — {efforts} {noun} · {frontier} frontier")),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_divider(frame: &mut Frame<'_>, area: Rect, palette: &Palette) {
    let style = Style::default().fg(palette.separator);
    let lines = (0..area.height)
        .map(|_| Line::from(Span::styled("│", style)))
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), area);
}

// ── effort rail ──────────────────────────────────────────────────────────────

fn render_rail(
    tickets: &TicketList,
    frame: &mut Frame<'_>,
    area: Rect,
    now: DateTime<Utc>,
    palette: &Palette,
) {
    let width = usize::from(area.width);
    let mut lines = vec![
        Line::from(Span::styled(
            pad_to_width("All efforts", width),
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        )),
        Line::default(),
    ];
    if tickets.all_efforts_selected() {
        lines[0] = lines[0]
            .clone()
            .style(Style::default().bg(palette.selected_bg));
    }
    let mut selected_range = 0..1;
    for card in tickets.rail() {
        let selected = matches!(&card, RailCard::Effort(card) if card.selected);
        let start = lines.len();
        lines.extend(match card {
            RailCard::Failure { unit, error } => {
                discovery_failure_card(unit, error, width, palette)
            }
            RailCard::Incomplete { unit, caveat } => {
                discovery_incomplete_card(unit, caveat, width, palette)
            }
            RailCard::Effort(card) => effort_card(card, width, now, palette),
        });
        if selected {
            selected_range = start..lines.len();
            for line in &mut lines[selected_range.clone()] {
                *line = line.clone().style(Style::default().bg(palette.selected_bg));
            }
        }
        lines.push(Line::default());
    }
    let offset = selected_range
        .end
        .saturating_sub(usize::from(area.height))
        .min(selected_range.start);
    frame.render_widget(
        Paragraph::new(lines.into_iter().skip(offset).collect::<Vec<_>>()),
        area,
    );
}

fn effort_card(
    card: &EffortCard,
    width: usize,
    now: DateTime<Utc>,
    palette: &Palette,
) -> Vec<Line<'static>> {
    let source = match card.source {
        EffortSource::GitHub => "github",
        EffortSource::Local => "local",
    };
    let muted = Style::default().fg(palette.muted);
    let title = Span::styled(
        card.title.clone(),
        Style::default().add_modifier(Modifier::BOLD),
    );
    let mut lines = vec![repo_led_line(&card.repo, title, width, palette)];
    match &card.outcome {
        Ok(summary) => {
            let counts = summary.counts;
            lines.push(Line::from(Span::styled(
                truncate(
                    &format!(
                        "{source} · {}/{} decided · {} frontier",
                        counts.decided, counts.total, counts.frontier
                    ),
                    width,
                ),
                muted,
            )));
            lines.push(Line::from(Span::styled(
                truncate(summary.destination.as_deref().unwrap_or(""), width),
                muted,
            )));
        }
        Err(reason) => {
            lines.push(Line::from(vec![
                Span::styled(format!("{source} · "), muted),
                Span::styled(
                    truncate(
                        "couldn't read — r to retry",
                        width.saturating_sub(source.width() + 3),
                    ),
                    Style::default().fg(palette.error),
                ),
            ]));
            lines.push(Line::from(Span::styled(
                truncate(reason, width),
                Style::default().fg(palette.warning),
            )));
        }
    }
    let mut fetch = Vec::new();
    if card.fetch.refreshing {
        fetch.push(Span::styled(
            format!("{REFRESH_GLYPH} refreshing"),
            Style::default().fg(palette.accent),
        ));
        if card.fetch.fetched_at.is_some() {
            fetch.push(Span::styled(" · ", muted));
        }
    }
    fetch.extend(fetched_age_spans(card.fetch.fetched_at, now, palette));
    if !fetch.is_empty() {
        lines.push(Line::from(fetch));
    }
    lines
}

/// A discovery unit that failed before attributing any Effort: the unit's
/// name where a card's repo goes, so the failure reads in the same place a
/// card would have, and what it couldn't do worded by source — a local unit
/// probes the filesystem, a GitHub unit reads the map.
fn discovery_failure_card(
    unit: &DiscoveryUnit,
    error: &str,
    width: usize,
    palette: &Palette,
) -> Vec<Line<'static>> {
    let failure = match unit.source() {
        EffortSource::Local => "couldn't probe — r to retry",
        EffortSource::GitHub => "couldn't read — r to retry",
    };
    let failure = Span::styled(failure, Style::default().fg(palette.error));
    vec![
        repo_led_line(unit.label(), failure, width, palette),
        Line::from(Span::styled(
            truncate(error, width),
            Style::default().fg(palette.warning),
        )),
    ]
}

/// A discovery unit whose read settled short of the whole unit: `repo ·
/// incomplete` in the warning colour (a warning, not the failure red — its
/// Efforts did pool and keep their cards), then what lies beyond the window.
fn discovery_incomplete_card(
    unit: &DiscoveryUnit,
    caveat: &str,
    width: usize,
    palette: &Palette,
) -> Vec<Line<'static>> {
    let warning = Style::default().fg(palette.warning);
    vec![
        repo_led_line(
            unit.label(),
            Span::styled("incomplete", warning),
            width,
            palette,
        ),
        Line::from(Span::styled(truncate(caveat, width), warning)),
    ]
}

/// `repo · <tail>`: the repo in its Repo Color and bold, a separator, then
/// `tail` truncated to what remains — so a long tail never pushes the repo
/// off the card.
fn repo_led_line(
    repo: &str,
    tail: Span<'static>,
    width: usize,
    palette: &Palette,
) -> Line<'static> {
    let tail_width = width.saturating_sub(repo.width() + 3);
    Line::from(vec![
        Span::styled(
            repo.to_owned(),
            Style::default()
                .fg(repo_color(repo))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::default().fg(palette.separator)),
        Span::styled(truncate(&tail.content, tail_width), tail.style),
    ])
}

// ── queue ────────────────────────────────────────────────────────────────────

/// The ticket queue's columns, in display order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum QueueColumn {
    Indicator,
    Ref,
    Repo,
    Type,
    State,
    Title,
    Block,
    Updated,
}

/// The one queue column declaration, fitted to `row_width` with the cached
/// display-set measurements. `compact` means the effort rail is hidden, so
/// the row carries a State column in its place. Ref is the only required
/// fitted column: it yields toward its minimum when the row cannot hold it
/// and Title's 18-column admission reserve, and takes surplus first once
/// Title has 40. Metadata is admitted by rank — Type, Repo, Block, Updated.
fn queue_table(row_width: usize, content: QueueContentWidths, compact: bool) -> Table<QueueColumn> {
    Columns::new()
        .column(Column::fixed(QueueColumn::Indicator, "", 1))
        .column(Column::fitted(
            QueueColumn::Ref,
            "Ticket",
            content.display_ref,
            ColumnBounds::new(6, 14, 64),
        ))
        .column(
            Column::fitted(
                QueueColumn::Repo,
                "Repo",
                content.repo,
                ColumnBounds::new(4, 14, 32),
            )
            .optional(1),
        )
        .column(
            Column::fitted(
                QueueColumn::Type,
                "Type",
                content.ty,
                ColumnBounds::new(4, 12, 24),
            )
            .optional(0),
        )
        .column_if(compact, Column::fixed(QueueColumn::State, "State", 8))
        .fill(QueueColumn::Title, "Title", FillReserves::new(18, 40))
        .column(Column::fixed(QueueColumn::Block, "Block", 7).optional(2))
        .column(Column::fixed(QueueColumn::Updated, "Updated", 7).optional(3))
        .fit(row_width)
}

fn render_queue(
    tickets: &TicketList,
    frame: &mut Frame<'_>,
    area: Rect,
    compact: bool,
    now: DateTime<Utc>,
    palette: &Palette,
) {
    let [header_area, rows_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
    let width = usize::from(area.width);
    let table = queue_table(width, tickets.content_widths(), compact);
    frame.render_widget(
        Paragraph::new(table.header(Style::default().add_modifier(Modifier::BOLD))),
        header_area,
    );

    if tickets.visible_is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from("No open tickets")).alignment(Alignment::Center),
            rows_area,
        );
        return;
    }
    let lines: Vec<Line<'static>> = tickets
        .visible_rows()
        .map(|(row, selected)| match row {
            QueueRow::Header(tier) => tier_header_line(*tier, width, palette),
            QueueRow::Ticket(row) => ticket_line(row, &table, selected, now, palette),
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), rows_area);
}

/// `── <tier> ` in the tier's colour, padded to the row width and bolded: the
/// queue's state signal, so the rule takes the tier colour (unlike the PR
/// list's accent-coloured Smart-status headers).
fn tier_header_line(tier: QueueTier, width: usize, palette: &Palette) -> Line<'static> {
    let text = format!("  ── {} ", tier.label());
    Line::from(Span::styled(
        pad_to_width(&text, width),
        Style::default()
            .fg(tier_color(tier, palette))
            .add_modifier(Modifier::BOLD),
    ))
}

/// The tier's role colour, echoed by the markers `title_spans` paints.
fn tier_color(tier: QueueTier, palette: &Palette) -> Color {
    match tier {
        QueueTier::Frontier => palette.frontier,
        QueueTier::Claimed => palette.claimed,
        QueueTier::Blocked => palette.blocked,
    }
}

/// One ticket's display row: the fitted table asks for each present column's
/// content by ID, so the row can neither omit nor reorder a column.
fn ticket_line(
    row: &TicketRow,
    table: &Table<QueueColumn>,
    selected: bool,
    now: DateTime<Utc>,
    palette: &Palette,
) -> Line<'static> {
    // The Selected Row brightens only the title; every other cell keeps its
    // semantic foreground over the band the table lays down (ADR 0005).
    let title_style = if selected {
        Style::default().fg(palette.selected_fg)
    } else {
        Style::default()
    };
    // Without a Type column the title carries the Either tag, so the indicator
    // cell stays free for the refresh glyph and neither signal hides the other.
    let type_is_hidden = table.width(QueueColumn::Type).is_none();
    table.row(selected.then_some(palette.selected_bg), |column, width| {
        let text = |text: String, style: Style| vec![Span::styled(text, style)];
        match column {
            QueueColumn::Indicator => text(
                if row.refreshing { REFRESH_GLYPH } else { "" }.to_owned(),
                Style::default().fg(palette.accent),
            ),
            QueueColumn::Ref => text(
                truncate_middle(&row.display_ref, width),
                Style::default()
                    .fg(palette.count)
                    .add_modifier(Modifier::BOLD),
            ),
            QueueColumn::Repo => text(
                truncate_middle(&row.repo, width),
                Style::default().fg(repo_color(&row.repo)),
            ),
            QueueColumn::Type => text(
                if row.ty.mode() == Mode::Either {
                    format!("*{}", row.ty.0)
                } else {
                    row.ty.0.clone()
                },
                Style::default().fg(palette.mode(row.ty.mode())),
            ),
            QueueColumn::State => text(
                row.tier().label().to_owned(),
                Style::default().fg(tier_color(row.tier(), palette)),
            ),
            QueueColumn::Title => {
                let either_tag = (type_is_hidden && row.ty.mode() == Mode::Either)
                    .then(|| Span::styled("*", Style::default().fg(palette.mode(Mode::Either))));
                title_spans(
                    &row.title,
                    either_tag,
                    row.marker.as_ref(),
                    width,
                    title_style,
                    palette,
                )
            }
            QueueColumn::Block => block_spans(row.upstream, row.downstream, palette),
            QueueColumn::Updated => text(
                row.updated_at
                    .map_or_else(String::new, |stamp| format_age(stamp, now)),
                Style::default(),
            ),
        }
    })
}

/// An optional leading `tag`, the title, then its state marker — `⟨claimed
/// X⟩`, `⟨after Y⟩`, or `⟨dep? Z⟩` — fitted to a `width`-column cell with
/// the title truncated first so the tag and marker survive.
fn title_spans(
    title: &str,
    tag: Option<Span<'static>>,
    marker: Option<&RowMarker>,
    width: usize,
    title_style: Style,
    palette: &Palette,
) -> Vec<Span<'static>> {
    let tag_width = tag.as_ref().map_or(0, |tag| tag.width());
    let mut spans: Vec<Span<'static>> = tag.into_iter().collect();
    let Some(marker) = marker else {
        spans.push(Span::styled(
            truncate(title, width.saturating_sub(tag_width)),
            title_style,
        ));
        return spans;
    };
    let (marker, color) = match marker {
        RowMarker::Claimed(Some(who)) => (format!("⟨claimed {who}⟩"), palette.claimed),
        RowMarker::Claimed(None) => ("⟨claimed⟩".to_owned(), palette.claimed),
        RowMarker::After(target) => (format!("⟨after {target}⟩"), palette.blocked),
        RowMarker::UnknownDependency(raw) => (format!("⟨dep? {raw}⟩"), palette.blocked),
    };
    // The marker is the row's state signal, so it takes the width first and
    // the title gets the rest — none at all when the marker alone fills the
    // cell, where the table truncates the marker rather than lose it.
    let title_budget = width.saturating_sub(tag_width + marker.width() + 1);
    let marker = Span::styled(marker, Style::default().fg(color));
    if title_budget == 0 {
        spans.push(marker);
    } else {
        spans.push(Span::styled(truncate(title, title_budget), title_style));
        spans.push(Span::raw(" "));
        spans.push(marker);
    }
    spans
}

/// `↑N` open upstream Dependencies (red) and `↓N` open downstream dependents
/// (blue), either omitted when zero.
fn block_spans(upstream: usize, downstream: usize, palette: &Palette) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    if upstream > 0 {
        spans.push(Span::styled(
            format!("↑{upstream}"),
            Style::default().fg(palette.blocked),
        ));
    }
    if downstream > 0 {
        if !spans.is_empty() {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(
            format!("↓{downstream}"),
            Style::default().fg(palette.blocks),
        ));
    }
    spans
}

fn render_status(model: &Model, frame: &mut Frame<'_>, area: Rect, palette: &Palette) {
    if area.width < 100 {
        let hints = if area.width < 60 {
            "p copy t PRs"
        } else {
            "J/K effort m mode p/y copy"
        };
        frame.render_widget(Paragraph::new(hints), area);
        super::render_status_right(model, frame, area, palette);
        return;
    }
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let left = Line::from(vec![
        Span::styled("j/k", bold),
        Span::raw(" nav  "),
        Span::styled("J/K", bold),
        Span::raw(" efforts  "),
        Span::styled("h/l", bold),
        Span::raw(" tabs  "),
        Span::styled("m", bold),
        Span::raw(" mode  "),
        Span::styled("p/y", bold),
        Span::raw(" copy prompt/ref  "),
        Span::styled("r/R", bold),
        Span::raw(" refresh  "),
        Span::styled("t", bold),
        Span::raw(" PRs  "),
        Span::styled("q", bold),
        Span::raw(" quit"),
    ]);
    frame.render_widget(Paragraph::new(left), area);
    super::render_status_right(model, frame, area, palette);
}
