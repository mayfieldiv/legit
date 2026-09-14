//! The ticket surface: the effort rail beside the tier-grouped ticket queue,
//! under its own app header and above the shared status bar (spec §6.1–§6.2,
//! the prototype's "Efforts pane" layout).

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

use super::row::{Cell, GAP, fill_width, render_cells};
use crate::{
    app::{
        model::Model,
        ticket_list::{
            EffortCard, QueueContentWidths, QueueRow, QueueTier, RailCard, RowMarker, TicketList,
            TicketRow,
        },
        ticket_list_layout::{DIVIDER_WIDTH, rail_width},
    },
    color::repo_color,
    format::{pad_to_width, truncate, truncate_middle},
    palette::Palette,
    ticket::EffortSource,
};

#[cfg(test)]
mod tests;

pub fn render(model: &Model, frame: &mut Frame<'_>, area: Rect, palette: &Palette) {
    let [header, main, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);
    render_header(model, frame, header, palette);
    let tickets = &model.tickets;
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
        match rail_width(main.width) {
            Some(rail_width) => {
                let [rail, divider, queue] = Layout::horizontal([
                    Constraint::Length(rail_width),
                    Constraint::Length(DIVIDER_WIDTH),
                    Constraint::Min(1),
                ])
                .areas(main);
                render_rail(tickets, frame, rail, palette);
                render_divider(frame, divider, palette);
                render_queue(tickets, frame, queue, palette);
            }
            None => render_queue(tickets, frame, main, palette),
        }
    }
    render_status(model, frame, status, palette);
}

fn render_header(model: &Model, frame: &mut Frame<'_>, area: Rect, palette: &Palette) {
    let (efforts, frontier) = model
        .tickets
        .rail()
        .filter_map(|card| match card {
            RailCard::Effort(card) => Some(card),
            RailCard::Failure { .. } => None,
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

/// The rail: the `All efforts` entry (the only filter this slice has, so it
/// is always the active one), then one two-line card per failed discovery
/// unit and one three-line card per Effort, each followed by a blank row.
fn render_rail(tickets: &TicketList, frame: &mut Frame<'_>, area: Rect, palette: &Palette) {
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
    for card in tickets.rail() {
        lines.extend(match card {
            RailCard::Failure { unit, error } => {
                discovery_failure_card(unit, error, width, palette)
            }
            RailCard::Effort(card) => effort_card(card, width, palette),
        });
        lines.push(Line::default());
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn effort_card(card: &EffortCard, width: usize, palette: &Palette) -> Vec<Line<'static>> {
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
                    truncate("couldn't read", width.saturating_sub(source.width() + 3)),
                    Style::default().fg(palette.error),
                ),
            ]));
            lines.push(Line::from(Span::styled(
                truncate(reason, width),
                Style::default().fg(palette.warning),
            )));
        }
    }
    lines
}

/// A discovery unit that failed before attributing any Effort: the unit's
/// name where a card's repo goes, so the failure reads in the same place a
/// card would have.
fn discovery_failure_card(
    name: &str,
    error: &str,
    width: usize,
    palette: &Palette,
) -> Vec<Line<'static>> {
    let failure = Span::styled("couldn't probe", Style::default().fg(palette.error));
    vec![
        repo_led_line(name, failure, width, palette),
        Line::from(Span::styled(
            truncate(error, width),
            Style::default().fg(palette.warning),
        )),
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

/// The leading one-column glyph slot every queue row starts with, like the
/// PR list's worktree/refresh column. Empty in this slice.
// TODO(#132): the per-row refresh indicator.
const INDICATOR_COL: usize = 1;
const TITLE_COL_MIN: usize = 40;
/// `↑NN ↓NN`.
const BLOCK_COL: usize = 7;
/// Sized like the PR list's Updated column so the header lands where the data
/// will; the cells stay empty until Fetch Age is stamped.
// TODO(#132): render Fetch Age.
const AGE_COL: usize = 7;

/// How a content-sized queue column may grow. It opens at its content width
/// within `min..=opening_max`; once the title has more than `TITLE_COL_MIN`,
/// the surplus widens the fitted columns in order, each up to `max` — so a
/// long ref or type takes room only when the title can spare it.
struct ColumnBounds {
    min: usize,
    opening_max: usize,
    max: usize,
}

impl ColumnBounds {
    fn opening(&self, content: usize) -> usize {
        content.clamp(self.min, self.opening_max)
    }

    fn grown(&self, content: usize) -> usize {
        content.clamp(self.min, self.max)
    }
}

const REF_COL: ColumnBounds = ColumnBounds {
    min: 6,
    opening_max: 14,
    max: 64,
};
const REPO_COL: ColumnBounds = ColumnBounds {
    min: 4,
    opening_max: 14,
    max: 32,
};
const TYPE_COL: ColumnBounds = ColumnBounds {
    min: 4,
    opening_max: 12,
    max: 24,
};

struct QueueLayout {
    width: usize,
    ref_col: usize,
    repo_col: usize,
    type_col: usize,
}

impl QueueLayout {
    fn new(width: usize, content: QueueContentWidths) -> Self {
        let mut layout = Self {
            width,
            ref_col: REF_COL.opening(content.display_ref),
            repo_col: REPO_COL.opening(content.repo),
            type_col: TYPE_COL.opening(content.ty),
        };
        let mut spare = layout.title_col().saturating_sub(TITLE_COL_MIN);
        for (column, grown) in [
            (&mut layout.ref_col, REF_COL.grown(content.display_ref)),
            (&mut layout.repo_col, REPO_COL.grown(content.repo)),
            (&mut layout.type_col, TYPE_COL.grown(content.ty)),
        ] {
            let extra = grown.saturating_sub(*column).min(spare);
            *column += extra;
            spare -= extra;
        }
        layout
    }

    /// Whatever the fixed columns and their gaps leave for the title.
    fn title_col(&self) -> usize {
        fill_width(
            self.width,
            [
                INDICATOR_COL,
                self.ref_col,
                self.repo_col,
                self.type_col,
                BLOCK_COL,
                AGE_COL,
            ],
        )
    }
}

fn render_queue(tickets: &TicketList, frame: &mut Frame<'_>, area: Rect, palette: &Palette) {
    let [header_area, rows_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
    let width = usize::from(area.width);
    let layout = QueueLayout::new(width, tickets.content_widths());
    frame.render_widget(Paragraph::new(header_row(&layout)), header_area);

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
            QueueRow::Ticket(row) => ticket_line(row, &layout, selected, palette),
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), rows_area);
}

/// `── <tier> ` in the tier's colour, padded to the row width and bolded: the
/// queue's state signal, so the rule takes the tier colour (unlike the PR
/// list's accent-coloured Smart-status headers).
fn tier_header_line(tier: QueueTier, width: usize, palette: &Palette) -> Line<'static> {
    let text = format!("{}── {} ", " ".repeat(INDICATOR_COL + GAP), tier.label());
    Line::from(Span::styled(
        pad_to_width(&text, width),
        Style::default()
            .fg(tier_color(tier, palette))
            .add_modifier(Modifier::BOLD),
    ))
}

/// The tier's role colour, echoed by the markers `title_cell` paints.
fn tier_color(tier: QueueTier, palette: &Palette) -> Color {
    match tier {
        QueueTier::Frontier => palette.frontier,
        QueueTier::Claimed => palette.claimed,
        QueueTier::Blocked => palette.blocked,
    }
}

fn header_row(layout: &QueueLayout) -> Line<'static> {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    render_cells(
        vec![
            Cell::text("", INDICATOR_COL, Style::default()),
            Cell::text("Ticket", layout.ref_col, bold),
            Cell::text("Repo", layout.repo_col, bold),
            Cell::text("Type", layout.type_col, bold),
            Cell::text("Title", layout.title_col(), bold),
            Cell::text("Block", BLOCK_COL, bold),
            Cell::text("Age", AGE_COL, bold),
        ],
        None,
    )
}

fn ticket_line(
    row: &TicketRow,
    layout: &QueueLayout,
    selected: bool,
    palette: &Palette,
) -> Line<'static> {
    // The Selected Row brightens only the title; every other cell keeps its
    // semantic foreground over the band `render_cells` lays down (ADR 0005).
    let title_style = if selected {
        Style::default().fg(palette.selected_fg)
    } else {
        Style::default()
    };
    let cells = vec![
        Cell::text("", INDICATOR_COL, Style::default()),
        Cell::text(
            truncate_middle(&row.display_ref, layout.ref_col),
            layout.ref_col,
            Style::default()
                .fg(palette.count)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::text(
            truncate_middle(&row.repo, layout.repo_col),
            layout.repo_col,
            Style::default().fg(repo_color(&row.repo)),
        ),
        Cell::text(
            row.ty.0.clone(),
            layout.type_col,
            Style::default().fg(palette.mode(row.ty.mode())),
        ),
        title_cell(
            &row.title,
            row.marker.as_ref(),
            layout.title_col(),
            title_style,
            palette,
        ),
        block_cell(row.upstream, row.downstream, palette),
        Cell::text("", AGE_COL, Style::default()),
    ];
    render_cells(cells, selected.then_some(palette.selected_bg))
}

/// The title plus its state marker — `⟨claimed X⟩`, `⟨after Y⟩`, or
/// `⟨dep? Z⟩` — with the title truncated first so the marker survives.
fn title_cell(
    title: &str,
    marker: Option<&RowMarker>,
    width: usize,
    title_style: Style,
    palette: &Palette,
) -> Cell {
    let Some(marker) = marker else {
        return Cell::text(title.to_owned(), width, title_style);
    };
    let (marker, color) = match marker {
        RowMarker::Claimed(Some(who)) => (format!("⟨claimed {who}⟩"), palette.claimed),
        RowMarker::Claimed(None) => ("⟨claimed⟩".to_owned(), palette.claimed),
        RowMarker::After(target) => (format!("⟨after {target}⟩"), palette.blocked),
        RowMarker::UnknownDependency(raw) => (format!("⟨dep? {raw}⟩"), palette.blocked),
    };
    // The marker is the row's state signal, so it takes the width first and
    // the title gets the rest — none at all when the marker alone fills the
    // cell, where `render_cells` truncates the marker rather than lose it.
    let title_budget = width.saturating_sub(marker.width() + 1);
    let marker = Span::styled(marker, Style::default().fg(color));
    let spans = if title_budget == 0 {
        vec![marker]
    } else {
        vec![
            Span::styled(truncate(title, title_budget), title_style),
            Span::raw(" "),
            marker,
        ]
    };
    Cell { spans, width }
}

/// `↑N` open upstream Dependencies (red) and `↓N` open downstream dependents
/// (blue), either omitted when zero.
fn block_cell(upstream: usize, downstream: usize, palette: &Palette) -> Cell {
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
    Cell {
        spans,
        width: BLOCK_COL,
    }
}

fn render_status(model: &Model, frame: &mut Frame<'_>, area: Rect, palette: &Palette) {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let left = Line::from(vec![
        Span::styled("j/k", bold),
        Span::raw(" nav  "),
        Span::styled("t", bold),
        Span::raw(" PRs  "),
        Span::styled("q", bold),
        Span::raw(" quit"),
    ]);
    frame.render_widget(Paragraph::new(left), area);
    super::render_status_right(model, frame, area, palette);
}
