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
use crate::palette::DARK;

/// Conclusions that count as a failing check for display. `action_required`
/// is included here so a completed check that needs follow-up gets an
/// individual row instead of hiding behind the passed count. The blocker
/// engine treats it as its own Next Action after hard CI failures.
const FAILING_CONCLUSIONS: [&str; 4] = ["failure", "timed_out", "cancelled", "action_required"];

/// Format a past instant as a compact age relative to `now`. Mirrors the TS
/// `formatAge` in `src/lib/format.ts`: "now", "Nm", "Nh", "Nd", "Nmo",
/// "NyNmo" / "Ny".
pub fn format_age(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (now - then).num_seconds().max(0);
    if seconds < 60 {
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
/// single column (`check_row`) and the detail view's grid both prepend it so a
/// check reads the same depth in either view. The single source of truth for
/// that depth.
pub const CHECK_INDENT: &str = "  ";

/// The styled spans for one check cell within `max_width` columns — the coloured
/// status icon from `check_icon`, the `workflow / job` label, and (when present)
/// the muted Check Duration: `✓ ci / build 2m`.
///
/// The `workflow / ` prefix (separator included) is painted muted so the job
/// name stands out as the primary text. The icon and duration are never dropped;
/// the job name is middle-truncated to fit (`ci / Gen…Commits`) so a long label
/// still lines up. Pass `usize::MAX` to disable truncation. The single source of
/// truth for a check's painted content, shared by the summary panel and the
/// detail grid so the icon colouring and label treatment stay identical. The
/// `CHECK_INDENT` is prepended by each caller, not here.
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
        // `workflow / ` muted, then the job name in the default colour. The job
        // absorbs the squeeze; only a very tight column truncates the prefix too.
        Some(workflow) => {
            let prefix = format!("{workflow} / ");
            if prefix.width() >= label_budget {
                spans.push(Span::styled(
                    format!(" {}", truncate_middle(&prefix, label_budget)),
                    Style::default().fg(DARK.muted),
                ));
            } else {
                let job = truncate_middle(&check.name, label_budget - prefix.width());
                spans.push(Span::styled(
                    format!(" {prefix}"),
                    Style::default().fg(DARK.muted),
                ));
                spans.push(Span::raw(job));
            }
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
mod tests {
    use std::ffi::OsString;

    use chrono::TimeZone;

    use ratatui::text::Line;
    use unicode_width::UnicodeWidthStr;

    use super::{
        CheckOutcome, ChecksSummary, CommentCounts, ReviewsSummary, abbreviate_home,
        abbreviate_home_with, check_cell_spans, check_icon, check_row, check_sort_group,
        checks_summary, checks_two_column_lines, comment_counts, format_age, format_duration,
        format_merge_status, format_mergeable, format_repo_short, format_review_state, format_size,
        outcome, pad_to_width, review_icon, reviews_summary, sort_check_runs, truncate,
        truncate_middle,
    };
    use crate::github::types::{CheckRun, FullReviewThread, PRState, Review, ReviewComment};
    use crate::palette::DARK;

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 5, 20, 12, 0, 0).unwrap()
    }

    fn check(name: &str, status: &str, conclusion: Option<&str>) -> CheckRun {
        CheckRun {
            name: name.to_owned(),
            workflow_name: None,
            status: status.to_owned(),
            conclusion: conclusion.map(str::to_owned),
            started_at: None,
            completed_at: None,
        }
    }

    /// A check in a named workflow, so the `workflow / job` label renders.
    fn check_in(workflow: &str, name: &str) -> CheckRun {
        CheckRun {
            workflow_name: Some(workflow.to_owned()),
            ..check(name, "completed", Some("success"))
        }
    }

    /// A completed check with a Check Duration of `seconds` (both endpoints
    /// present). The wall-clock start is arbitrary; only the span matters.
    fn timed_check(name: &str, conclusion: &str, seconds: i64) -> CheckRun {
        let started = now();
        CheckRun {
            name: name.to_owned(),
            workflow_name: None,
            status: "completed".to_owned(),
            conclusion: Some(conclusion.to_owned()),
            started_at: Some(started),
            completed_at: Some(started + chrono::Duration::seconds(seconds)),
        }
    }

    fn review(state: &str) -> Review {
        Review {
            user: "r".to_owned(),
            state: state.to_owned(),
        }
    }

    /// A thread resolved per `resolved`, whose comments are built from
    /// `(author, is_bot)` pairs (the first one decides bot classification). An
    /// empty `comments` slice models a thread with no first comment.
    fn thread(resolved: bool, comments: &[(&str, bool)]) -> FullReviewThread {
        FullReviewThread {
            id: "T".to_owned(),
            is_resolved: resolved,
            path: "src/x.rs".to_owned(),
            line: Some(1),
            comments: comments
                .iter()
                .map(|(author, is_bot)| ReviewComment {
                    id: "C".to_owned(),
                    author: (*author).to_owned(),
                    body: "b".to_owned(),
                    created_at: now(),
                    url: "u".to_owned(),
                    is_bot: *is_bot,
                })
                .collect(),
        }
    }

    #[test]
    fn format_age_under_minute_is_now() {
        let then = now() - chrono::Duration::seconds(45);
        assert_eq!(format_age(then, now()), "now");
    }

    #[test]
    fn format_age_returns_compact_units() {
        assert_eq!(
            format_age(now() - chrono::Duration::minutes(15), now()),
            "15m"
        );
        assert_eq!(format_age(now() - chrono::Duration::hours(3), now()), "3h");
        assert_eq!(format_age(now() - chrono::Duration::hours(48), now()), "2d");
        assert_eq!(format_age(now() - chrono::Duration::days(45), now()), "1mo");
    }

    #[test]
    fn format_size_renders_additions_and_deletions() {
        assert_eq!(format_size(5, 3), "+5/-3");
        assert_eq!(format_size(0, 0), "+0/-0");
    }

    #[test]
    fn format_repo_short_keeps_only_the_repo_name() {
        assert_eq!(format_repo_short("owner/widgets"), "widgets");
        assert_eq!(format_repo_short("widgets"), "widgets");
    }

    #[test]
    fn truncate_leaves_short_strings_alone() {
        assert_eq!(truncate("hi", 10), "hi");
    }

    #[test]
    fn truncate_appends_ellipsis_for_long_strings() {
        assert_eq!(truncate("abcdefghij", 5), "abcd…");
    }

    #[test]
    fn truncate_measures_wide_chars_by_display_width() {
        // Each CJK ideograph is two columns wide. At max=5 the budget before
        // the ellipsis is 4 columns, so only two ideographs (4 cols) fit.
        let result = truncate("一二三四五", 5);
        assert_eq!(result, "一二…");
        assert!(result.width() <= 5, "must fit the column: {result:?}");
    }

    #[test]
    fn truncate_middle_preserves_both_ends_of_long_strings() {
        assert_eq!(
            truncate_middle("very-long-author-name", 14),
            "very-lo…r-name"
        );
    }

    #[test]
    fn truncate_middle_counts_display_columns_for_wide_glyphs() {
        // Five ideographs are ten columns. At max 7 the head budget is 3
        // columns — only one whole 2-column glyph fits (a glyph straddling
        // the boundary is dropped, not clipped) — and likewise the tail.
        let result = truncate_middle("一二三四五", 7);
        assert_eq!(result, "一…五");
    }

    #[test]
    fn pad_to_width_counts_display_columns() {
        // Two ideographs already fill four columns; padding to 6 adds two
        // spaces, not "6 - char_count".
        assert_eq!(pad_to_width("一二", 6), "一二  ");
        assert_eq!(pad_to_width("ab", 5), "ab   ");
        assert_eq!(pad_to_width("already wide", 4), "already wide");
    }

    #[test]
    fn abbreviate_home_replaces_home_prefix() {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let home = home.to_string_lossy();

        assert_eq!(abbreviate_home(&home), "~");
        assert_eq!(
            abbreviate_home(&format!("{home}/src/widgets")),
            "~/src/widgets"
        );
        assert_eq!(
            abbreviate_home("/srv/worktrees/widgets"),
            "/srv/worktrees/widgets"
        );
    }

    #[test]
    fn abbreviate_home_ignores_empty_home() {
        assert_eq!(
            abbreviate_home_with("/srv/worktrees/widgets", Some(OsString::new())),
            "/srv/worktrees/widgets"
        );
    }

    #[test]
    fn format_review_state_labels_the_four_states() {
        assert_eq!(format_review_state("APPROVED"), "approved");
        assert_eq!(
            format_review_state("CHANGES_REQUESTED"),
            "changes requested"
        );
        assert_eq!(format_review_state("COMMENTED"), "commented");
        assert_eq!(format_review_state("DISMISSED"), "dismissed");
        assert_eq!(format_review_state("WAT"), "?");
    }

    #[test]
    fn format_merge_status_shows_lifecycle_state_for_non_open_prs() {
        // A merged/closed PR's mergeable is a permanent UNKNOWN; the lifecycle
        // state takes over so the row never shows a misleading "? merge unknown".
        assert_eq!(
            format_merge_status(&PRState::Merged, "UNKNOWN"),
            ("✓ merged", DARK.merged),
        );
        assert_eq!(
            format_merge_status(&PRState::Closed, "UNKNOWN"),
            ("✗ closed", DARK.failing),
        );
        // An OPEN PR falls through to the plain mergeable flag unchanged.
        assert_eq!(
            format_merge_status(&PRState::Open, "MERGEABLE"),
            format_mergeable("MERGEABLE"),
        );
        assert_eq!(
            format_merge_status(&PRState::Open, "UNKNOWN"),
            ("? merge unknown", DARK.muted),
        );
    }

    #[test]
    fn review_icon_maps_state_to_icon_and_colour() {
        assert_eq!(review_icon("APPROVED"), ("✓", DARK.approved));
        assert_eq!(
            review_icon("CHANGES_REQUESTED"),
            ("✗", DARK.changes_requested)
        );
        assert_eq!(review_icon("COMMENTED"), ("●", DARK.commented));
        assert_eq!(review_icon("DISMISSED"), ("–", DARK.muted));
        assert_eq!(review_icon("WAT"), ("?", DARK.muted));
    }

    #[test]
    fn check_sort_group_orders_failing_then_pending_then_rest() {
        assert_eq!(
            check_sort_group(&check("a", "completed", Some("failure"))),
            0
        );
        assert_eq!(
            check_sort_group(&check("a", "completed", Some("action_required"))),
            0
        );
        assert_eq!(check_sort_group(&check("a", "in_progress", None)), 1);
        assert_eq!(
            check_sort_group(&check("a", "completed", Some("success"))),
            2
        );
        assert_eq!(
            check_sort_group(&check("a", "completed", Some("skipped"))),
            2
        );
    }

    #[test]
    fn sort_check_runs_groups_then_sorts_by_name() {
        let runs = [
            check("zebra", "completed", Some("success")),
            check("alpha", "in_progress", None),
            check("yak", "completed", Some("failure")),
            check("beta", "completed", Some("failure")),
        ];
        let mut refs: Vec<&CheckRun> = runs.iter().collect();
        sort_check_runs(&mut refs);
        let names: Vec<&str> = refs.iter().map(|c| c.name.as_str()).collect();
        // Failing group (beta, yak) first, then pending (alpha), then passing
        // (zebra); untimed checks tie on duration, so they fall back to name.
        assert_eq!(names, ["beta", "yak", "alpha", "zebra"]);
    }

    #[test]
    fn sort_check_runs_orders_by_duration_descending_within_a_group() {
        // All passing, so the group is equal; the slowest must surface first,
        // and an untimed check sorts below every timed one (as if zero).
        let runs = [
            timed_check("fast", "success", 30),
            timed_check("slow", "success", 600),
            check("untimed", "completed", Some("success")),
            timed_check("medium", "success", 120),
        ];
        let mut refs: Vec<&CheckRun> = runs.iter().collect();
        sort_check_runs(&mut refs);
        let names: Vec<&str> = refs.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["slow", "medium", "fast", "untimed"]);
    }

    #[test]
    fn sort_check_runs_ranks_a_zero_duration_timed_check_above_an_untimed_one() {
        // A timed check that rounds to 0s must still outrank an untimed check in
        // the same group rather than tying it at zero and falling to name order.
        let runs = [
            check("aaa-untimed", "completed", Some("success")),
            timed_check("zzz-instant", "success", 0),
        ];
        let mut refs: Vec<&CheckRun> = runs.iter().collect();
        sort_check_runs(&mut refs);
        let names: Vec<&str> = refs.iter().map(|c| c.name.as_str()).collect();
        // "aaa" < "zzz" by name, yet the timed (0s) check sorts first.
        assert_eq!(names, ["zzz-instant", "aaa-untimed"]);
    }

    #[test]
    fn sort_check_runs_breaks_equal_durations_by_name() {
        let runs = [
            timed_check("yak", "success", 60),
            timed_check("alpha", "success", 60),
        ];
        let mut refs: Vec<&CheckRun> = runs.iter().collect();
        sort_check_runs(&mut refs);
        let names: Vec<&str> = refs.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["alpha", "yak"]);
    }

    #[test]
    fn sort_check_runs_keeps_failing_first_even_when_passing_is_slower() {
        // A slow passing check must still sort below a fast failing one: the
        // outcome group dominates the duration tiebreak.
        let runs = [
            timed_check("slow-pass", "success", 600),
            timed_check("fast-fail", "failure", 5),
        ];
        let mut refs: Vec<&CheckRun> = runs.iter().collect();
        sort_check_runs(&mut refs);
        let names: Vec<&str> = refs.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["fast-fail", "slow-pass"]);
    }

    #[test]
    fn format_duration_renders_sub_minute_minutes_and_missing() {
        assert_eq!(format_duration(Some(chrono::Duration::seconds(45))), "45s");
        assert_eq!(format_duration(Some(chrono::Duration::seconds(0))), "0s");
        assert_eq!(format_duration(Some(chrono::Duration::seconds(150))), "2m");
        assert_eq!(format_duration(Some(chrono::Duration::minutes(10))), "10m");
        // No duration renders as nothing — never a zero or placeholder.
        assert_eq!(format_duration(None), "");
    }

    #[test]
    fn checks_two_column_caps_rows_and_reports_overflow() {
        // Twenty short checks that all pair two-up. A three-row budget renders
        // three rows of two — six checks — and reports the other fourteen as
        // overflow. The cap is on rows, so it holds twice as many checks.
        let runs: Vec<CheckRun> = (0..20)
            .map(|i| check(&format!("c{i:02}"), "completed", Some("success")))
            .collect();
        let refs: Vec<&CheckRun> = runs.iter().collect();

        let (lines, overflow) = checks_two_column_lines(&refs, 30, 3);
        assert_eq!(lines.len(), 3, "row budget caps the grid at three rows");
        assert_eq!(overflow, 14, "the remaining checks overflow");
        for line in &lines {
            assert_eq!(
                line.spans.iter().filter(|s| s.content == "✓").count(),
                2,
                "each capped row still pairs two checks"
            );
        }
    }

    #[test]
    fn check_icon_maps_status_and_conclusion() {
        assert_eq!(
            check_icon(&check("a", "in_progress", None)),
            ("●", DARK.pending)
        );
        assert_eq!(
            check_icon(&check("a", "completed", Some("success"))),
            ("✓", DARK.passing)
        );
        assert_eq!(
            check_icon(&check("a", "completed", Some("failure"))),
            ("✗", DARK.failing)
        );
        assert_eq!(
            check_icon(&check("a", "completed", Some("action_required"))),
            ("✗", DARK.pending)
        );
        assert_eq!(
            check_icon(&check("a", "completed", Some("neutral"))),
            ("–", DARK.muted)
        );
        assert_eq!(
            check_icon(&check("a", "completed", Some("skipped"))),
            ("⊘", DARK.muted)
        );
        assert_eq!(
            check_icon(&check("a", "completed", Some("stale"))),
            ("⟳", DARK.pending)
        );
        assert_eq!(
            check_icon(&check("a", "completed", None)),
            ("?", DARK.muted)
        );
    }

    #[test]
    fn check_row_indents_icon_and_name() {
        let row = check_row(&check("build", "completed", Some("success")), usize::MAX);
        // Two-space indent, the icon span, then the space-prefixed name. An
        // untimed check shows no duration. Carry the icon's colour through so
        // the row matches `check_icon`.
        let text: String = row.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "  ✓ build");
        let icon_span = &row.spans[1];
        assert_eq!(icon_span.content.as_ref(), "✓");
        assert_eq!(icon_span.style.fg, Some(DARK.passing));
    }

    #[test]
    fn check_row_appends_muted_duration_when_present() {
        let row = check_row(&timed_check("build", "success", 150), usize::MAX);
        let text: String = row.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "  ✓ build 2m");
        // The duration is the last span and reads in the muted role.
        let duration_span = row.spans.last().expect("a duration span");
        assert_eq!(duration_span.content.as_ref(), " 2m");
        assert_eq!(duration_span.style.fg, Some(DARK.muted));
    }

    #[test]
    fn check_cell_paints_the_workflow_prefix_muted_and_the_job_plain() {
        let spans = check_cell_spans(&check_in("ci", "Tests"), usize::MAX);
        // icon, then a muted `workflow / ` prefix, then the job name in the
        // default colour so it stands out.
        let prefix = &spans[1];
        assert_eq!(prefix.content.as_ref(), " ci / ");
        assert_eq!(prefix.style.fg, Some(DARK.muted));
        let job = &spans[2];
        assert_eq!(job.content.as_ref(), "Tests");
        assert_eq!(job.style.fg, None, "the job name uses the default colour");

        // No workflow → a single plain name span, no separator.
        let bare = check_cell_spans(&check("Tests", "completed", Some("success")), usize::MAX);
        assert_eq!(bare[1].content.as_ref(), " Tests");
        assert_eq!(bare[1].style.fg, None);
    }

    #[test]
    fn check_row_middle_truncates_a_long_label_to_fit() {
        // A label far wider than the row: the icon and indent are kept, the label
        // is middle-truncated with an ellipsis, and the whole row fits `width`.
        let run = check_in("release", "Generate Version, Release Notes and Commits");
        let width = 24;
        let row = check_row(&run, width);
        let text: String = line_text(&row);

        assert!(text.contains('…'), "label is middle-truncated: {text:?}");
        assert!(
            text.starts_with("  ✓ release / "),
            "workflow prefix is kept: {text:?}"
        );
        assert!(
            text.chars().count() <= width,
            "row fits the width ({}): {text:?}",
            text.chars().count()
        );
    }

    /// Flatten a `Line`'s spans back to plain text for an exact column assertion.
    fn line_text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn checks_two_column_pairs_narrow_checks_and_solos_a_wide_one() {
        let runs = [
            check("a", "completed", Some("success")),
            check("b", "completed", Some("success")),
            check(&"x".repeat(20), "completed", Some("success")),
            check("c", "completed", Some("success")),
            check("d", "completed", Some("success")),
        ];
        let refs: Vec<&CheckRun> = runs.iter().collect();

        // Width 30: drawable 28, half 13. `✓ a` (3) pairs; the 20-char name
        // (cell 22) is wider than half, so it gets its own row. The pairable
        // cells are all 3 wide, so the column stride is 3 + gap(2) = 5.
        let (lines, overflow) = checks_two_column_lines(&refs, 30, 8);
        let texts: Vec<String> = lines.iter().map(line_text).collect();

        assert_eq!(overflow, 0, "everything fits in the row budget");
        assert_eq!(texts.len(), 3, "two pair rows + one solo row: {texts:?}");
        assert_eq!(texts[0], "  ✓ a  ✓ b", "first pair packs to content");
        assert_eq!(
            lines[1].spans.iter().filter(|s| s.content == "✓").count(),
            1,
            "the wide check is alone on its row: {:?}",
            texts[1]
        );
        assert!(
            texts[1].contains(&"x".repeat(20)),
            "wide check name: {texts:?}"
        );
        assert_eq!(
            texts[2], "  ✓ c  ✓ d",
            "the checks after the wide one re-pair"
        );
    }

    #[test]
    fn checks_two_column_degrades_to_one_column_when_too_narrow() {
        let runs = [
            check("alpha", "completed", Some("success")),
            check("bravo", "completed", Some("success")),
        ];
        let refs: Vec<&CheckRun> = runs.iter().collect();

        // A panel too narrow to fit two columns: every check takes its own row,
        // exactly like the old single column.
        let (lines, overflow) = checks_two_column_lines(&refs, 8, 8);
        assert_eq!(overflow, 0, "both checks fit");
        assert_eq!(lines.len(), 2, "one check per row");
        for line in &lines {
            assert_eq!(
                line.spans.iter().filter(|s| s.content == "✓").count(),
                1,
                "single column: one check per row"
            );
        }
    }

    #[test]
    fn outcome_classifies_pending_failed_and_passed() {
        assert_eq!(
            outcome(&check("a", "in_progress", None)),
            CheckOutcome::Pending
        );
        assert_eq!(
            outcome(&check("a", "completed", Some("failure"))),
            CheckOutcome::Failed
        );
        assert_eq!(
            outcome(&check("a", "completed", Some("timed_out"))),
            CheckOutcome::Failed
        );
        assert_eq!(
            outcome(&check("a", "completed", Some("success"))),
            CheckOutcome::Passed
        );
        // Non-failing completed conclusions (neutral, skipped, …) count as
        // passed; `action_required` is failed — the check is blocked on human
        // action, so it must not hide behind "N/N passed".
        assert_eq!(
            outcome(&check("a", "completed", Some("neutral"))),
            CheckOutcome::Passed
        );
        assert_eq!(
            outcome(&check("a", "completed", Some("action_required"))),
            CheckOutcome::Failed
        );
    }

    #[test]
    fn checks_summary_tallies_by_outcome() {
        let checks = vec![
            check("build", "completed", Some("success")),
            check("lint", "completed", Some("failure")),
            check("deploy", "in_progress", None),
            check("audit", "completed", Some("neutral")), // counts as passed
            check("e2e", "completed", Some("action_required")), // counts as failed
        ];
        assert_eq!(
            checks_summary(&checks),
            ChecksSummary {
                failed: 2,
                pending: 1,
                passed: 2,
                total: 5,
            }
        );
    }

    #[test]
    fn comment_counts_tallies_total_unresolved_human_and_bot() {
        let no_bots: [String; 0] = [];
        let threads = [
            thread(false, &[("alice", false)]),     // unresolved human
            thread(false, &[("dependabot", true)]), // unresolved bot (is_bot flag)
            thread(true, &[("bob", false)]),        // resolved: total only
        ];
        assert_eq!(
            comment_counts(&threads, &no_bots),
            CommentCounts {
                total: 3,
                unresolved: 2,
                unresolved_human: 1,
                unresolved_bot: 1,
            }
        );
    }

    #[test]
    fn comment_counts_marks_configured_bot_login_as_bot() {
        // The first comment's author isn't flagged `is_bot`, but a configured
        // bot login still classifies the thread as unresolved-bot.
        let bot_logins = ["renovate".to_owned()];
        let threads = [thread(false, &[("renovate", false)])];
        assert_eq!(
            comment_counts(&threads, &bot_logins),
            CommentCounts {
                total: 1,
                unresolved: 1,
                unresolved_human: 0,
                unresolved_bot: 1,
            }
        );
    }

    #[test]
    fn comment_counts_classifies_on_first_comment_only() {
        // The first comment (human) decides; a later bot comment is ignored.
        let no_bots: [String; 0] = [];
        let threads = [thread(false, &[("alice", false), ("dependabot", true)])];
        assert_eq!(comment_counts(&threads, &no_bots).unresolved_human, 1);
    }

    #[test]
    fn comment_counts_treats_empty_comments_thread_as_human() {
        // No first comment, so the TS `firstComment != null` null-check fails
        // and the unresolved thread falls through to human.
        let no_bots: [String; 0] = [];
        let threads = [thread(false, &[])];
        assert_eq!(
            comment_counts(&threads, &no_bots),
            CommentCounts {
                total: 1,
                unresolved: 1,
                unresolved_human: 1,
                unresolved_bot: 0,
            }
        );
    }

    #[test]
    fn reviews_summary_tallies_by_state() {
        let reviews = [
            review("APPROVED"),
            review("APPROVED"),
            review("CHANGES_REQUESTED"),
            review("COMMENTED"),
            review("DISMISSED"), // not counted in any bucket
        ];
        assert_eq!(
            reviews_summary(&reviews),
            ReviewsSummary {
                approved: 2,
                changes_requested: 1,
                commented: 1,
            }
        );
    }
}
