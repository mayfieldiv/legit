//! The ticket surface: the effort rail beside the tier-grouped ticket queue,
//! under its own app header and above the shared status bar (spec §6.1–§6.2,
//! the prototype's "Efforts pane" layout).

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

use super::row::{Cell, GAP, render_cells};
use crate::{
    app::{
        model::Model,
        ticket_list::{EffortEntry, QueueRow, QueueTier, TicketList},
        ticket_list_layout::{DIVIDER_WIDTH, rail_width},
    },
    color::repo_color,
    format::{format_repo_short, pad_to_width, truncate, truncate_middle},
    palette::Palette,
    ticket::{Claim, EffortSource, EffortTicket, TicketState},
};

#[cfg(test)]
mod tests;

/// Below this many columns for the queue the rail is dropped so the queue
/// keeps a usable width — the floor only.
// TODO(#133): the narrow-width collapse proper (spec §6.4).
const MIN_QUEUE_WIDTH: u16 = 40;

pub fn render(model: &Model, frame: &mut Frame<'_>, area: Rect, palette: &Palette) {
    let [header, main, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);
    render_header(model, frame, header, palette);
    let tickets = &model.tickets;
    let rail_width = rail_width(main.width);
    if tickets.efforts().is_empty() && tickets.discovery_failures().next().is_none() {
        let text = if tickets.is_loading() {
            "Loading efforts…"
        } else {
            "No efforts found"
        };
        frame.render_widget(
            Paragraph::new(Line::from(text)).alignment(Alignment::Center),
            main,
        );
    } else if main.width >= rail_width + DIVIDER_WIDTH + MIN_QUEUE_WIDTH {
        let [rail, divider, queue] = Layout::horizontal([
            Constraint::Length(rail_width),
            Constraint::Length(DIVIDER_WIDTH),
            Constraint::Min(1),
        ])
        .areas(main);
        render_rail(tickets, frame, rail, palette);
        render_divider(frame, divider, palette);
        render_queue(tickets, frame, queue, palette);
    } else {
        render_queue(tickets, frame, main, palette);
    }
    render_status(model, frame, status, palette);
}

fn render_header(model: &Model, frame: &mut Frame<'_>, area: Rect, palette: &Palette) {
    let efforts = model.tickets.efforts();
    let frontier: usize = efforts.iter().map(|entry| entry.counts().frontier).sum();
    let noun = if efforts.len() == 1 {
        "effort"
    } else {
        "efforts"
    };
    let bold = |color| Style::default().fg(color).add_modifier(Modifier::BOLD);
    let line = Line::from(vec![
        Span::styled("legit", bold(palette.accent)),
        Span::raw(" — "),
        Span::styled("Tickets", bold(palette.accent)),
        Span::raw(format!(" — {} {noun} · {frontier} frontier", efforts.len())),
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
/// is always the active one), then one three-line card per Effort and one
/// two-line card per failed discovery unit, each followed by a blank row.
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
    for entry in tickets.efforts() {
        lines.extend(effort_card(entry, width, palette));
        lines.push(Line::default());
    }
    for (name, error) in tickets.discovery_failures() {
        lines.extend(discovery_failure_card(name, error, width, palette));
        lines.push(Line::default());
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn effort_card(entry: &EffortEntry, width: usize, palette: &Palette) -> Vec<Line<'static>> {
    let repo = entry.repo.display_name();
    let source = match entry.source() {
        EffortSource::GitHub => "github",
        EffortSource::Local => "local",
    };
    let muted = Style::default().fg(palette.muted);
    let title = Span::styled(entry.title(), Style::default().add_modifier(Modifier::BOLD));
    let mut lines = vec![repo_led_line(&repo, title, width, palette)];
    match entry.error() {
        None => {
            let counts = entry.counts();
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
                truncate(entry.destination().unwrap_or(""), width),
                muted,
            )));
        }
        Some(reason) => {
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

/// `repo · <tail>`: the repo's short name in its Repo Color and bold, a
/// separator, then `tail` truncated to what remains — so a long tail never
/// pushes the repo off the card.
fn repo_led_line(
    repo: &str,
    tail: Span<'static>,
    width: usize,
    palette: &Palette,
) -> Line<'static> {
    let short = format_repo_short(repo);
    let tail_width = width.saturating_sub(short.width() + 3);
    Line::from(vec![
        Span::styled(
            short.to_owned(),
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
const REF_COL_MIN: usize = 6;
const REF_COL_MAX: usize = 64;
const REPO_COL_MIN: usize = 4;
const REPO_COL_MAX: usize = 32;
const TYPE_COL_MIN: usize = 4;
const TYPE_COL_MAX: usize = 24;
const TITLE_COL_MIN: usize = 40;
/// `↑NN ↓NN`.
const BLOCK_COL: usize = 7;
/// Sized like the PR list's Updated column so the header lands where the data
/// will; the cells stay empty until Fetch Age is stamped.
// TODO(#132): render Fetch Age.
const AGE_COL: usize = 7;

struct QueueLayout {
    width: usize,
    ref_col: usize,
    repo_col: usize,
    type_col: usize,
}

impl QueueLayout {
    fn new(width: usize, tickets: &TicketList) -> Self {
        let mut ref_width = REF_COL_MIN;
        let mut repo_width = REPO_COL_MIN;
        let mut type_width = TYPE_COL_MIN;
        for entry in tickets.efforts() {
            let Some(effort) = entry.effort() else {
                continue;
            };
            for ticket in effort
                .tickets()
                .filter(|ticket| ticket.state == TicketState::Open)
            {
                ref_width = ref_width.max(ticket.key.display_ref().width());
                repo_width = repo_width.max(format_repo_short(&entry.repo.display_name()).width());
                type_width = type_width.max(ticket.ty.0.width());
            }
        }
        let mut layout = Self {
            width,
            ref_col: ref_width.min(14),
            repo_col: repo_width.min(14),
            type_col: type_width.min(12),
        };
        let mut spare = layout.title_col().saturating_sub(TITLE_COL_MIN);
        for (column, desired) in [
            (&mut layout.ref_col, ref_width.min(REF_COL_MAX)),
            (&mut layout.repo_col, repo_width.min(REPO_COL_MAX)),
            (&mut layout.type_col, type_width.min(TYPE_COL_MAX)),
        ] {
            let extra = desired.saturating_sub(*column).min(spare);
            *column += extra;
            spare -= extra;
        }
        layout
    }

    /// Whatever the fixed columns and their gaps leave for the title.
    fn title_col(&self) -> usize {
        let fixed_cells = [
            INDICATOR_COL,
            self.ref_col,
            self.repo_col,
            self.type_col,
            BLOCK_COL,
            AGE_COL,
        ];
        let fixed = fixed_cells.iter().sum::<usize>() + fixed_cells.len() * GAP;
        self.width.saturating_sub(fixed).max(1)
    }
}

fn render_queue(tickets: &TicketList, frame: &mut Frame<'_>, area: Rect, palette: &Palette) {
    let [header_area, rows_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
    let width = usize::from(area.width);
    let layout = QueueLayout::new(width, tickets);
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
        .filter_map(|(row, selected)| match row {
            QueueRow::Header(tier) => Some(tier_header_line(*tier, width, palette)),
            QueueRow::Ticket(key) => tickets
                .ticket(key)
                .map(|(entry, ticket)| ticket_line(entry, &ticket, &layout, selected, palette)),
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
            .fg(palette.queue_tier(tier))
            .add_modifier(Modifier::BOLD),
    ))
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
    entry: &EffortEntry,
    ticket: &EffortTicket<'_>,
    layout: &QueueLayout,
    selected: bool,
    palette: &Palette,
) -> Line<'static> {
    let repo = entry.repo.display_name();
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
            truncate_middle(&ticket.key.display_ref(), layout.ref_col),
            layout.ref_col,
            Style::default()
                .fg(palette.count)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::text(
            truncate_middle(format_repo_short(&repo), layout.repo_col),
            layout.repo_col,
            Style::default().fg(repo_color(&repo)),
        ),
        Cell::text(
            ticket.ty.0.clone(),
            layout.type_col,
            Style::default().fg(palette.mode(ticket.ty.mode())),
        ),
        title_cell(ticket, layout.title_col(), title_style, palette),
        block_cell(ticket, palette),
        Cell::text("", AGE_COL, Style::default()),
    ];
    render_cells(cells, selected.then_some(palette.selected_bg))
}

/// The title plus its state marker — `⟨claimed X⟩`, `⟨after Y⟩`, or
/// `⟨dep? Z⟩` — with the title truncated first so the marker survives.
fn title_cell(
    ticket: &EffortTicket<'_>,
    width: usize,
    title_style: Style,
    palette: &Palette,
) -> Cell {
    let marker = match QueueTier::of(ticket) {
        QueueTier::Frontier => None,
        QueueTier::Claimed => Some((
            match &ticket.claim {
                Some(Claim::By(who)) => format!("⟨claimed {who}⟩"),
                Some(Claim::Anonymous) | None => "⟨claimed⟩".to_owned(),
            },
            palette.claimed,
        )),
        QueueTier::Blocked => ticket
            .unknown_dependency_ref()
            .map(|raw| format!("⟨dep? {raw}⟩"))
            .or_else(|| {
                ticket
                    .open_dependencies()
                    .first()
                    .map(|key| format!("⟨after {}⟩", key.display_ref()))
            })
            .map(|text| (text, palette.blocked)),
    };
    let Some((marker, color)) = marker else {
        return Cell::text(ticket.title.clone(), width, title_style);
    };
    // Below the room for a marker plus one title glyph, `render_cells`' own
    // fitting decides what survives.
    let title = if width > marker.width() + 2 {
        truncate(&ticket.title, width - marker.width() - 1)
    } else {
        ticket.title.clone()
    };
    Cell {
        spans: vec![
            Span::styled(title, title_style),
            Span::raw(" "),
            Span::styled(marker, Style::default().fg(color)),
        ],
        width,
    }
}

/// `↑N` open upstream Dependencies (red) and `↓N` open downstream dependents
/// (blue), either omitted when zero.
fn block_cell(ticket: &EffortTicket<'_>, palette: &Palette) -> Cell {
    let upstream = ticket.open_dependencies().len();
    let downstream = ticket.blocks().len();
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
