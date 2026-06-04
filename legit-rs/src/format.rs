//! Pure display formatters. Take inputs explicitly (no `Utc::now()` or other
//! ambient state) so they're trivially testable.
//!
//! This is the canonical display layer mirroring the TS `src/lib/format.ts`:
//! the check/review icon, colour, label, sort, and summary helpers live here so
//! both the summary panel and the detail view (issue #51) consume them rather
//! than re-deriving them per panel. The icon/colour helpers return a data-only
//! `(&'static str, Color)` tuple — mirroring TS's plain `{ icon, fg }` shape —
//! so this module depends on ratatui's `Color`, not on any widget types.

use chrono::{DateTime, Utc};
use ratatui::style::Color;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::github::types::CheckRun;

/// Conclusions that count as a failing check. Mirrors the TS
/// `FAILING_CONCLUSIONS` (and the blocker engine's set).
const FAILING_CONCLUSIONS: [&str; 3] = ["failure", "timed_out", "cancelled"];

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
    // Reserve one column for the ellipsis, then take chars until the next one
    // would spill past the budget. A wide char straddling the boundary is
    // dropped whole rather than clipped to half a glyph.
    let budget = max - 1;
    let mut width = 0;
    let mut head = String::new();
    for ch in s.chars() {
        let w = ch.width().unwrap_or(0);
        if width + w > budget {
            break;
        }
        width += w;
        head.push(ch);
    }
    format!("{head}…")
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

/// Icon + colour for a review state. Mirrors the TS `reviewIcon`.
pub fn review_icon(state: &str) -> (&'static str, Color) {
    match state {
        "APPROVED" => ("✓", Color::Green),
        "CHANGES_REQUESTED" => ("✗", Color::Red),
        "COMMENTED" => ("●", Color::Blue),
        "DISMISSED" => ("–", Color::Gray),
        _ => ("?", Color::Gray),
    }
}

/// Sort group for a check row. Mirrors the TS `checkSortGroup`: failing first
/// (0), then pending (1), then everything else (2).
pub fn check_sort_group(check: &CheckRun) -> u8 {
    if check.status != "completed" {
        return 1;
    }
    match check.conclusion.as_deref() {
        Some("failure" | "timed_out" | "cancelled" | "action_required") => 0,
        _ => 2,
    }
}

/// Sort `checks` in place by sort group then name. Mirrors the TS
/// `sortCheckRuns` (which returns a sorted copy); callers that need the
/// original order untouched sort a borrowed slice of references.
pub fn sort_check_runs(checks: &mut [&CheckRun]) {
    checks.sort_by(|a, b| {
        check_sort_group(a)
            .cmp(&check_sort_group(b))
            .then(a.name.cmp(&b.name))
    });
}

/// Icon + colour for a check run. Mirrors the TS `checkIcon`.
pub fn check_icon(check: &CheckRun) -> (&'static str, Color) {
    if check.status != "completed" {
        return ("●", Color::Yellow);
    }
    match check.conclusion.as_deref() {
        Some("success") => ("✓", Color::Green),
        Some("failure" | "timed_out" | "cancelled") => ("✗", Color::Red),
        Some("action_required") => ("✗", Color::Yellow),
        Some("neutral") => ("–", Color::Gray),
        Some("skipped") => ("⊘", Color::Gray),
        Some("stale") => ("⟳", Color::Yellow),
        _ => ("?", Color::Gray),
    }
}

/// The three-way classification of a check run's outcome. The single source of
/// truth for "what counts as passing/pending/failed": both `checks_summary`
/// (the header counts) and the summary view's per-row filter classify via
/// `outcome`, so the header totals and the rendered failed+pending rows can
/// never disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckOutcome {
    Failed,
    Pending,
    Passed,
}

/// Classify a check run's outcome. Mirrors the TS `checksSummary` bucketing: a
/// non-completed run is pending; a completed run whose conclusion is in
/// `FAILING_CONCLUSIONS` is failed; every other completed run (success, neutral,
/// skipped, stale, …) counts as passed.
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

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use ratatui::style::Color;

    use unicode_width::UnicodeWidthStr;

    use super::{
        CheckOutcome, ChecksSummary, check_icon, check_sort_group, checks_summary, format_age,
        format_review_state, format_size, outcome, pad_to_width, review_icon, sort_check_runs,
        truncate,
    };
    use crate::github::types::CheckRun;

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 5, 20, 12, 0, 0).unwrap()
    }

    fn check(name: &str, status: &str, conclusion: Option<&str>) -> CheckRun {
        CheckRun {
            name: name.to_owned(),
            status: status.to_owned(),
            conclusion: conclusion.map(str::to_owned),
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
    fn pad_to_width_counts_display_columns() {
        // Two ideographs already fill four columns; padding to 6 adds two
        // spaces, not "6 - char_count".
        assert_eq!(pad_to_width("一二", 6), "一二  ");
        assert_eq!(pad_to_width("ab", 5), "ab   ");
        assert_eq!(pad_to_width("already wide", 4), "already wide");
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
    fn review_icon_maps_state_to_icon_and_colour() {
        assert_eq!(review_icon("APPROVED"), ("✓", Color::Green));
        assert_eq!(review_icon("CHANGES_REQUESTED"), ("✗", Color::Red));
        assert_eq!(review_icon("COMMENTED"), ("●", Color::Blue));
        assert_eq!(review_icon("DISMISSED"), ("–", Color::Gray));
        assert_eq!(review_icon("WAT"), ("?", Color::Gray));
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
        // (zebra); ties within a group sort by name.
        assert_eq!(names, ["beta", "yak", "alpha", "zebra"]);
    }

    #[test]
    fn check_icon_maps_status_and_conclusion() {
        assert_eq!(
            check_icon(&check("a", "in_progress", None)),
            ("●", Color::Yellow)
        );
        assert_eq!(
            check_icon(&check("a", "completed", Some("success"))),
            ("✓", Color::Green)
        );
        assert_eq!(
            check_icon(&check("a", "completed", Some("failure"))),
            ("✗", Color::Red)
        );
        assert_eq!(
            check_icon(&check("a", "completed", Some("action_required"))),
            ("✗", Color::Yellow)
        );
        assert_eq!(
            check_icon(&check("a", "completed", Some("neutral"))),
            ("–", Color::Gray)
        );
        assert_eq!(
            check_icon(&check("a", "completed", Some("skipped"))),
            ("⊘", Color::Gray)
        );
        assert_eq!(
            check_icon(&check("a", "completed", Some("stale"))),
            ("⟳", Color::Yellow)
        );
        assert_eq!(
            check_icon(&check("a", "completed", None)),
            ("?", Color::Gray)
        );
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
        // passed, and `action_required` is *not* in FAILING_CONCLUSIONS so it
        // passes too — matching `checks_summary`'s bucketing.
        assert_eq!(
            outcome(&check("a", "completed", Some("neutral"))),
            CheckOutcome::Passed
        );
        assert_eq!(
            outcome(&check("a", "completed", Some("action_required"))),
            CheckOutcome::Passed
        );
    }

    #[test]
    fn checks_summary_tallies_by_outcome() {
        let checks = vec![
            check("build", "completed", Some("success")),
            check("lint", "completed", Some("failure")),
            check("deploy", "in_progress", None),
            check("audit", "completed", Some("neutral")), // counts as passed
        ];
        assert_eq!(
            checks_summary(&checks),
            ChecksSummary {
                failed: 1,
                pending: 1,
                passed: 2,
                total: 4,
            }
        );
    }
}
