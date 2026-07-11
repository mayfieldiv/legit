//! Pure display formatters. Take inputs explicitly (no `Utc::now()` or other
//! ambient state) so they're trivially testable.
//!
//! This is the canonical display layer mirroring the TS `src/lib/format.ts`:
//! the check/review icon, colour, label, sort, and summary helpers live here so
//! both the summary panel and the detail view (issue #51) consume them rather
//! than re-deriving them per panel. The icon/colour helpers return a data-only
//! `(&'static str, Color)` tuple — mirroring TS's plain `{ icon, fg }` shape.
//! `check_cell_spans` goes one step further and assembles a check's ready-to-paint
//! spans (the icon, the `workflow / job` label, the duration), which the summary
//! grid and the detail grid both wrap into `Line<'static>`s so a check reads
//! identically in either view — which is why this module pulls in ratatui's
//! `Line`/`Span` text types.

use chrono::{DateTime, Utc};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::github::types::{CheckRun, FullReviewThread, PRState, Review};
use crate::palette::{DARK, Palette};

/// Conclusions that count as a failing check for display. `action_required`
/// is included here so a completed check that needs follow-up gets an
/// individual row instead of hiding behind the passed count. The blocker
/// engine treats it as its own Next Action after hard CI failures.
const FAILING_CONCLUSIONS: [&str; 4] = ["failure", "timed_out", "cancelled", "action_required"];

/// Seconds below which an age reads as the sub-minute case: `format_age`'s "now"
/// and `fetched_age_spans`'s "just now". Shared between the two so their
/// sub-minute thresholds can never drift apart.
const SUB_MINUTE_SECS: i64 = 60;

/// Format a past instant as a compact age relative to `now`. Mirrors the TS
/// `formatAge` in `src/lib/format.ts`: "now", "Nm", "Nh", "Nd", "Nmo",
/// "NyNmo" / "Ny".
pub fn format_age(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (now - then).num_seconds().max(0);
    if seconds < SUB_MINUTE_SECS {
        return "now".to_owned();
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{hours}h");
    }
    let days = hours / 24;
    if days < 30 {
        return format!("{days}d");
    }
    let months = days / 30;
    if months < 12 {
        return format!("{months}mo");
    }
    let years = months / 12;
    let rem = months % 12;
    if rem > 0 {
        format!("{years}y{rem}mo")
    } else {
        format!("{years}y")
    }
}

/// The styled spans for a PR's Fetch Age — the muted `fetched ` label plus the
/// `<age> ago` value (`fetched 2m ago`), or `fetched just now` within the first
/// minute. The single source of truth for the staleness signal's painted
/// content, shared by the summary panel (which wraps it in its own line) and the
/// detail header (which prepends a ` · ` separator and extends its meta row).
/// Mirrors `check_cell_spans`: the caller owns the surrounding layout, this owns
/// the cell.
///
/// `None` (the PR has no Fetch Age stamp yet — see CONTEXT.md "Fetch Age")
/// returns an empty `Vec`, so an unfetched PR shows no misleading "now". The
/// wording stays distinct from GitHub's "updated Y" activity time so the local
/// staleness signal is never confused with `updated_at`.
pub fn fetched_age_spans(
    fetched_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    palette: &Palette,
) -> Vec<Span<'static>> {
    let Some(fetched_at) = fetched_at else {
        return Vec::new();
    };
    // Under a minute reads "just now", never the ungrammatical "now ago". Branch
    // on the elapsed seconds directly — the same `SUB_MINUTE_SECS` threshold
    // `format_age` uses — rather than re-parsing its "now" output: no throwaway
    // String and no literal-match coupling to that function's sub-minute wording.
    let value = if (now - fetched_at).num_seconds() < SUB_MINUTE_SECS {
        "just now".to_owned()
    } else {
        format!("{} ago", format_age(fetched_at, now))
    };
    vec![
        Span::styled("fetched ", Style::default().fg(palette.muted)),
        Span::raw(value),
    ]
}

/// Format additions/deletions as `+A/-D`.
pub fn format_size(additions: u64, deletions: u64) -> String {
    format!("+{additions}/-{deletions}")
}

/// Compact display for a repo slug: `owner/repo` -> `repo`.
pub fn format_repo_short(slug: &str) -> &str {
    slug.rsplit_once('/').map_or(slug, |(_, repo)| repo)
}

/// The longest prefix of `s` that fits within `budget` display columns
/// (measured via `unicode-width`, not `char` count). A wide char straddling
/// the boundary is dropped whole rather than clipped to half a glyph.
fn width_prefix(s: &str, budget: usize) -> &str {
    let mut width = 0;
    let mut end = 0;
    for ch in s.chars() {
        let w = ch.width().unwrap_or(0);
        if width + w > budget {
            break;
        }
        width += w;
        end += ch.len_utf8();
    }
    &s[..end]
}

/// The longest suffix of `s` that fits within `budget` display columns; the
/// suffix twin of `width_prefix`, with the same whole-glyph boundary rule.
fn width_suffix(s: &str, budget: usize) -> &str {
    let mut width = 0;
    let mut start = s.len();
    for ch in s.chars().rev() {
        let w = ch.width().unwrap_or(0);
        if width + w > budget {
            break;
        }
        width += w;
        start -= ch.len_utf8();
    }
    &s[start..]
}

/// Truncate `s` to at most `max` terminal columns, appending `…` when
/// shortened. Width is measured in display columns (via `unicode-width`), not
/// `char` count: CJK ideographs and emoji occupy two columns, so a char-count
/// truncation would overflow the column it's sized for.
pub fn truncate(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if s.width() <= max {
        return s.to_owned();
    }
    // One column reserved for the ellipsis.
    format!("{}…", width_prefix(s, max - 1))
}

/// Truncate `s` to at most `max` terminal columns, replacing the middle with
/// `…` when shortened. Use this for path-like or identity-like cells where the
/// beginning and end both carry signal (repo slugs, long logins).
pub fn truncate_middle(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if s.width() <= max {
        return s.to_owned();
    }
    if max == 1 {
        return "…".to_owned();
    }
    // One column reserved for the ellipsis; the head rounds up on odd splits.
    let keep = max - 1;
    let head_budget = keep.div_ceil(2);
    format!(
        "{}…{}",
        width_prefix(s, head_budget),
        width_suffix(s, keep - head_budget)
    )
}

/// Right-pad `s` with spaces to at least `width` terminal columns. Like
/// `format!("{s:<width$}")` but measures display columns instead of `char`
/// count, so columns stay aligned when a cell contains wide glyphs.
pub fn pad_to_width(s: &str, width: usize) -> String {
    let used = s.width();
    if used >= width {
        return s.to_owned();
    }
    format!("{s}{}", " ".repeat(width - used))
}

/// Replace a leading `$HOME` with `~` for compact path display.
pub fn abbreviate_home(absolute_path: &str) -> String {
    abbreviate_home_with(absolute_path, std::env::var_os("HOME"))
}

fn abbreviate_home_with(absolute_path: &str, home: Option<std::ffi::OsString>) -> String {
    let Some(home) = home.filter(|home| !home.as_os_str().is_empty()) else {
        return absolute_path.to_owned();
    };
    let home = home.to_string_lossy();
    if absolute_path == home {
        "~".to_owned()
    } else if let Some(rest) = absolute_path.strip_prefix(&format!("{home}/")) {
        format!("~/{rest}")
    } else {
        absolute_path.to_owned()
    }
}

// ── Check & review display helpers ──────────────────────────────────────────

/// Human label for a review state. Mirrors the TS `formatReviewState`.
pub fn format_review_state(state: &str) -> &'static str {
    match state {
        "APPROVED" => "approved",
        "CHANGES_REQUESTED" => "changes requested",
        "COMMENTED" => "commented",
        "DISMISSED" => "dismissed",
        // The TS enum is exhaustive over the four states above; an unknown
        // value falls back to a question mark like the icon helper.
        _ => "?",
    }
}

/// Text + colour for a PR's mergeable state. Mirrors the TS `formatMergeable`:
/// `MERGEABLE` → "✓ mergeable" (green), `CONFLICTING` → "! conflict" (red),
/// anything else (including `UNKNOWN`) → "? merge unknown" (gray).
/// The OPEN-state formatter underneath `format_merge_status` — not called
/// directly by the views.
fn format_mergeable(mergeable: &str) -> (&'static str, Color) {
    match mergeable {
        "MERGEABLE" => ("✓ mergeable", DARK.passing),
        "CONFLICTING" => ("! conflict", DARK.failing),
        _ => ("? merge unknown", DARK.muted),
    }
}

/// Text + colour for a PR's merge readiness, lifecycle-aware. A MERGED or CLOSED
/// PR's `mergeable` is a permanent, meaningless `UNKNOWN`, so once a refresh has
/// detected the transition (see CONTEXT.md "Lifecycle State") the row shows the
/// state itself — "merged" (magenta) or "closed" (red) — instead of a misleading
/// "? merge unknown". An OPEN PR falls through to the plain mergeable flag. This
/// is the lifecycle-aware wrapper the summary/detail views call; the bare
/// `format_mergeable` stays the canonical OPEN-state formatter underneath. No TS
/// counterpart — the reference UI shows "? merge unknown" for merged PRs too.
pub fn format_merge_status(state: &PRState, mergeable: &str) -> (&'static str, Color) {
    match state {
        PRState::Merged => ("✓ merged", DARK.merged),
        PRState::Closed => ("✗ closed", DARK.failing),
        PRState::Open => format_mergeable(mergeable),
    }
}

/// Icon + colour for a review state. Mirrors the TS `reviewIcon`.
pub fn review_icon(state: &str) -> (&'static str, Color) {
    match state {
        "APPROVED" => ("✓", DARK.approved),
        "CHANGES_REQUESTED" => ("✗", DARK.changes_requested),
        "COMMENTED" => ("●", DARK.commented),
        "DISMISSED" => ("–", DARK.muted),
        _ => ("?", DARK.muted),
    }
}

/// Sort group for a check row: failing first (0), then pending (1), then
/// everything else (2). Derived from `outcome` so sorting can never disagree
/// with the header counts about which checks are failing; the derived groups
/// match the TS `checkSortGroup` exactly (it already put `action_required` in
/// the failing group).
pub fn check_sort_group(check: &CheckRun) -> u8 {
    match outcome(check) {
        CheckOutcome::Failed => 0,
        CheckOutcome::Pending => 1,
        CheckOutcome::Passed => 2,
    }
}

/// Sort `checks` in place by outcome priority, then Check Duration descending
/// (slowest first), then name. A check with no duration sorts strictly below any
/// timed check in its outcome group — even a timed check whose duration rounds to
/// zero outranks an untimed one — so completed/timed checks always surface above
/// untimed ones; name is the final stable tiebreak. Callers that need the
/// original order untouched sort a borrowed slice of references.
pub fn sort_check_runs(checks: &mut [&CheckRun]) {
    checks.sort_by(|a, b| {
        check_sort_group(a)
            .cmp(&check_sort_group(b))
            // Descending Check Duration via Option ordering: None < Some, so reversing
            // the operands sorts longer durations first and drops untimed checks last.
            .then_with(|| b.duration().cmp(&a.duration()))
            .then(a.name.cmp(&b.name))
    });
}

/// Render a Check Duration as a short human string. `None` (an untimed check)
/// renders as the empty string — never a zero or placeholder. Sub-minute spans
/// read in seconds (`45s`); a minute or more reads in whole minutes (`3m`),
/// matching the compact, lossy posture of `format_age`.
pub fn format_duration(duration: Option<chrono::Duration>) -> String {
    let Some(duration) = duration else {
        return String::new();
    };
    let seconds = duration.num_seconds().max(0);
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m", seconds / 60)
    }
}

/// Icon + colour for a check run. Mirrors the TS `checkIcon`.
pub fn check_icon(check: &CheckRun) -> (&'static str, Color) {
    if check.status != "completed" {
        return ("●", DARK.pending);
    }
    match check.conclusion.as_deref() {
        Some("success") => ("✓", DARK.passing),
        Some("failure" | "timed_out" | "cancelled") => ("✗", DARK.failing),
        Some("action_required") => ("✗", DARK.pending),
        Some("neutral") => ("–", DARK.muted),
        Some("skipped") => ("⊘", DARK.muted),
        Some("stale") => ("⟳", DARK.pending),
        _ => ("?", DARK.muted),
    }
}

/// The two-space indent every check row sits behind — the summary panel's
/// two-column grid (with `check_row` as its solo-row fallback) and the detail
/// view's grid both prepend it so a check reads the same depth in either view.
/// The single source of truth for that depth.
pub const CHECK_INDENT: &str = "  ";

/// Width to keep of a truncated workflow name before the job name starts
/// truncating too. The job is the more distinctive half, so a tight column
/// shrinks the `workflow / ` prefix down to this floor first (or to the
/// workflow's full width, if it's already shorter).
const MIN_WORKFLOW_WIDTH: usize = 8;

/// Fit a `workflow` + ` / ` + `job` label into `budget` columns. The workflow
/// shrinks first — down to [`MIN_WORKFLOW_WIDTH`] — and only the remaining
/// shortfall comes off the job, since the job name carries the most signal.
/// Returns the (possibly truncated) workflow and job; the caller adds the ` / `.
/// The workflow truncates at the end (keeping its recognisable start); the job
/// truncates in the middle (both ends carry signal, e.g. `build…backend`).
fn fit_workflow_label(workflow: &str, job: &str, budget: usize) -> (String, String) {
    const SEP: usize = 3; // " / "
    let (wf_width, job_width) = (workflow.width(), job.width());
    if wf_width + SEP + job_width <= budget {
        return (workflow.to_owned(), job.to_owned());
    }
    let over = (wf_width + SEP + job_width) - budget;
    // Take from the workflow first, but not below the floor; whatever shortfall
    // is left then comes off the job.
    let floor = wf_width.min(MIN_WORKFLOW_WIDTH);
    let wf_target = wf_width.saturating_sub(over).max(floor);
    let remaining = over - (wf_width - wf_target);
    let job_target = job_width.saturating_sub(remaining);
    // Final clamp so an extreme-narrow column can't overflow the budget.
    let wf_target = wf_target.min(budget.saturating_sub(SEP));
    let job_target = job_target.min(budget.saturating_sub(wf_target + SEP));
    (
        truncate(workflow, wf_target),
        truncate_middle(job, job_target),
    )
}

/// The styled spans for one check cell within `max_width` columns — the coloured
/// status icon from `check_icon`, the `workflow / job` label, and (when present)
/// the muted Check Duration: `✓ ci / build 2m`.
///
/// The `workflow / ` prefix (separator included) is painted muted so the job
/// name stands out as the primary text. The icon and duration are never dropped;
/// the label is truncated to fit via [`fit_workflow_label`] — the workflow gives
/// way first, then the job. Pass `usize::MAX` to disable truncation. The single
/// source of truth for a check's painted content, shared by the summary panel
/// and the detail grid so the icon colouring and label treatment stay identical.
/// The `CHECK_INDENT` is prepended by each caller, not here.
pub fn check_cell_spans(check: &CheckRun, max_width: usize) -> Vec<Span<'static>> {
    let (icon, color) = check_icon(check);
    let duration = format_duration(check.duration());
    // Columns the fixed parts always take: the icon, the space before the label,
    // and (when present) the space + duration. The label gets whatever is left.
    let duration_cols = if duration.is_empty() {
        0
    } else {
        1 + duration.width()
    };
    let label_budget = max_width.saturating_sub(icon.width() + 1 + duration_cols);

    let mut spans = vec![Span::styled(icon, Style::default().fg(color))];
    match check.workflow_name.as_deref().filter(|w| !w.is_empty()) {
        // `workflow / ` muted, then the job name in the default colour.
        Some(workflow) => {
            let (wf_text, job_text) = fit_workflow_label(workflow, &check.name, label_budget);
            spans.push(Span::styled(
                format!(" {wf_text} / "),
                Style::default().fg(DARK.muted),
            ));
            spans.push(Span::raw(job_text));
        }
        // No workflow: the bare job name, middle-truncated to fit.
        None => spans.push(Span::raw(format!(
            " {}",
            truncate_middle(&check.name, label_budget)
        ))),
    }
    if !duration.is_empty() {
        spans.push(Span::styled(
            format!(" {duration}"),
            Style::default().fg(DARK.muted),
        ));
    }
    spans
}

/// The untruncated display width of one check's painted cell: the icon, the full
/// `workflow / job` label, and any Check Duration. Sets the column stride for
/// both grids (which size columns to the content, then truncate as needed).
///
/// Measures by building the real spans via `check_cell_spans` and summing rather
/// than a width-only path that re-derives the layout arithmetic, so the width
/// can't drift from what's actually painted. The throwaway `Vec<Span>` is a
/// deliberate trade of allocation for that guarantee — at most a couple dozen
/// cells, measured a few times per throttled frame, it's nowhere near observable.
pub fn check_cell_width(check: &CheckRun) -> usize {
    check_cell_spans(check, usize::MAX)
        .iter()
        .map(|s| s.content.width())
        .sum()
}

/// One indented single-column check row within `width` columns — two spaces then
/// the cell, the label middle-truncated to fit. The single source of truth for a
/// one-column check row (the summary grid's solo rows), so the indent and content
/// stay identical to the detail grid's cells.
pub fn check_row(check: &CheckRun, width: usize) -> Line<'static> {
    let mut spans = vec![Span::raw(CHECK_INDENT)];
    spans.extend(check_cell_spans(
        check,
        width.saturating_sub(CHECK_INDENT.len()),
    ));
    Line::from(spans)
}

/// Blank columns separating the summary panel's two check columns. The second
/// column begins this many spaces past the widest pairable cell.
const SUMMARY_COLUMN_GAP: usize = 2;

/// Lay the already-ordered `checks` into up to two columns for the summary panel
/// at content width `width`, emitting at most `max_rows` grid rows. Returns the
/// rendered rows (the caller adds the header) and the count of checks that didn't
/// fit — the `+N more` overflow.
///
/// Most checks pair two-up, packed to the left: the column stride is the widest
/// *pairable* cell plus [`SUMMARY_COLUMN_GAP`], so the second column hugs the
/// first rather than spreading to the panel's midpoint. A check whose cell is
/// too wide to leave room for a partner — wider than half the drawable width —
/// takes its own full-width row (so does a trailing unpaired check). This is the
/// "some rows have just one check" behaviour: a narrow panel where nothing pairs
/// degrades cleanly to the old single column.
///
/// The cap is on *rows*, not checks, so a wide terminal that pairs everything
/// shows up to `2 × max_rows` checks while the section's height stays bounded.
pub fn checks_two_column_lines(
    checks: &[&CheckRun],
    width: usize,
    max_rows: usize,
) -> (Vec<Line<'static>>, usize) {
    let inner = width.saturating_sub(CHECK_INDENT.len());
    // A cell wider than half the drawable width can't share a row with a
    // partner, so it can never be a column — it gets a solo row instead.
    let half = inner.saturating_sub(SUMMARY_COLUMN_GAP) / 2;
    let pairable = |check: &CheckRun| check_cell_width(check) <= half;

    // Pack the paired columns to content: the widest pairable cell plus the gap
    // is where the second column begins. Two columns always fit — a pairable
    // cell is at most `half` wide and `CHECK_INDENT + 2·half + GAP <= width`.
    // Measure the stride over only the checks that could fill the row budget (at
    // most two per row) so a long name ranked past the cap can't widen the
    // visible columns.
    let candidate = &checks[..checks.len().min(2 * max_rows)];
    let stride = candidate
        .iter()
        .copied()
        .filter(|c| pairable(c))
        .map(check_cell_width)
        .max()
        .map_or(0, |widest| widest + SUMMARY_COLUMN_GAP);

    let mut lines: Vec<Line<'static>> = Vec::new();
    // Count checks actually rendered into a row; a check held in `pending` is not
    // rendered until flushed, so the overflow tally below counts it correctly.
    let mut placed = 0usize;
    let mut pending: Option<&CheckRun> = None;
    let mut i = 0;
    while i < checks.len() && lines.len() < max_rows {
        let check = checks[i];
        if pairable(check) {
            match pending.take() {
                None => {
                    pending = Some(check);
                    i += 1;
                }
                Some(first) => {
                    lines.push(two_column_row(first, check, stride));
                    placed += 2;
                    i += 1;
                }
            }
        } else if let Some(first) = pending.take() {
            // A wide check can't pair: flush the half-formed pair to its own row
            // first. Don't advance — the wide check is handled next iteration if
            // the row budget still allows.
            lines.push(check_row(first, width));
            placed += 1;
        } else {
            // A solo row spans the full width; its label is middle-truncated to
            // fit so a long `workflow / job` reads `release / Gen…Commits 1m`.
            lines.push(check_row(check, width));
            placed += 1;
            i += 1;
        }
    }
    // A trailing unpaired check gets its own row if the budget allows.
    if let Some(first) = pending.take()
        && lines.len() < max_rows
    {
        lines.push(check_row(first, width));
        placed += 1;
    }
    (lines, checks.len().saturating_sub(placed))
}

/// One two-column grid row: `first` (after the shared [`CHECK_INDENT`]) padded
/// out to `stride` columns so `second` aligns, then `second` unpadded. Both
/// cells are pairable — each at most half the drawable width — so neither needs
/// truncating; the row fits within the panel by construction.
fn two_column_row(first: &CheckRun, second: &CheckRun, stride: usize) -> Line<'static> {
    let cell = check_cell_spans(first, usize::MAX);
    let used: usize = cell.iter().map(|s| s.content.width()).sum();
    let mut spans = vec![Span::raw(CHECK_INDENT)];
    spans.extend(cell);
    if used < stride {
        spans.push(Span::raw(" ".repeat(stride - used)));
    }
    spans.extend(check_cell_spans(second, usize::MAX));
    Line::from(spans)
}

/// The muted `+N more` overflow line shown when the visible-check cap hides some
/// checks: `  +3 more`. Indented with `CHECK_INDENT` so it lines up under the
/// check rows in either view. The muted colour is passed in because the summary
/// panel themes from the live `palette` while the detail grid uses `DARK`.
pub fn overflow_line(count: usize, muted: Color) -> Line<'static> {
    Line::from(Span::styled(
        format!("{CHECK_INDENT}+{count} more"),
        Style::default().fg(muted),
    ))
}

/// Order `checks` by the shared check ordering and return them as references,
/// leaving the input slice untouched. The single source of truth for the order
/// both the summary panel and the detail grid draw checks in. Each view then
/// lays out and caps the sorted list its own way: the detail grid sizes columns
/// to the body width, while the summary panel packs two-up to a row budget.
pub fn sorted_check_runs(checks: &[CheckRun]) -> Vec<&CheckRun> {
    let mut sorted: Vec<&CheckRun> = checks.iter().collect();
    sort_check_runs(&mut sorted);
    sorted
}

/// The three-way classification of a check run's outcome. The single source of
/// truth for "what counts as passing/pending/failed": `checks_summary` (the
/// header counts), the summary view's per-row filter, and `check_sort_group`
/// all classify via `outcome`, so the header totals, the rendered
/// failed+pending rows, and the sort order can never disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckOutcome {
    Failed,
    Pending,
    Passed,
}

/// Classify a check run's outcome: a non-completed run is pending; a completed
/// run whose conclusion is in `FAILING_CONCLUSIONS` is failed; every other
/// completed run (success, neutral, skipped, stale, …) counts as passed.
/// `action_required` counts as failed for display — see
/// `FAILING_CONCLUSIONS`.
pub fn outcome(check: &CheckRun) -> CheckOutcome {
    if check.status != "completed" {
        CheckOutcome::Pending
    } else if check
        .conclusion
        .as_deref()
        .is_some_and(|c| FAILING_CONCLUSIONS.contains(&c))
    {
        CheckOutcome::Failed
    } else {
        CheckOutcome::Passed
    }
}

/// Check-run counts by outcome. Mirrors the TS `checksSummary`'s shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChecksSummary {
    pub failed: usize,
    pub pending: usize,
    pub passed: usize,
    pub total: usize,
}

/// Tally check runs by outcome, classifying each via `outcome` (the shared
/// source of truth with the summary view's per-row filter).
pub fn checks_summary(checks: &[CheckRun]) -> ChecksSummary {
    let mut summary = ChecksSummary {
        failed: 0,
        pending: 0,
        passed: 0,
        total: checks.len(),
    };
    for check in checks {
        match outcome(check) {
            CheckOutcome::Failed => summary.failed += 1,
            CheckOutcome::Pending => summary.pending += 1,
            CheckOutcome::Passed => summary.passed += 1,
        }
    }
    summary
}

/// Review-thread comment counts. Mirrors the TS `CommentCounts`. `unresolved`
/// is the sum of `unresolved_human` and `unresolved_bot`; `total` counts every
/// thread including resolved ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommentCounts {
    pub total: usize,
    pub unresolved: usize,
    pub unresolved_human: usize,
    pub unresolved_bot: usize,
}

/// Tally review threads into `CommentCounts`. Mirrors the TS
/// `computeCommentCounts` exactly: every thread counts toward `total`; an
/// unresolved thread is bot when its *first* comment is a bot — either the
/// fetch-time `is_bot` flag or an author in `bot_logins` — else human. A thread
/// with no comments has no first comment, so it falls through to human (the TS
/// `firstComment != null` null-check).
pub fn comment_counts(threads: &[FullReviewThread], bot_logins: &[String]) -> CommentCounts {
    let mut counts = CommentCounts {
        total: 0,
        unresolved: 0,
        unresolved_human: 0,
        unresolved_bot: 0,
    };
    for thread in threads {
        counts.total += 1;
        if thread.is_resolved {
            continue;
        }
        counts.unresolved += 1;
        let is_bot = thread
            .comments
            .first()
            .is_some_and(|c| c.is_bot || bot_logins.iter().any(|b| b == &c.author));
        if is_bot {
            counts.unresolved_bot += 1;
        } else {
            counts.unresolved_human += 1;
        }
    }
    counts
}

/// Review counts by state. Mirrors `ChecksSummary`'s shape: the summary view's
/// reviews header derives its counts from this single pass rather than three
/// inline filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewsSummary {
    pub approved: usize,
    pub changes_requested: usize,
    pub commented: usize,
}

/// Tally reviews by state in a single pass. States other than the three counted
/// here (e.g. `DISMISSED`) contribute to none of the buckets, matching the
/// per-reviewer header the summary panel renders.
pub fn reviews_summary(reviews: &[Review]) -> ReviewsSummary {
    let mut summary = ReviewsSummary {
        approved: 0,
        changes_requested: 0,
        commented: 0,
    };
    for review in reviews {
        match review.state.as_str() {
            "APPROVED" => summary.approved += 1,
            "CHANGES_REQUESTED" => summary.changes_requested += 1,
            "COMMENTED" => summary.commented += 1,
            _ => {}
        }
    }
    summary
}

#[cfg(test)]
mod tests;
