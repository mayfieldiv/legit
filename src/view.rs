use chrono::{DateTime, Utc};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::grouping::Grouping;
use crate::app::list_layout;
use crate::app::model::{Model, StatusKind, ViewMode};
use crate::format::{abbreviate_home, truncate_middle};
use crate::git_remote::RepoInfo;
use crate::palette::{DARK, Palette};

pub mod detail;
pub mod list;
pub mod summary;

pub(crate) const WORKTREE_GLYPH: &str = "\u{e725}";

/// Per-row indicator for a PR whose `r`/`R` refresh is in flight. Shares the
/// leading one-column glyph slot with `WORKTREE_GLYPH`, taking precedence while
/// a refresh is in flight so the activity is visible.
pub(crate) const REFRESH_GLYPH: &str = "\u{21bb}";

pub(crate) fn worktree_line(path: &str, max_path_width: usize, palette: &Palette) -> Line<'static> {
    Line::from(vec![
        Span::styled(WORKTREE_GLYPH, Style::default().fg(palette.accent)),
        Span::styled(" worktree: ", Style::default().fg(palette.muted)),
        Span::raw(truncate_middle(
            &abbreviate_home(path),
            max_path_width.max(1),
        )),
    ])
}

/// Short label for the active grouping mode, shown in the status-bar `g` hint.
fn grouping_label(model: &Model) -> &'static str {
    match model.list.grouping() {
        Grouping::SmartStatus => "smart-status",
        Grouping::Repo => "repo",
        Grouping::None => "none",
    }
}

pub fn view(model: &Model, frame: &mut Frame<'_>, now: DateTime<Utc>) {
    // The one curated palette (ADR 0005): the shared `DARK` instance, threaded
    // through every render call. The format/markdown/detail-layout helpers read
    // `DARK` directly, so sourcing the view layer from it too keeps a single
    // seam — swap `DARK` and the whole app follows.
    let palette = &*DARK;
    let area = frame.area();

    // Detail view takes the whole frame and manages its own chrome (header + status bar).
    if let ViewMode::Detail(detail) = &model.view_mode {
        detail::render(model, detail, frame, area, now, palette);
        return;
    }

    // ── List view ────────────────────────────────────────────────────────────
    // Fixed layout: app header, tab bar, filter chip, list, status bar. The
    // chip collapses to zero height while the filter is inactive, giving its
    // row back to the list — which keeps the row count in step with
    // `Model::chrome_rows`, the shared definition `sync_viewport` derives the
    // viewport height from.
    let filter_visible = model.list.filter().is_visible();
    let [header, tabs, chip, main, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(u16::from(filter_visible)),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);

    render_app_header(model, frame, header, palette);
    render_tabs(model, frame, tabs, palette);
    if filter_visible {
        render_filter_chip(model, frame, chip, palette);
    }
    // Split the main region into the list and the summary panel when the
    // terminal is wide enough; below 80 columns the list takes the whole row.
    // The widths come from `list_layout`, the same geometry mouse hit-testing
    // maps clicks against.
    match list_layout::panel_width(main.width) {
        Some(panel_width) => {
            let [list_area, divider_area, summary_area] = Layout::horizontal([
                Constraint::Min(1),
                Constraint::Length(list_layout::DIVIDER_WIDTH),
                Constraint::Length(panel_width),
            ])
            .areas(main);
            list::render(model, frame, list_area, now, palette);
            render_summary_divider(frame, divider_area, palette);
            summary::render(model, frame, summary_area, now, palette);
        }
        None => list::render(model, frame, main, now, palette),
    }
    render_status(model, frame, status, palette);
}

fn render_app_header(model: &Model, frame: &mut Frame<'_>, area: Rect, palette: &Palette) {
    let scope = model
        .active_scope()
        .unwrap_or_else(|| "All repos".to_owned());
    let count = model
        .list
        .prs()
        .iter()
        .filter(|pr| scope == "All repos" || pr.repo_slug == scope)
        .count();
    let line = Line::from(vec![
        Span::styled(
            "legit",
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" — "),
        Span::styled(scope, Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(format!(" — {count} open PRs")),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_summary_divider(frame: &mut Frame<'_>, area: Rect, palette: &Palette) {
    let style = Style::default().fg(palette.separator);
    let lines = (0..area.height)
        .map(|_| Line::from(Span::styled("│", style)))
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), area);
}

/// The filter chip above the list: `/text` plus a block cursor while editing;
/// just the accented text once applied, so it reads as a sticky chip.
fn render_filter_chip(model: &Model, frame: &mut Frame<'_>, area: Rect, palette: &Palette) {
    let filter = model.list.filter();
    let mut spans = vec![
        Span::styled(
            "/",
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(filter.text().to_owned()),
    ];
    if filter.is_editing() {
        spans.push(Span::styled("█", Style::default().fg(palette.accent)));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The Repo Tab bar: `All` plus one tab per Tracked Repo, the active tab
/// bracketed and accented (`[All]  acme/web `), matching the TS tab bar.
fn render_tabs(model: &Model, frame: &mut Frame<'_>, area: Rect, palette: &Palette) {
    let repos = model.tracked_repos();
    // Highlight the active tab, falling back to the All tab (0) when `active_tab`
    // is out of range — the same All-fallback policy `active_scope` applies to the
    // list filter, mirrored here from the repos already in hand so the bar never
    // rebuilds `tracked_repos`. Tab `i >= 1` maps to `repos[i - 1]`, so the
    // highest in-range index is `repos.len()`.
    let active = if model.active_tab <= repos.len() {
        model.active_tab
    } else {
        0
    };
    let labels = std::iter::once("All".to_owned()).chain(repos.iter().map(RepoInfo::slug));
    let mut spans = Vec::new();
    for (i, label) in labels.enumerate() {
        let (text, style) = if i == active {
            (
                format!("[{label}]"),
                Style::default()
                    .fg(palette.accent)
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

fn render_status(model: &Model, frame: &mut Frame<'_>, area: Rect, palette: &Palette) {
    // Left: key hints — the filter editor's own hints while it is open, the
    // normal-mode hints otherwise. The network indicator is always rendered on
    // the right so the app's activity signal does not disappear when idle.
    let mut left = Vec::new();
    let bold = Style::default().add_modifier(Modifier::BOLD);
    if model.list.filter().is_editing() {
        left.push(Span::styled("enter", bold));
        left.push(Span::raw(" apply  "));
        left.push(Span::styled("esc", bold));
        left.push(Span::raw(" clear"));
    } else {
        left.push(Span::styled("j/k", bold));
        left.push(Span::raw(" nav  "));
        left.push(Span::styled("↵", bold));
        left.push(Span::raw(" open  "));
        left.push(Span::styled("q", bold));
        left.push(Span::raw(" quit  "));
        left.push(Span::styled("g", bold));
        left.push(Span::raw(format!(" group: {}", grouping_label(model))));
        left.push(Span::raw("  "));
        left.push(Span::styled("h/l", bold));
        left.push(Span::raw(" tabs  "));
        left.push(Span::styled("/", bold));
        left.push(Span::raw(" filter  "));
        left.push(Span::styled("w", bold));
        left.push(Span::raw(" worktree  "));
        left.push(Span::styled("r/R", bold));
        left.push(Span::raw(" refresh"));
    }
    frame.render_widget(Paragraph::new(Line::from(left)), area);
    let network_width = network_indicator_width(model, area.width);
    let overlay_width = area.width.saturating_sub(network_width.saturating_add(1));
    render_status_overlay(
        model,
        frame,
        Rect {
            width: overlay_width,
            ..area
        },
        palette,
    );
    render_network_indicator(model, frame, area, network_width, palette);
}

fn network_indicator_label(model: &Model) -> String {
    let stats = model.network_stats;
    format!("{} in-flight · {} waiting", stats.in_flight, stats.waiting)
}

fn network_indicator_width(model: &Model, max_width: u16) -> u16 {
    (network_indicator_label(model).chars().count() as u16).min(max_width)
}

fn render_network_indicator(
    model: &Model,
    frame: &mut Frame<'_>,
    area: Rect,
    width: u16,
    palette: &Palette,
) {
    let stats = model.network_stats;
    let label = network_indicator_label(model);
    let x = area.x + area.width.saturating_sub(width);
    let color = if stats.in_flight > 0 || stats.waiting > 0 {
        palette.accent
    } else {
        palette.muted
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(label, Style::default().fg(color)))),
        Rect {
            x,
            y: area.y,
            width,
            height: area.height,
        },
    );
}

/// The right-aligned overlay every status bar shares (list and detail): an
/// app-level fatal (a malformed config) wins, then a hard list-load failure,
/// then the transient status message (info / success / error). Painted
/// right-aligned over the same row so it sits opposite that bar's key hints —
/// one renderer, so a `CommandFailed` raised in any view is visible there
/// before its scheduled clear wipes it.
pub(crate) fn render_status_overlay(
    model: &Model,
    frame: &mut Frame<'_>,
    area: Rect,
    palette: &Palette,
) {
    if let Some(failure) = model.fatal.as_deref().or_else(|| model.list.failure()) {
        let line = Line::from(vec![
            Span::styled("error: ", Style::default().fg(palette.error)),
            Span::styled(failure.to_owned(), Style::default().fg(palette.warning)),
        ]);
        frame.render_widget(Paragraph::new(line).alignment(Alignment::Right), area);
    } else if let Some(status) = &model.status {
        let color = match status.kind {
            StatusKind::Info => palette.muted,
            StatusKind::Success => palette.passing,
            StatusKind::Error => palette.error,
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
