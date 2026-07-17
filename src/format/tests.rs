use std::ffi::OsString;

use chrono::TimeZone;

use ratatui::text::Line;
use unicode_width::UnicodeWidthStr;

use super::{
    CheckOutcome, ChecksSummary, CommentCounts, MIN_WORKFLOW_WIDTH, ReviewsSummary,
    abbreviate_home, abbreviate_home_with, check_cell_spans, check_icon, check_row,
    check_sort_group, checks_summary, checks_two_column_lines, comment_counts, fetched_age_spans,
    fit_workflow_label, format_age, format_duration, format_merge_status, format_mergeable,
    format_repo_short, format_review_state, format_size, outcome, pad_to_width, review_icon,
    reviews_summary, sort_check_runs, truncate, truncate_middle,
};
use crate::github::types::{CheckRun, FullReviewThread, PRState, Review, ReviewComment};
use crate::palette::DARK;
use crate::test_fixtures::{check, timed_check};

fn now() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc.with_ymd_and_hms(2026, 5, 20, 12, 0, 0).unwrap()
}

/// A check in a named workflow, so the `workflow / job` label renders.
fn check_in(workflow: &str, name: &str) -> CheckRun {
    CheckRun {
        workflow_name: Some(workflow.to_owned()),
        ..check(name, "completed", Some("success"))
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
fn fetched_age_spans_renders_muted_label_and_age_value() {
    let fetched = now() - chrono::Duration::minutes(2);
    let spans = fetched_age_spans(Some(fetched), now(), &DARK);
    let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(text, "fetched 2m ago");
    // The label reads in the muted role; the value carries no override.
    assert_eq!(spans[0].content.as_ref(), "fetched ");
    assert_eq!(spans[0].style.fg, Some(DARK.muted));
    assert_eq!(spans[1].style.fg, None);
}

#[test]
fn fetched_age_spans_reads_just_now_within_the_first_minute() {
    // Under a minute old, format_age yields "now"; the line must read
    // "fetched just now", never the ungrammatical "fetched now ago".
    let fetched = now() - chrono::Duration::seconds(20);
    let spans = fetched_age_spans(Some(fetched), now(), &DARK);
    let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(text, "fetched just now");
}

#[test]
fn fetched_age_spans_is_empty_without_a_stamp() {
    // No Fetch Age stamp yet: an empty Vec so an unfetched PR shows no
    // misleading "now".
    assert!(fetched_age_spans(None, now(), &DARK).is_empty());
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

#[test]
fn fit_workflow_label_shrinks_the_workflow_before_the_job() {
    // Plenty of room: nothing truncated.
    assert_eq!(
        fit_workflow_label("ci", "Tests", 40),
        ("ci".to_owned(), "Tests".to_owned())
    );

    // Tight enough that the workflow must give, but the job still fits whole:
    // the workflow truncates (end ellipsis) while the job is left untouched.
    let (wf, job) = fit_workflow_label("Build and Test on Pull Request", "deploy", 26);
    assert!(wf.contains('…'), "workflow truncated first: {wf:?}");
    assert_eq!(
        job, "deploy",
        "job stays whole while the workflow has slack"
    );

    // Tighter still: the workflow bottoms out at its floor, then the job
    // starts truncating too.
    let (wf, job) = fit_workflow_label(
        "Build and Test on Pull Request",
        "build-and-test-backend",
        24,
    );
    assert!(
        wf.width() <= MIN_WORKFLOW_WIDTH,
        "workflow held at its floor: {wf:?} ({}w)",
        wf.width()
    );
    assert!(
        wf.contains('…') && job.contains('…'),
        "both truncated once the workflow hit the floor: {wf:?} / {job:?}"
    );
    assert!(wf.width() + 3 + job.width() <= 24, "fits the budget");
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
