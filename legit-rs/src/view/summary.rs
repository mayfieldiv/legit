//! The right-side summary panel for the selected PR. Renders, top to bottom:
//! PR identity metadata -> Next Action (coloured by smart-status tier) ->
//! mergeable state -> threads summary -> reviews/requested reviewers -> CI
//! checks summary -> file-category size breakdown -> contextual metadata ->
//! worktree path placeholder -> footer GitHub URL. Sections whose enrichment
//! hasn't arrived render a "Loading…" placeholder so the panel fills in
//! reactively as the per-PR fan-out lands.
//!
//! Panel width is a function of the terminal width: hidden below 80 columns,
//! 36 columns at 80-139, 50 columns at >=140 — defined by
//! `app::list_layout::panel_width`, the canonical list-view geometry shared
//! with `view::view` (which splits the main area) and mouse hit-testing.

use chrono::{DateTime, Utc};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::model::{FilesState, Model};
use crate::format::{
    CheckOutcome, check_row, checks_summary, comment_counts, format_age, format_mergeable,
    format_review_state, format_size, outcome, review_icon, reviews_summary, sort_check_runs,
    truncate,
};
use crate::github::rest::PR;
use crate::github::types::CheckRun;

/// Placeholder text for a section whose enrichment hasn't arrived yet.
const LOADING: &str = "Loading…";

/// Max number of individual check rows before collapsing the rest into a
/// `+N more` line. Mirrors the TS `MAX_VISIBLE_CHECKS`.
const MAX_VISIBLE_CHECKS: usize = 6;

#[cfg(test)]
mod tests;

/// Render the summary panel into `area`. Assumes `area` is the panel's region
/// (already split off the list by the caller).
pub fn render(model: &Model, frame: &mut Frame<'_>, area: Rect, now: DateTime<Utc>) {
    let Some(pr) = model.list.selected_pr() else {
        let line = Line::from(Span::styled(
            "No PR selected",
            Style::default().fg(Color::Gray),
        ));
        frame.render_widget(Paragraph::new(line), area);
        return;
    };

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.extend(identity_lines(pr, now, usize::from(area.width)));
    lines.push(next_action_line(model, pr));
    lines.push(mergeable_line(pr));
    lines.push(threads_line(model, pr));
    lines.extend(reviews_lines(model, pr));
    lines.extend(requested_reviewers_lines(pr));
    lines.extend(checks_lines(model, pr));
    lines.extend(files_lines(model, pr));
    lines.extend(labels_lines(pr, usize::from(area.width)));
    lines.extend(assignees_lines(pr, usize::from(area.width)));
    lines.push(worktree_line(pr));
    lines.push(url_footer_line(pr));

    frame.render_widget(Paragraph::new(lines), area);
}

fn identity_lines(pr: &PR, now: DateTime<Utc>, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        truncate(&pr.title, width.max(1)),
        Style::default().add_modifier(Modifier::BOLD),
    )));

    let mut meta = vec![
        Span::styled(pr.author.clone(), Style::default().fg(Color::Green)),
        Span::raw(format!(" #{}", pr.number)),
    ];
    if pr.is_draft {
        meta.push(Span::styled(" draft", Style::default().fg(Color::Yellow)));
    }
    lines.push(Line::from(meta));

    if !pr.head_ref.is_empty() || !pr.base_ref.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(pr.head_ref.clone(), Style::default().fg(Color::Cyan)),
            Span::styled(" → ", Style::default().fg(Color::Gray)),
            Span::styled(pr.base_ref.clone(), Style::default().fg(Color::Cyan)),
        ]));
    }

    lines.push(Line::from(vec![
        Span::styled("created ", Style::default().fg(Color::Gray)),
        Span::raw(format_age(pr.created_at, now)),
        Span::styled(" updated ", Style::default().fg(Color::Gray)),
        Span::raw(format_age(pr.updated_at, now)),
    ]));

    lines
}

fn labels_lines(pr: &PR, width: usize) -> Vec<Line<'static>> {
    if pr.labels.is_empty() {
        return Vec::new();
    }
    let text = format!("labels: {}", pr.labels.join(", "));
    vec![Line::from(vec![
        Span::styled("labels: ", Style::default().fg(Color::Gray)),
        Span::raw(truncate(
            text.strip_prefix("labels: ").unwrap_or(&text),
            width.saturating_sub("labels: ".len()).max(1),
        )),
    ])]
}

fn assignees_lines(pr: &PR, width: usize) -> Vec<Line<'static>> {
    if pr.assignees.is_empty() {
        return Vec::new();
    }
    let text = format!("assignees: {}", pr.assignees.join(", "));
    vec![Line::from(vec![
        Span::styled("assignees: ", Style::default().fg(Color::Gray)),
        Span::raw(truncate(
            text.strip_prefix("assignees: ").unwrap_or(&text),
            width.saturating_sub("assignees: ".len()).max(1),
        )),
    ])]
}

/// The Next Action line, coloured by smart-status tier (me-blocking magenta,
/// waiting-on-author yellow, needs-review gray). `Loading…` until the PR's
/// blocker has been derived (both threads and reviews arrived).
fn next_action_line(model: &Model, pr: &PR) -> Line<'static> {
    match model.blockers.get(&pr.key()) {
        Some(result) => Line::from(Span::styled(
            result.reason.clone(),
            Style::default().fg(super::tier_color(result.tier)),
        )),
        None => loading_line(),
    }
}

/// The mergeable-state line. Delegates to `format::format_mergeable` — the
/// canonical display helper shared with the detail view.
fn mergeable_line(pr: &PR) -> Line<'static> {
    let (text, color) = format_mergeable(&pr.mergeable);
    Line::from(Span::styled(text, Style::default().fg(color)))
}

/// The reviews section: a `reviews` header with approved / changes-requested /
/// commented counts, then one indented row per reviewer with an icon and their
/// state. `Loading…` beside the header until the reviews fetch arrives (`None`
/// = not loaded, distinct from `Some(&[])` = loaded, no reviews).
fn reviews_lines(model: &Model, pr: &PR) -> Vec<Line<'static>> {
    let Some(reviews) = model.enrichment.reviews.get(&pr.key()) else {
        return vec![header_with_loading("reviews")];
    };

    let summary = reviews_summary(reviews);
    let mut lines = vec![Line::from(vec![
        section_header("reviews"),
        Span::raw(format!(
            " {} approved, {} changes requested, {} commented",
            summary.approved, summary.changes_requested, summary.commented
        )),
    ])];

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

fn requested_reviewers_lines(pr: &PR) -> Vec<Line<'static>> {
    if pr.requested_reviewers.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![Line::from(section_header("requested"))];
    for reviewer in &pr.requested_reviewers {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("○", Style::default().fg(Color::Yellow)),
            Span::raw(format!(" {reviewer} ")),
            Span::styled("pending", Style::default().fg(Color::Gray)),
        ]));
    }
    lines
}

/// The threads summary line: `threads N total, M unresolved (H human, B bot)`.
/// `Loading…` until the review-threads fetch arrives. A thin formatter over
/// `format::comment_counts` (the canonical derivation shared with the detail
/// view in issue #51), which mirrors the TS `computeCommentCounts` bot
/// classification.
fn threads_line(model: &Model, pr: &PR) -> Line<'static> {
    let Some(threads) = model.enrichment.review_threads.get(&pr.key()) else {
        return header_with_loading("threads");
    };

    let counts = comment_counts(threads, &model.config.bot_logins);
    Line::from(vec![
        section_header("threads"),
        Span::raw(format!(
            " {} total, {} unresolved ({} human, {} bot)",
            counts.total, counts.unresolved, counts.unresolved_human, counts.unresolved_bot
        )),
    ])
}

/// The CI checks section: a `checks` header with failed / pending / passed
/// counts, then one indented row per failed, pending, or action-required check
/// (passing checks are summarised by the count alone). `Loading…` until the
/// checks fetch arrives — which can't start until review-status reports the
/// PR's head SHA, so a PR with no head SHA also reads as loading.
fn checks_lines(model: &Model, pr: &PR) -> Vec<Line<'static>> {
    let Some(checks) = model.enrichment.checks_for(pr) else {
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
        lines.push(check_row(check));
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
fn files_lines(model: &Model, pr: &PR) -> Vec<Line<'static>> {
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
fn worktree_line(_pr: &PR) -> Line<'static> {
    Line::from("")
}

/// The footer line: the PR's full GitHub URL. Mirrors the TS `prUrl`.
fn url_footer_line(pr: &PR) -> Line<'static> {
    let url = format!("https://github.com/{}/pull/{}", pr.repo_slug, pr.number);
    Line::from(Span::styled(url, Style::default().fg(Color::Cyan)))
}

/// One indented breakdown row: `  <label>: +A/-D (N)`.
fn category_row(label: &str, additions: u64, deletions: u64, files: u64) -> Line<'static> {
    Line::from(vec![
        Span::raw(format!("  {label}: ")),
        Span::raw(format!("{} ({files})", format_size(additions, deletions))),
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

/// A muted `Loading…` placeholder line for a not-yet-arrived section.
fn loading_line() -> Line<'static> {
    Line::from(Span::styled(LOADING, Style::default().fg(Color::Gray)))
}
