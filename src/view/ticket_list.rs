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

use crate::{
    app::{
        model::Model,
        ticket_list::{EffortEntry, QueueRow, QueueTier, TicketList},
        ticket_list_layout::{DIVIDER_WIDTH, RAIL_WIDTH},
    },
    color::repo_color,
    format::{format_repo_short, pad_to_width, truncate, truncate_middle},
    palette::Palette,
    ticket::{Claim, EffortSource, EffortTicket},
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
    } else if main.width >= RAIL_WIDTH + DIVIDER_WIDTH + MIN_QUEUE_WIDTH {
        let [rail, divider, queue] = Layout::horizontal([
            Constraint::Length(RAIL_WIDTH),
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
/// two-line card per failed probe, each followed by a blank row.
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
        lines.extend(probe_failure_card(name, error, width, palette));
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
    let mut lines = vec![card_title_line(&repo, &entry.title(), width, palette)];
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
fn probe_failure_card(
    name: &str,
    error: &str,
    width: usize,
    palette: &Palette,
) -> Vec<Line<'static>> {
    let short = format_repo_short(name);
    vec![
        Line::from(vec![
            Span::styled(
                short.to_owned(),
                Style::default()
                    .fg(repo_color(name))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" · ", Style::default().fg(palette.separator)),
            Span::styled(
                truncate("couldn't probe", width.saturating_sub(short.width() + 3)),
                Style::default().fg(palette.error),
            ),
        ]),
        Line::from(Span::styled(
            truncate(error, width),
            Style::default().fg(palette.warning),
        )),
    ]
}

/// `repo · title`, the repo in its Repo Color and the title bold, truncated as
/// one string so a long title never pushes the repo off the card.
fn card_title_line(repo: &str, title: &str, width: usize, palette: &Palette) -> Line<'static> {
    let short = format_repo_short(repo);
    let repo_span = Span::styled(
        short.to_owned(),
        Style::default()
            .fg(repo_color(repo))
            .add_modifier(Modifier::BOLD),
    );
    let separator = Span::styled(" · ", Style::default().fg(palette.separator));
    let title_width = width.saturating_sub(short.width() + 3);
    let title_span = Span::styled(
        truncate(title, title_width),
        Style::default().add_modifier(Modifier::BOLD),
    );
    Line::from(vec![repo_span, separator, title_span])
}

// ── queue ────────────────────────────────────────────────────────────────────

/// One column's gap, and the one-column left pad every queue row starts with.
const GAP: usize = 1;
const LEFT_PAD: usize = 1;
const REF_COL_MIN: usize = 6;
/// Refs cap at 14 columns with a middle ellipsis (spec §6.2).
const REF_COL_MAX: usize = 14;
const REPO_COL: usize = 14;
const TYPE_COL_MIN: usize = 4;
const TYPE_COL_MAX: usize = 12;
/// `↑NN ↓NN`.
const BLOCK_COL: usize = 7;
/// Empty until the refresh slice stamps Fetch Age; sized like the PR list's
/// Updated column so the header lands where the data will.
const AGE_COL: usize = 7;

/// Per-render column sizing derived from the visible Tickets.
struct QueueLayout {
    width: usize,
    ref_col: usize,
    type_col: usize,
}

impl QueueLayout {
    fn new(width: usize, visible: &[EffortTicket<'_>]) -> Self {
        let widest =
            |f: &dyn Fn(&EffortTicket<'_>) -> usize| visible.iter().map(f).max().unwrap_or(0);
        Self {
            width,
            ref_col: widest(&|t| t.key.display_ref().width()).clamp(REF_COL_MIN, REF_COL_MAX),
            type_col: widest(&|t| t.ty.0.width()).clamp(TYPE_COL_MIN, TYPE_COL_MAX),
        }
    }

    /// Whatever the fixed columns and their gaps leave for the title.
    fn title_col(&self) -> usize {
        let fixed = LEFT_PAD
            + self.ref_col
            + GAP
            + REPO_COL
            + GAP
            + self.type_col
            + GAP
            + GAP
            + BLOCK_COL
            + GAP
            + AGE_COL;
        self.width.saturating_sub(fixed).max(1)
    }
}

fn render_queue(tickets: &TicketList, frame: &mut Frame<'_>, area: Rect, palette: &Palette) {
    let [header_area, rows_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
    let width = usize::from(area.width);
    let visible: Vec<EffortTicket<'_>> = tickets
        .visible_rows()
        .filter_map(|(row, _)| match row {
            QueueRow::Ticket(key) => tickets.ticket(key).map(|(_, ticket)| ticket),
            QueueRow::Header(_) => None,
        })
        .collect();
    let layout = QueueLayout::new(width, &visible);
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
    Line::from(Span::styled(
        pad_to_width(
            &format!("{}── {} ", " ".repeat(LEFT_PAD), tier.label()),
            width,
        ),
        Style::default()
            .fg(palette.queue_tier(tier))
            .add_modifier(Modifier::BOLD),
    ))
}

/// One fixed-width cell: its spans, fitted to `width` (truncated with an
/// ellipsis when they overflow, space-padded when they don't).
struct Cell {
    spans: Vec<Span<'static>>,
    width: usize,
}

impl Cell {
    fn text(text: impl Into<String>, width: usize, style: Style) -> Self {
        Self {
            spans: vec![Span::styled(text.into(), style)],
            width,
        }
    }
}

fn header_row(layout: &QueueLayout) -> Line<'static> {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    render_cells(
        vec![
            Cell::text("Ticket", layout.ref_col, bold),
            Cell::text("Repo", REPO_COL, bold),
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
    let title_style = if selected {
        Style::default().fg(palette.selected_fg)
    } else {
        Style::default()
    };
    let cells = vec![
        Cell::text(
            truncate_middle(&ticket.key.display_ref(), layout.ref_col),
            layout.ref_col,
            Style::default()
                .fg(palette.count)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::text(
            truncate_middle(format_repo_short(&repo), REPO_COL),
            REPO_COL,
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
    let mut spans = Vec::new();
    match marker {
        Some((text, color)) if width > text.width() + 2 => {
            spans.push(Span::styled(
                truncate(&ticket.title, width - text.width() - 1),
                title_style,
            ));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(text, Style::default().fg(color)));
        }
        Some((text, color)) => {
            spans.push(Span::styled(ticket.title.clone(), title_style));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(text, Style::default().fg(color)));
        }
        None => spans.push(Span::styled(ticket.title.clone(), title_style)),
    }
    Cell { spans, width }
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

/// Lay cells out left to right with the left pad and one-column gaps. Each
/// cell's spans are fitted to its width: spans past the budget are dropped
/// and the one straddling it is truncated with an ellipsis; a short cell is
/// space-padded. A `fill` paints the Selected Row's band under the whole
/// line while every span keeps its own foreground (ADR 0005).
fn render_cells(cells: Vec<Cell>, fill: Option<Color>) -> Line<'static> {
    let mut spans = vec![Span::raw(" ".repeat(LEFT_PAD))];
    for (i, cell) in cells.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" ".repeat(GAP)));
        }
        let mut used = 0;
        for span in cell.spans {
            let remaining = cell.width.saturating_sub(used);
            if remaining == 0 {
                break;
            }
            let text = truncate(&span.content, remaining);
            used += text.width();
            spans.push(Span::styled(text, span.style));
        }
        if used < cell.width {
            spans.push(Span::raw(" ".repeat(cell.width - used)));
        }
    }
    let line = Line::from(spans);
    match fill {
        Some(color) => line.style(Style::default().bg(color)),
        None => line,
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
