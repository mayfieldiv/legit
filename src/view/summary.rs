//! The right-side summary panel for the selected PR. Renders, top to bottom:
//! PR identity metadata + GitHub URL -> branch/worktree/mergeability -> Next
//! Action (coloured by smart-status tier) -> threads summary ->
//! reviews/requested reviewers -> CI checks summary -> file-category size
//! breakdown -> contextual metadata. Sections whose enrichment hasn't arrived
//! render a "Loading…" placeholder so the panel fills in reactively as the
//! per-PR fan-out lands.
//!
//! Panel width is a function of the terminal width: hidden below 80 columns,
//! 36 columns at 80-139, 50 columns at >=140 — defined by
//! `app::list_layout::panel_width`, the canonical list-view geometry shared
//! with `view::view` (which splits the main area) and mouse hit-testing.

use chrono::{DateTime, Utc};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::model::{FilesState, Model};
use crate::chip::label_lines;
use crate::format::{
    check_row, checks_summary, comment_counts, format_age, format_merge_status,
    format_review_state, format_size, overflow_line, review_icon, reviews_summary, truncate,
    visible_checks,
};
use crate::github::rest::PR;
use crate::palette::Palette;

/// Placeholder text for a section whose enrichment hasn't arrived yet.
const LOADING: &str = "Loading…";

#[cfg(test)]
mod tests;

/// Render the summary panel into `area`. Assumes `area` is the panel's region
/// (already split off the list by the caller).
pub fn render(
    model: &Model,
    frame: &mut Frame<'_>,
    area: Rect,
    now: DateTime<Utc>,
    palette: &Palette,
) {
    let Some(pr) = model.list.selected_pr() else {
        let line = Line::from(Span::styled(
            "No PR selected",
            Style::default().fg(palette.muted),
        ));
        frame.render_widget(Paragraph::new(line), area);
        return;
    };

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.extend(identity_lines(
        model,
        pr,
        now,
        usize::from(area.width),
        palette,
    ));
    lines.push(next_action_line(model, pr, palette));
    lines.push(threads_line(model, pr, palette));
    lines.extend(reviews_lines(model, pr, palette));
    lines.extend(requested_reviewers_lines(pr, palette));
    lines.extend(checks_lines(model, pr, palette));
    lines.extend(files_lines(model, pr, palette));
    lines.extend(label_lines(&pr.labels, usize::from(area.width), palette));
    lines.extend(assignees_lines(pr, usize::from(area.width), palette));

    frame.render_widget(Paragraph::new(lines), area);
}

fn identity_lines(
    model: &Model,
    pr: &PR,
    now: DateTime<Utc>,
    width: usize,
    palette: &Palette,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        truncate(&pr.title, width.max(1)),
        Style::default().add_modifier(Modifier::BOLD),
    )));

    let mut meta = vec![
        Span::styled(pr.author.clone(), Style::default().fg(palette.author)),
        Span::raw(format!(" #{}", pr.number)),
    ];
    if pr.is_draft {
        meta.push(Span::styled(" draft", Style::default().fg(palette.draft)));
    }
    lines.push(Line::from(meta));

    lines.push(url_line(pr, width, palette));
    lines.extend(checkout_status_lines(model, pr, width, palette));

    lines.push(Line::from(vec![
        Span::styled("created ", Style::default().fg(palette.muted)),
        Span::raw(format_age(pr.created_at, now)),
        Span::styled(" updated ", Style::default().fg(palette.muted)),
        Span::raw(format_age(pr.updated_at, now)),
    ]));

    lines
}

fn checkout_status_lines(
    model: &Model,
    pr: &PR,
    width: usize,
    palette: &Palette,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    let branch_spans = branch_spans(pr, palette);
    if !branch_spans.is_empty() {
        lines.push(Line::from(branch_spans));
    }

    if let Some(worktree) = model.worktree_for_pr(pr) {
        lines.push(super::worktree_line(
            &worktree.path,
            width.saturating_sub(worktree_label_width()),
            palette,
        ));
    }

    lines.push(mergeability_line(pr));
    lines
}

fn branch_spans(pr: &PR, palette: &Palette) -> Vec<Span<'static>> {
    if pr.head_ref.is_empty() && pr.base_ref.is_empty() {
        return Vec::new();
    }

    vec![
        Span::styled(pr.head_ref.clone(), Style::default().fg(palette.accent)),
        Span::styled(" → ", Style::default().fg(palette.separator)),
        Span::styled(pr.base_ref.clone(), Style::default().fg(palette.accent)),
    ]
}

fn worktree_label_width() -> usize {
    1 + " worktree: ".len()
}

/// The merge/lifecycle-state line. Delegates to `format::format_merge_status` —
/// the lifecycle-aware helper shared with the detail view, so a merged/closed
/// PR shows its state rather than a permanent "? merge unknown".
fn mergeability_line(pr: &PR) -> Line<'static> {
    let (text, color) = format_merge_status(&pr.state, &pr.mergeable);
    Line::from(Span::styled(text, Style::default().fg(color)))
}

fn assignees_lines(pr: &PR, width: usize, palette: &Palette) -> Vec<Line<'static>> {
    if pr.assignees.is_empty() {
        return Vec::new();
    }
    let text = format!("assignees: {}", pr.assignees.join(", "));
    vec![Line::from(vec![
        Span::styled("assignees: ", Style::default().fg(palette.muted)),
        Span::raw(truncate(
            text.strip_prefix("assignees: ").unwrap_or(&text),
            width.saturating_sub("assignees: ".len()).max(1),
        )),
    ])]
}

/// The Next Action line, coloured by smart-status tier (me-blocking magenta,
/// waiting-on-author yellow, needs-review gray). `Loading…` until the PR's
/// blocker has been derived (both threads and reviews arrived).
fn next_action_line(model: &Model, pr: &PR, palette: &Palette) -> Line<'static> {
    match model.blockers.get(&pr.key()) {
        Some(result) => Line::from(Span::styled(
            result.reason.clone(),
            Style::default().fg(palette.tier(result.tier)),
        )),
        None => loading_line(palette),
    }
}

/// The reviews section: a `reviews` header with approved / changes-requested /
/// commented counts, then one indented row per reviewer with an icon and their
/// state. `Loading…` beside the header until the reviews fetch arrives (`None`
/// = not loaded, distinct from `Some(&[])` = loaded, no reviews).
fn reviews_lines(model: &Model, pr: &PR, palette: &Palette) -> Vec<Line<'static>> {
    let Some(reviews) = model.enrichment.reviews.get(&pr.key()) else {
        return vec![header_with_loading("reviews", palette)];
    };

    let summary = reviews_summary(reviews);
    let mut lines = vec![Line::from(vec![
        section_header("reviews", palette),
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
                Style::default().fg(palette.muted),
            ),
        ]));
    }
    lines
}

fn requested_reviewers_lines(pr: &PR, palette: &Palette) -> Vec<Line<'static>> {
    if pr.requested_reviewers.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![Line::from(section_header("requested", palette))];
    for reviewer in &pr.requested_reviewers {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("○", Style::default().fg(palette.pending)),
            Span::raw(format!(" {reviewer} ")),
            Span::styled("pending", Style::default().fg(palette.muted)),
        ]));
    }
    lines
}

/// The threads summary line: `threads N total, M unresolved (H human, B bot)`.
/// `Loading…` until the review-threads fetch arrives. A thin formatter over
/// `format::comment_counts` (the canonical derivation shared with the detail
/// view in issue #51), which mirrors the TS `computeCommentCounts` bot
/// classification.
fn threads_line(model: &Model, pr: &PR, palette: &Palette) -> Line<'static> {
    let Some(threads) = model.enrichment.review_threads.get(&pr.key()) else {
        return header_with_loading("threads", palette);
    };

    let counts = comment_counts(threads, &model.config.bot_logins);
    Line::from(vec![
        section_header("threads", palette),
        Span::raw(format!(
            " {} total, {} unresolved ({} human, {} bot)",
            counts.total, counts.unresolved, counts.unresolved_human, counts.unresolved_bot
        )),
    ])
}

/// The CI checks section: a `checks` header with failed / pending / passed
/// counts (always over ALL checks), then one indented row per check of any
/// outcome — ordered failing-first, then slowest, then name — capped at eight
/// with a `+N more` overflow line. A single column, mirroring the detail view's
/// grid ordering. `Loading…` until the checks fetch arrives — which can't start
/// until review-status reports the PR's head SHA, so a PR with no head SHA also
/// reads as loading.
fn checks_lines(model: &Model, pr: &PR, palette: &Palette) -> Vec<Line<'static>> {
    let Some(checks) = model.enrichment.checks_for(pr) else {
        return vec![header_with_loading("checks", palette)];
    };

    let summary = checks_summary(checks);

    let mut header: Vec<Span<'static>> = vec![section_header("checks", palette), Span::raw(" ")];
    if summary.failed > 0 {
        header.push(Span::styled(
            format!("{} failed ", summary.failed),
            Style::default().fg(palette.failing),
        ));
    }
    if summary.pending > 0 {
        header.push(Span::styled(
            format!("{} pending ", summary.pending),
            Style::default().fg(palette.pending),
        ));
    }
    header.push(Span::styled(
        format!("{}/{} passed", summary.passed, summary.total),
        Style::default().fg(if summary.passed == summary.total {
            palette.passing
        } else {
            palette.muted
        }),
    ));
    let mut lines = vec![Line::from(header)];

    // Up to eight checks of ANY outcome, ordered failing-first then slowest via
    // the shared `visible_checks` selection (the same ordering and cap the
    // detail grid draws from). The header counts above still tally ALL checks.
    let (visible, overflow) = visible_checks(checks);
    for check in visible {
        lines.push(check_row(check));
    }
    if overflow > 0 {
        lines.push(overflow_line(overflow, palette.muted));
    }
    lines
}

/// The File Category breakdown section: a `files` header, then one indented row
/// per non-empty category (`code: +14/-3 (2)`), plus a `total` row. `Loading…`
/// both before the fetch is requested (no entry) and while it's in flight
/// (`Requested`); the breakdown renders once it's `Loaded` and categorised.
fn files_lines(model: &Model, pr: &PR, palette: &Palette) -> Vec<Line<'static>> {
    let categorization = match model.enrichment.files.get(&pr.key()) {
        Some(FilesState::Loaded(categorization)) => categorization,
        None | Some(FilesState::Requested) => return vec![header_with_loading("files", palette)],
    };
    let breakdown = &categorization.breakdown;

    let mut lines = vec![Line::from(section_header("files", palette))];
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

/// The PR's full GitHub URL. Mirrors the TS `prUrl`.
fn url_line(pr: &PR, width: usize, palette: &Palette) -> Line<'static> {
    let url = format!("https://github.com/{}/pull/{}", pr.repo_slug, pr.number);
    Line::from(Span::styled(
        truncate(&url, width.max(1)),
        Style::default().fg(palette.link),
    ))
}

/// One indented breakdown row: `  <label>: +A/-D (N)`.
fn category_row(label: &str, additions: u64, deletions: u64, files: u64) -> Line<'static> {
    Line::from(vec![
        Span::raw(format!("  {label}: ")),
        Span::raw(format!("{} ({files})", format_size(additions, deletions))),
    ])
}

/// An accented section-header span (e.g. `reviews`, `checks`).
fn section_header(label: &str, palette: &Palette) -> Span<'static> {
    Span::styled(label.to_owned(), Style::default().fg(palette.accent))
}

/// A section header followed by a `Loading…` placeholder, for a section whose
/// enrichment hasn't arrived.
fn header_with_loading(label: &str, palette: &Palette) -> Line<'static> {
    Line::from(vec![
        section_header(label, palette),
        Span::raw(" "),
        Span::styled(LOADING, Style::default().fg(palette.muted)),
    ])
}

/// A muted `Loading…` placeholder line for a not-yet-arrived section.
fn loading_line(palette: &Palette) -> Line<'static> {
    Line::from(Span::styled(LOADING, Style::default().fg(palette.muted)))
}
