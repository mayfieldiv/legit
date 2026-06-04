//! The right-side summary panel for the selected PR. Renders, top to bottom:
//! smart-status reason (coloured by tier) -> mergeable state -> reviews summary
//! -> threads summary -> CI checks summary -> file-category size breakdown ->
//! worktree path placeholder -> footer GitHub URL. Sections whose enrichment
//! hasn't arrived render a "Loading…" placeholder so the panel fills in
//! reactively as the per-PR fan-out lands.
//!
//! Panel width is a function of the terminal width: hidden below 80 columns,
//! 36 columns at 80-139, 50 columns at >=140. `panel_width` is the single
//! source of truth shared by `view::view` (which splits the main area) and the
//! tests.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::model::{FilesState, Model};
use crate::blocker::Tier;
use crate::format::{
    CheckOutcome, check_icon, checks_summary, format_review_state, outcome, review_icon,
    sort_check_runs,
};
use crate::github::types::CheckRun;

/// Placeholder text for a section whose enrichment hasn't arrived yet.
const LOADING: &str = "Loading…";

/// Max number of individual check rows before collapsing the rest into a
/// `+N more` line. Mirrors the TS `MAX_VISIBLE_CHECKS`.
const MAX_VISIBLE_CHECKS: usize = 6;

#[cfg(test)]
mod tests;

/// Below this width the summary panel is hidden entirely — the list takes the
/// whole row.
const MIN_WIDTH_FOR_PANEL: u16 = 80;
/// At this width and above the panel widens from 36 to 50 columns.
const WIDE_WIDTH: u16 = 140;
/// Panel width in the narrow band (80-139 columns).
const NARROW_PANEL_WIDTH: u16 = 36;
/// Panel width at >=140 columns.
const WIDE_PANEL_WIDTH: u16 = 50;

/// The summary panel's width for a given terminal width, or `None` when the
/// terminal is too narrow to show it (the list then takes the whole row).
pub fn panel_width(total_cols: u16) -> Option<u16> {
    if total_cols < MIN_WIDTH_FOR_PANEL {
        None
    } else if total_cols < WIDE_WIDTH {
        Some(NARROW_PANEL_WIDTH)
    } else {
        Some(WIDE_PANEL_WIDTH)
    }
}

/// Render the summary panel into `area`. Assumes `area` is the panel's region
/// (already split off the list by the caller).
pub fn render(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let Some(pr) = model.list.selected_pr() else {
        let line = Line::from(Span::styled(
            "No PR selected",
            Style::default().fg(Color::Gray),
        ));
        frame.render_widget(Paragraph::new(line), area);
        return;
    };

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(smart_status_line(model, pr));
    lines.push(mergeable_line(pr));
    lines.extend(reviews_lines(model, pr));
    lines.push(threads_line(model, pr));
    lines.extend(checks_lines(model, pr));
    lines.extend(files_lines(model, pr));
    lines.push(worktree_line(pr));
    lines.push(url_footer_line(pr));

    frame.render_widget(Paragraph::new(lines), area);
}

/// The smart-status reason line, coloured by tier (me-blocking magenta,
/// waiting-on-author yellow, needs-review gray). `Loading…` until the PR's
/// blocker has been derived (both threads and reviews arrived).
fn smart_status_line(model: &Model, pr: &crate::github::rest::PR) -> Line<'static> {
    match model.blockers.get(&pr.key()) {
        Some(result) => Line::from(Span::styled(
            result.reason.clone(),
            Style::default().fg(tier_color(result.tier)),
        )),
        None => loading_line(),
    }
}

/// The mergeable-state line. Mirrors the TS `formatMergeable`: `CONFLICTING` ->
/// "! conflict" (red), `MERGEABLE` -> "✓ mergeable" (green), anything else
/// (including `UNKNOWN`) -> "? merge unknown" (gray).
fn mergeable_line(pr: &crate::github::rest::PR) -> Line<'static> {
    let (text, color) = match pr.mergeable.as_str() {
        "CONFLICTING" => ("! conflict", Color::Red),
        "MERGEABLE" => ("✓ mergeable", Color::Green),
        _ => ("? merge unknown", Color::Gray),
    };
    Line::from(Span::styled(text, Style::default().fg(color)))
}

/// The reviews section: a `reviews` header with approved / changes-requested /
/// commented counts, then one indented row per reviewer with an icon and their
/// state. `Loading…` beside the header until the reviews fetch arrives (`None`
/// = not loaded, distinct from `Some(&[])` = loaded, no reviews).
fn reviews_lines(model: &Model, pr: &crate::github::rest::PR) -> Vec<Line<'static>> {
    let Some(reviews) = model.enrichment.reviews.get(&pr.key()) else {
        return vec![header_with_loading("reviews")];
    };

    let approved = reviews.iter().filter(|r| r.state == "APPROVED").count();
    let changes = reviews
        .iter()
        .filter(|r| r.state == "CHANGES_REQUESTED")
        .count();
    let commented = reviews.iter().filter(|r| r.state == "COMMENTED").count();

    let mut counts: Vec<Span<'static>> = vec![section_header("reviews")];
    counts.push(Span::raw(" "));
    counts.push(Span::raw(format!(
        "{approved} approved, {changes} changes requested, {commented} commented"
    )));
    let mut lines = vec![Line::from(counts)];

    for review in reviews {
        let (icon, color) = review_icon(&review.state);
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(icon, Style::default().fg(color)),
            Span::raw(format!(" {} ", review.user)),
            Span::styled(
                format_review_state(&review.state),
                Style::default().fg(Color::Gray),
            ),
        ]));
    }
    lines
}

/// The threads summary line: `threads N total, M unresolved (H human, B bot)`.
/// `Loading…` until the review-threads fetch arrives. Bot classification mirrors
/// the TS `computeCommentCounts`: a thread is unresolved-bot when its first
/// comment is a bot (the fetch-time `is_bot` flag) or its author is a configured
/// bot login.
fn threads_line(model: &Model, pr: &crate::github::rest::PR) -> Line<'static> {
    let Some(threads) = model.enrichment.review_threads.get(&pr.key()) else {
        return header_with_loading("threads");
    };

    let bot_logins = &model.config.bot_logins;
    let total = threads.len();
    let mut unresolved = 0;
    let mut human = 0;
    let mut bot = 0;
    for thread in threads {
        if thread.is_resolved {
            continue;
        }
        unresolved += 1;
        let is_bot = thread
            .comments
            .first()
            .is_some_and(|c| c.is_bot || bot_logins.iter().any(|b| b == &c.author));
        if is_bot {
            bot += 1;
        } else {
            human += 1;
        }
    }

    Line::from(vec![
        section_header("threads"),
        Span::raw(format!(
            " {total} total, {unresolved} unresolved ({human} human, {bot} bot)"
        )),
    ])
}

/// The CI checks section: a `checks` header with failed / pending / passed
/// counts, then one indented row per non-passing check (passing checks are
/// summarised by the count alone). `Loading…` until the checks fetch arrives —
/// which can't start until review-status reports the PR's head SHA, so a PR
/// with no head SHA also reads as loading.
fn checks_lines(model: &Model, pr: &crate::github::rest::PR) -> Vec<Line<'static>> {
    let checks = pr.head_commit_sha.as_ref().and_then(|sha| {
        model
            .enrichment
            .checks
            .get(&(pr.repo_slug.clone(), sha.clone()))
    });
    let Some(checks) = checks else {
        return vec![header_with_loading("checks")];
    };

    let summary = checks_summary(checks);

    let mut header: Vec<Span<'static>> = vec![section_header("checks"), Span::raw(" ")];
    if summary.failed > 0 {
        header.push(Span::styled(
            format!("{} failed ", summary.failed),
            Style::default().fg(Color::Red),
        ));
    }
    if summary.pending > 0 {
        header.push(Span::styled(
            format!("{} pending ", summary.pending),
            Style::default().fg(Color::Yellow),
        ));
    }
    header.push(Span::styled(
        format!("{}/{} passed", summary.passed, summary.total),
        Style::default().fg(if summary.passed == summary.total {
            Color::Green
        } else {
            Color::Gray
        }),
    ));
    let mut lines = vec![Line::from(header)];

    // Per-check rows for the non-passing checks only, sorted (failing first,
    // then by name) and capped, mirroring the TS `sortCheckRuns` + visible cap.
    // Classifying via `outcome` keeps this filter in lockstep with the header
    // counts above, which `checks_summary` derives from the same predicate.
    let mut non_passing: Vec<&CheckRun> = checks
        .iter()
        .filter(|c| outcome(c) != CheckOutcome::Passed)
        .collect();
    sort_check_runs(&mut non_passing);

    for check in non_passing.iter().take(MAX_VISIBLE_CHECKS) {
        let (icon, color) = check_icon(check);
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(icon, Style::default().fg(color)),
            Span::raw(format!(" {}", check.name)),
        ]));
    }
    let overflow = non_passing.len().saturating_sub(MAX_VISIBLE_CHECKS);
    if overflow > 0 {
        lines.push(Line::from(Span::styled(
            format!("  +{overflow} more"),
            Style::default().fg(Color::Gray),
        )));
    }
    lines
}

/// The File Category breakdown section: a `files` header, then one indented row
/// per non-empty category (`code: +14/-3 (2)`), plus a `total` row. `Loading…`
/// both before the fetch is requested (no entry) and while it's in flight
/// (`Requested`); the breakdown renders once it's `Loaded` and categorised.
fn files_lines(model: &Model, pr: &crate::github::rest::PR) -> Vec<Line<'static>> {
    let categorization = match model.enrichment.files.get(&pr.key()) {
        Some(FilesState::Loaded(categorization)) => categorization,
        None | Some(FilesState::Requested) => return vec![header_with_loading("files")],
    };
    let breakdown = &categorization.breakdown;

    let mut lines = vec![Line::from(section_header("files"))];
    for (category, stats) in breakdown.category_rows() {
        if stats.files == 0 {
            continue;
        }
        lines.push(category_row(
            category.as_str(),
            stats.additions,
            stats.deletions,
            stats.files,
        ));
    }
    // The total row sums every category (or reads 0/0 (0) for an empty diff).
    let total = breakdown.total();
    lines.push(category_row(
        "total",
        total.additions,
        total.deletions,
        total.files,
    ));
    lines
}

/// The worktree path line. Worktree detection lands in #50, so for now this is
/// a blank placeholder line — the section keeps its slot in the layout so the
/// footer URL sits where it will once worktrees arrive.
fn worktree_line(_pr: &crate::github::rest::PR) -> Line<'static> {
    Line::from("")
}

/// The footer line: the PR's full GitHub URL. Mirrors the TS `prUrl`.
fn url_footer_line(pr: &crate::github::rest::PR) -> Line<'static> {
    let url = format!("https://github.com/{}/pull/{}", pr.repo_slug, pr.number);
    Line::from(Span::styled(url, Style::default().fg(Color::Cyan)))
}

/// One indented breakdown row: `  <label>: +A/-D (N)`.
fn category_row(label: &str, additions: u64, deletions: u64, files: u64) -> Line<'static> {
    Line::from(vec![
        Span::raw(format!("  {label}: ")),
        Span::raw(format!(
            "{} ({files})",
            crate::format::format_size(additions, deletions)
        )),
    ])
}

/// A muted section-header span (e.g. `reviews`, `checks`).
fn section_header(label: &str) -> Span<'static> {
    Span::styled(label.to_owned(), Style::default().fg(Color::Cyan))
}

/// A section header followed by a `Loading…` placeholder, for a section whose
/// enrichment hasn't arrived.
fn header_with_loading(label: &str) -> Line<'static> {
    Line::from(vec![
        section_header(label),
        Span::raw(" "),
        Span::styled(LOADING, Style::default().fg(Color::Gray)),
    ])
}

/// Theme colour for a smart-status tier. Mirrors the TS `blockerTierColor`.
fn tier_color(tier: Tier) -> Color {
    match tier {
        Tier::MeBlocking => Color::Magenta,
        Tier::WaitingOnAuthor => Color::Yellow,
        Tier::NeedsReview => Color::Gray,
    }
}

/// A muted `Loading…` placeholder line for a not-yet-arrived section.
fn loading_line() -> Line<'static> {
    Line::from(Span::styled(LOADING, Style::default().fg(Color::Gray)))
}
