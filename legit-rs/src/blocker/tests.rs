//! Unit tests for the blocker engine. Mirror the table-driven coverage of the
//! TS `tests/blocker-engine.test.ts`: every rule, every edge case, every tier
//! transition. Pure and synchronous — no tokio.

use chrono::{DateTime, TimeZone, Utc};

use super::{BlockerOptions, ThreadKind, Tier, classify_thread, classify_threads, compute_blocker};
use crate::github::rest::{PR, PRState};
use crate::github::types::{CheckRun, FullReviewThread, Review, ReviewComment};

// ── PR builder ───────────────────────────────────────────────────────────────

/// Mirror of the TS `makePR` test helper: a sane default open PR whose fields
/// are overridden per-test via the returned struct.
fn make_pr(author: &str) -> PR {
    PR {
        number: 1,
        title: "Test PR".to_owned(),
        author: author.to_owned(),
        created_at: Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap(),
        updated_at: Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap(),
        additions: 0,
        deletions: 0,
        is_draft: false,
        labels: Vec::new(),
        requested_reviewers: Vec::new(),
        assignees: Vec::new(),
        review_decision: String::new(),
        mergeable: "UNKNOWN".to_owned(),
        last_commit_date: None,
        head_commit_sha: None,
        head_ref: "feature".to_owned(),
        base_ref: "main".to_owned(),
        head_repository_owner: "mayfieldiv".to_owned(),
        state: PRState::Open,
    }
}

const ME: &str = "alice";
const OTHER: &str = "bob";
const AUTHOR: &str = "charlie";

// ── check / review / thread helpers ──────────────────────────────────────────

fn check(name: &str, status: &str, conclusion: Option<&str>) -> CheckRun {
    CheckRun {
        name: name.to_owned(),
        status: status.to_owned(),
        conclusion: conclusion.map(str::to_owned),
    }
}

fn failed_check() -> CheckRun {
    check("ci", "completed", Some("failure"))
}

fn passed_check(name: &str) -> CheckRun {
    check(name, "completed", Some("success"))
}

fn pending_check() -> CheckRun {
    check("ci", "in_progress", None)
}

fn review(user: &str, state: &str) -> Review {
    Review {
        user: user.to_owned(),
        state: state.to_owned(),
    }
}

fn date(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .expect("valid rfc3339")
        .with_timezone(&Utc)
}

fn comment(author: &str) -> ReviewComment {
    comment_at(author, "2026-03-15T00:00:00Z")
}

fn comment_at(author: &str, created_at: &str) -> ReviewComment {
    ReviewComment {
        id: format!("comment-{author}-{created_at}"),
        author: author.to_owned(),
        body: "comment body".to_owned(),
        created_at: date(created_at),
        url: "https://github.com/test".to_owned(),
        is_bot: false,
    }
}

fn bot_comment(author: &str) -> ReviewComment {
    ReviewComment {
        is_bot: true,
        ..comment(author)
    }
}

fn thread(comments: Vec<ReviewComment>) -> FullReviewThread {
    thread_with(comments, false)
}

fn thread_with(comments: Vec<ReviewComment>, is_resolved: bool) -> FullReviewThread {
    FullReviewThread {
        id: "thread".to_owned(),
        is_resolved,
        path: "src/test.rs".to_owned(),
        line: Some(10),
        comments,
    }
}

/// Build `BlockerOptions` from optional enrichment slices.
fn opts<'a>(
    checks: &'a [CheckRun],
    reviews: &'a [Review],
    threads: Option<&'a [FullReviewThread]>,
) -> BlockerOptions<'a> {
    BlockerOptions {
        checks,
        reviews,
        threads,
    }
}

// ── Base tier logic (no extended data) ───────────────────────────────────────

#[test]
fn current_user_requested_reviewer_is_me_blocking() {
    let mut pr = make_pr(AUTHOR);
    pr.requested_reviewers = vec![ME.to_owned()];
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::MeBlocking);
    assert_eq!(result.blocker, ME);
}

#[test]
fn another_reviewer_requested_is_needs_review() {
    let mut pr = make_pr(AUTHOR);
    pr.requested_reviewers = vec![OTHER.to_owned()];
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::NeedsReview);
    assert_eq!(result.blocker, OTHER);
}

#[test]
fn review_decision_changes_requested_is_waiting_on_author() {
    let mut pr = make_pr(AUTHOR);
    pr.review_decision = "CHANGES_REQUESTED".to_owned();
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert_eq!(result.blocker, AUTHOR);
}

#[test]
fn no_reviewers_no_reviews_no_ci_is_needs_review() {
    let pr = make_pr(AUTHOR);
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::NeedsReview);
}

#[test]
fn review_decision_review_required_is_needs_review() {
    let mut pr = make_pr(AUTHOR);
    pr.review_decision = "REVIEW_REQUIRED".to_owned();
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::NeedsReview);
}

#[test]
fn review_decision_approved_is_waiting_on_author() {
    let mut pr = make_pr(AUTHOR);
    pr.review_decision = "APPROVED".to_owned();
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert_eq!(result.blocker, AUTHOR);
    assert!(result.reason.to_lowercase().contains("approved"));
}

// ── CI checks ────────────────────────────────────────────────────────────────

#[test]
fn failing_check_is_waiting_on_author_with_ci_reason() {
    let pr = make_pr(AUTHOR);
    let checks = [failed_check()];
    let result = compute_blocker(&pr, ME, &opts(&checks, &[], None));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert_eq!(result.blocker, AUTHOR);
    assert!(result.reason.to_lowercase().contains("ci"));
}

#[test]
fn timed_out_check_is_waiting_on_author() {
    let pr = make_pr(AUTHOR);
    let checks = [check("build", "completed", Some("timed_out"))];
    let result = compute_blocker(&pr, ME, &opts(&checks, &[], None));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
}

#[test]
fn cancelled_check_is_waiting_on_author() {
    let pr = make_pr(AUTHOR);
    let checks = [check("build", "completed", Some("cancelled"))];
    let result = compute_blocker(&pr, ME, &opts(&checks, &[], None));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
}

#[test]
fn in_progress_check_is_not_ci_failing() {
    let pr = make_pr(AUTHOR);
    let checks = [pending_check()];
    let result = compute_blocker(&pr, ME, &opts(&checks, &[], None));
    assert_ne!(result.tier, Tier::WaitingOnAuthor);
}

#[test]
fn all_passing_checks_no_ci_waiting() {
    let pr = make_pr(AUTHOR);
    let checks = [passed_check("ci")];
    let result = compute_blocker(&pr, ME, &opts(&checks, &[], None));
    assert_eq!(result.tier, Tier::NeedsReview);
}

#[test]
fn mixed_checks_one_failing_is_waiting_on_author() {
    let pr = make_pr(AUTHOR);
    let checks = [
        passed_check("lint"),
        check("test", "completed", Some("failure")),
    ];
    let result = compute_blocker(&pr, ME, &opts(&checks, &[], None));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
}

#[test]
fn ci_failing_overrides_me_blocking() {
    let mut pr = make_pr(AUTHOR);
    pr.requested_reviewers = vec![ME.to_owned()];
    let checks = [failed_check()];
    let result = compute_blocker(&pr, ME, &opts(&checks, &[], None));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert_eq!(result.blocker, AUTHOR);
}

// ── Individual reviews ─────────────────────────────────────────────────────────

#[test]
fn changes_requested_review_by_anyone_is_waiting_on_author() {
    let pr = make_pr(AUTHOR);
    let reviews = [review(OTHER, "CHANGES_REQUESTED")];
    let result = compute_blocker(&pr, ME, &opts(&[], &reviews, None));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert_eq!(result.blocker, AUTHOR);
}

#[test]
fn changes_requested_review_by_current_user_is_waiting_on_author() {
    let pr = make_pr(AUTHOR);
    let reviews = [review(ME, "CHANGES_REQUESTED")];
    let result = compute_blocker(&pr, ME, &opts(&[], &reviews, None));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert_eq!(result.blocker, AUTHOR);
}

#[test]
fn approved_review_by_current_user_is_waiting_on_author() {
    let pr = make_pr(AUTHOR);
    let reviews = [review(ME, "APPROVED")];
    let result = compute_blocker(&pr, ME, &opts(&[], &reviews, None));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
}

#[test]
fn commented_review_does_not_affect_tier() {
    let pr = make_pr(AUTHOR);
    let reviews = [review(OTHER, "COMMENTED")];
    let result = compute_blocker(&pr, ME, &opts(&[], &reviews, None));
    assert_eq!(result.tier, Tier::NeedsReview);
}

#[test]
fn individual_changes_requested_overrides_individual_approved() {
    let pr = make_pr(AUTHOR);
    let reviews = [
        review("dave", "APPROVED"),
        review(OTHER, "CHANGES_REQUESTED"),
    ];
    let result = compute_blocker(&pr, ME, &opts(&[], &reviews, None));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
}

// ── Multiple requested reviewers ───────────────────────────────────────────────

#[test]
fn current_user_plus_others_is_me_blocking() {
    let mut pr = make_pr(AUTHOR);
    pr.requested_reviewers = vec![OTHER.to_owned(), ME.to_owned()];
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::MeBlocking);
    assert_eq!(result.blocker, ME);
}

#[test]
fn multiple_others_is_needs_review_with_first_reviewer() {
    let mut pr = make_pr(AUTHOR);
    pr.requested_reviewers = vec!["dave".to_owned(), "eve".to_owned()];
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::NeedsReview);
    assert_eq!(result.blocker, "dave");
}

// ── Precedence ordering ────────────────────────────────────────────────────────

#[test]
fn ci_failing_beats_changes_requested_decision() {
    let mut pr = make_pr(AUTHOR);
    pr.review_decision = "CHANGES_REQUESTED".to_owned();
    let checks = [failed_check()];
    let result = compute_blocker(&pr, ME, &opts(&checks, &[], None));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert!(result.reason.to_lowercase().contains("ci"));
}

#[test]
fn changes_requested_review_beats_needs_review() {
    let mut pr = make_pr(AUTHOR);
    pr.requested_reviewers = vec![OTHER.to_owned()];
    let reviews = [review(OTHER, "CHANGES_REQUESTED")];
    let result = compute_blocker(&pr, ME, &opts(&[], &reviews, None));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
}

#[test]
fn me_blocking_beats_needs_review_when_current_user_also_requested() {
    let mut pr = make_pr(AUTHOR);
    pr.requested_reviewers = vec![ME.to_owned(), OTHER.to_owned()];
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::MeBlocking);
}

#[test]
fn changes_requested_by_other_beats_me_blocking() {
    let mut pr = make_pr(AUTHOR);
    pr.requested_reviewers = vec![ME.to_owned()];
    let reviews = [review(OTHER, "CHANGES_REQUESTED")];
    let result = compute_blocker(&pr, ME, &opts(&[], &reviews, None));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert_eq!(result.blocker, AUTHOR);
}

// ── Draft PRs ──────────────────────────────────────────────────────────────────

#[test]
fn draft_pr_is_waiting_on_author() {
    let mut pr = make_pr(AUTHOR);
    pr.is_draft = true;
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert_eq!(result.blocker, AUTHOR);
}

#[test]
fn draft_reason_mentions_draft() {
    let mut pr = make_pr(AUTHOR);
    pr.is_draft = true;
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert!(result.reason.to_lowercase().contains("draft"));
}

#[test]
fn draft_overrides_me_blocking() {
    let mut pr = make_pr(AUTHOR);
    pr.is_draft = true;
    pr.requested_reviewers = vec![ME.to_owned()];
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
}

#[test]
fn non_draft_pr_is_not_waiting_due_to_draft() {
    let pr = make_pr(AUTHOR);
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::NeedsReview);
}

#[test]
fn draft_pr_by_current_user_is_me_blocking() {
    let mut pr = make_pr(ME);
    pr.is_draft = true;
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::MeBlocking);
    assert_eq!(result.blocker, ME);
}

// ── Merge conflicts ─────────────────────────────────────────────────────────────

#[test]
fn conflicting_is_waiting_on_author() {
    let mut pr = make_pr(AUTHOR);
    pr.mergeable = "CONFLICTING".to_owned();
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert_eq!(result.blocker, AUTHOR);
}

#[test]
fn conflict_reason_mentions_conflict() {
    let mut pr = make_pr(AUTHOR);
    pr.mergeable = "CONFLICTING".to_owned();
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert!(result.reason.to_lowercase().contains("conflict"));
}

#[test]
fn conflict_overrides_me_blocking() {
    let mut pr = make_pr(AUTHOR);
    pr.mergeable = "CONFLICTING".to_owned();
    pr.requested_reviewers = vec![ME.to_owned()];
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
}

#[test]
fn mergeable_is_not_conflict_blocked() {
    let mut pr = make_pr(AUTHOR);
    pr.mergeable = "MERGEABLE".to_owned();
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::NeedsReview);
}

#[test]
fn unknown_mergeable_is_not_conflict_blocked() {
    let mut pr = make_pr(AUTHOR);
    pr.mergeable = "UNKNOWN".to_owned();
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::NeedsReview);
}

#[test]
fn ci_failing_takes_precedence_over_conflict() {
    let mut pr = make_pr(AUTHOR);
    pr.mergeable = "CONFLICTING".to_owned();
    let checks = [failed_check()];
    let result = compute_blocker(&pr, ME, &opts(&checks, &[], None));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert!(result.reason.to_lowercase().contains("ci"));
}

// ── Approved review decision ───────────────────────────────────────────────────

#[test]
fn approved_no_pending_reviewers_is_waiting_on_author() {
    let mut pr = make_pr(AUTHOR);
    pr.review_decision = "APPROVED".to_owned();
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert_eq!(result.blocker, AUTHOR);
}

#[test]
fn approved_with_pending_reviewer_is_still_waiting_on_author() {
    let mut pr = make_pr(AUTHOR);
    pr.review_decision = "APPROVED".to_owned();
    pr.requested_reviewers = vec![ME.to_owned()];
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert_eq!(result.blocker, AUTHOR);
}

#[test]
fn approved_overrides_needs_review() {
    let mut pr = make_pr(AUTHOR);
    pr.review_decision = "APPROVED".to_owned();
    pr.requested_reviewers = vec![OTHER.to_owned()];
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
}

#[test]
fn conflict_beats_approved() {
    let mut pr = make_pr(AUTHOR);
    pr.review_decision = "APPROVED".to_owned();
    pr.mergeable = "CONFLICTING".to_owned();
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert!(result.reason.to_lowercase().contains("conflict"));
}

#[test]
fn ci_failing_beats_approved() {
    let mut pr = make_pr(AUTHOR);
    pr.review_decision = "APPROVED".to_owned();
    let checks = [failed_check()];
    let result = compute_blocker(&pr, ME, &opts(&checks, &[], None));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert!(result.reason.to_lowercase().contains("ci"));
}

#[test]
fn unresolved_threads_beat_approved() {
    let mut pr = make_pr(AUTHOR);
    pr.review_decision = "APPROVED".to_owned();
    let threads = [thread(vec![comment(OTHER)])];
    let result = compute_blocker(&pr, ME, &opts(&[], &[], Some(&threads)));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert!(result.reason.to_lowercase().contains("thread"));
}

#[test]
fn individual_approved_review_counts_as_approved() {
    let pr = make_pr(AUTHOR);
    let reviews = [review(OTHER, "APPROVED")];
    let result = compute_blocker(&pr, ME, &opts(&[], &reviews, None));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert_eq!(result.blocker, AUTHOR);
}

#[test]
fn current_users_approved_review_removes_from_queue() {
    let mut pr = make_pr(AUTHOR);
    pr.requested_reviewers = vec![OTHER.to_owned()];
    let reviews = [review(ME, "APPROVED")];
    let result = compute_blocker(&pr, ME, &opts(&[], &reviews, None));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert_eq!(result.blocker, AUTHOR);
    assert!(result.reason.contains("Approved"));
}

#[test]
fn current_users_approved_with_awaiting_reviewer_threads_is_approved() {
    let pr = make_pr(AUTHOR);
    let reviews = [review(ME, "APPROVED")];
    let threads = [thread(vec![comment(ME), comment(AUTHOR)])];
    let result = compute_blocker(&pr, ME, &opts(&[], &reviews, Some(&threads)));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert_eq!(result.blocker, AUTHOR);
}

// ── Unresolved review threads ───────────────────────────────────────────────────

#[test]
fn unreplied_threads_are_waiting_on_author() {
    let pr = make_pr(AUTHOR);
    let threads = [thread(vec![comment(OTHER)]), thread(vec![comment(OTHER)])];
    let result = compute_blocker(&pr, ME, &opts(&[], &[], Some(&threads)));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert_eq!(result.blocker, AUTHOR);
    assert_eq!(result.reason, "2 unreplied threads");
}

#[test]
fn all_resolved_threads_have_no_effect() {
    let pr = make_pr(AUTHOR);
    let threads = [
        thread_with(vec![comment(OTHER)], true),
        thread_with(vec![comment(OTHER)], true),
    ];
    let result = compute_blocker(&pr, ME, &opts(&[], &[], Some(&threads)));
    assert_eq!(result.tier, Tier::NeedsReview);
}

#[test]
fn no_threads_provided_has_no_effect() {
    let pr = make_pr(AUTHOR);
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::NeedsReview);
}

#[test]
fn changes_requested_beats_unreplied_threads() {
    let mut pr = make_pr(AUTHOR);
    pr.review_decision = "CHANGES_REQUESTED".to_owned();
    let threads = [thread(vec![comment(OTHER)])];
    let result = compute_blocker(&pr, ME, &opts(&[], &[], Some(&threads)));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert!(result.reason.to_lowercase().contains("changes"));
}

#[test]
fn unreplied_threads_override_me_blocking() {
    let mut pr = make_pr(AUTHOR);
    pr.requested_reviewers = vec![ME.to_owned()];
    let threads = [thread(vec![comment(OTHER)])];
    let result = compute_blocker(&pr, ME, &opts(&[], &[], Some(&threads)));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert_eq!(result.blocker, AUTHOR);
}

#[test]
fn singular_unreplied_thread_grammar() {
    let pr = make_pr(AUTHOR);
    let threads = [thread(vec![comment(OTHER)])];
    let result = compute_blocker(&pr, ME, &opts(&[], &[], Some(&threads)));
    assert_eq!(result.reason, "1 unreplied thread");
}

#[test]
fn plural_unreplied_threads_grammar() {
    let pr = make_pr(AUTHOR);
    let threads = [thread(vec![comment(OTHER)]), thread(vec![comment(OTHER)])];
    let result = compute_blocker(&pr, ME, &opts(&[], &[], Some(&threads)));
    assert_eq!(result.reason, "2 unreplied threads");
}

// ── Edge cases ───────────────────────────────────────────────────────────────

#[test]
fn current_user_author_no_issues_is_needs_review() {
    let pr = make_pr(ME);
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::NeedsReview);
}

#[test]
fn current_user_author_ci_failing_is_me_blocking() {
    let pr = make_pr(ME);
    let checks = [failed_check()];
    let result = compute_blocker(&pr, ME, &opts(&checks, &[], None));
    assert_eq!(result.tier, Tier::MeBlocking);
    assert_eq!(result.blocker, ME);
}

#[test]
fn empty_checks_treated_as_no_ci_data() {
    let pr = make_pr(AUTHOR);
    let result = compute_blocker(&pr, ME, &opts(&[], &[], None));
    assert_eq!(result.tier, Tier::NeedsReview);
}

#[test]
fn skipped_neutral_checks_do_not_count_as_failing() {
    let pr = make_pr(AUTHOR);
    let checks = [
        check("skip", "completed", Some("skipped")),
        check("neutral", "completed", Some("neutral")),
    ];
    let result = compute_blocker(&pr, ME, &opts(&checks, &[], None));
    assert_ne!(result.tier, Tier::WaitingOnAuthor);
}

// ── Effective author (assignee takeover) ───────────────────────────────────────

#[test]
fn assignee_no_issues_is_needs_review() {
    let mut pr = make_pr(AUTHOR);
    pr.assignees = vec![ME.to_owned()];
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::NeedsReview);
}

#[test]
fn assignee_ci_failing_is_me_blocking() {
    let mut pr = make_pr(AUTHOR);
    pr.assignees = vec![ME.to_owned()];
    let checks = [failed_check()];
    let result = compute_blocker(&pr, ME, &opts(&checks, &[], None));
    assert_eq!(result.tier, Tier::MeBlocking);
    assert_eq!(result.blocker, ME);
    assert!(result.reason.contains("CI"));
}

#[test]
fn assignee_draft_is_me_blocking() {
    let mut pr = make_pr(AUTHOR);
    pr.assignees = vec![ME.to_owned()];
    pr.is_draft = true;
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::MeBlocking);
    assert_eq!(result.blocker, ME);
}

#[test]
fn assignee_merge_conflict_is_me_blocking() {
    let mut pr = make_pr(AUTHOR);
    pr.assignees = vec![ME.to_owned()];
    pr.mergeable = "CONFLICTING".to_owned();
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::MeBlocking);
    assert_eq!(result.blocker, ME);
}

#[test]
fn assignee_changes_requested_is_me_blocking() {
    let mut pr = make_pr(AUTHOR);
    pr.assignees = vec![ME.to_owned()];
    pr.review_decision = "CHANGES_REQUESTED".to_owned();
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::MeBlocking);
    assert_eq!(result.blocker, ME);
}

#[test]
fn assignee_approved_is_me_blocking() {
    let mut pr = make_pr(AUTHOR);
    pr.assignees = vec![ME.to_owned()];
    pr.review_decision = "APPROVED".to_owned();
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::MeBlocking);
    assert_eq!(result.blocker, ME);
    assert!(result.reason.contains("merge"));
}

#[test]
fn assignee_unreplied_threads_is_me_blocking() {
    let mut pr = make_pr(AUTHOR);
    pr.assignees = vec![ME.to_owned()];
    let threads = [thread(vec![comment(OTHER)])];
    let result = compute_blocker(&pr, ME, &opts(&[], &[], Some(&threads)));
    assert_eq!(result.tier, Tier::MeBlocking);
    assert_eq!(result.blocker, ME);
}

#[test]
fn both_user_and_author_assigned_keeps_author() {
    let mut pr = make_pr(AUTHOR);
    pr.assignees = vec![ME.to_owned(), AUTHOR.to_owned()];
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::NeedsReview);
}

#[test]
fn both_user_and_author_assigned_ci_failing_is_waiting_on_author() {
    let mut pr = make_pr(AUTHOR);
    pr.assignees = vec![ME.to_owned(), AUTHOR.to_owned()];
    let checks = [failed_check()];
    let result = compute_blocker(&pr, ME, &opts(&checks, &[], None));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert_eq!(result.blocker, AUTHOR);
}

#[test]
fn another_user_assigned_has_no_effect() {
    let mut pr = make_pr(AUTHOR);
    pr.assignees = vec![OTHER.to_owned()];
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::NeedsReview);
}

#[test]
fn no_assignees_uses_normal_rules() {
    let pr = make_pr(AUTHOR);
    let result = compute_blocker(&pr, ME, &BlockerOptions::default());
    assert_eq!(result.tier, Tier::NeedsReview);
}

#[test]
fn user_assigned_alongside_other_non_author_is_effective_author() {
    let mut pr = make_pr(AUTHOR);
    pr.assignees = vec![ME.to_owned(), OTHER.to_owned()];
    let checks = [failed_check()];
    let result = compute_blocker(&pr, ME, &opts(&checks, &[], None));
    assert_eq!(result.tier, Tier::MeBlocking);
    assert_eq!(result.blocker, ME);
}

#[test]
fn result_always_has_non_empty_reason() {
    let mut with_reviewer = make_pr(AUTHOR);
    with_reviewer.requested_reviewers = vec![ME.to_owned()];
    let mut changes = make_pr(AUTHOR);
    changes.review_decision = "CHANGES_REQUESTED".to_owned();
    let mut other_reviewer = make_pr(AUTHOR);
    other_reviewer.requested_reviewers = vec![OTHER.to_owned()];
    let cases = [with_reviewer, make_pr(AUTHOR), changes, other_reviewer];
    for pr in &cases {
        let result = compute_blocker(pr, ME, &BlockerOptions::default());
        assert!(!result.reason.is_empty());
    }
}

// ── classify_threads ───────────────────────────────────────────────────────────

#[test]
fn classify_resolved_threads_ignored() {
    let threads = [thread_with(vec![comment(OTHER), comment(AUTHOR)], true)];
    let result = classify_threads(&threads);
    assert_eq!(result.unreplied, 0);
    assert_eq!(result.awaiting_reviewer, 0);
}

#[test]
fn classify_only_reviewer_comment_is_unreplied() {
    let threads = [thread(vec![comment(OTHER)])];
    let result = classify_threads(&threads);
    assert_eq!(result.unreplied, 1);
    assert_eq!(result.awaiting_reviewer, 0);
}

#[test]
fn classify_author_replied_last_is_awaiting_reviewer() {
    let threads = [thread(vec![comment(OTHER), comment(AUTHOR)])];
    let result = classify_threads(&threads);
    assert_eq!(result.unreplied, 0);
    assert_eq!(result.awaiting_reviewer, 1);
    let entry = result
        .awaiting_by_reviewer
        .iter()
        .find(|e| e.reviewer == OTHER)
        .expect("OTHER tracked");
    assert_eq!(entry.count, 1);
}

#[test]
fn classify_ping_pong_reviewer_last_is_unreplied() {
    let threads = [thread(vec![
        comment(OTHER),
        comment(AUTHOR),
        comment(OTHER),
    ])];
    let result = classify_threads(&threads);
    assert_eq!(result.unreplied, 1);
    assert_eq!(result.awaiting_reviewer, 0);
}

#[test]
fn classify_bot_after_author_does_not_flip_to_unreplied() {
    let threads = [thread(vec![
        comment(OTHER),
        comment(AUTHOR),
        bot_comment("github-actions[bot]"),
    ])];
    let result = classify_threads(&threads);
    assert_eq!(result.unreplied, 0);
    assert_eq!(result.awaiting_reviewer, 1);
}

#[test]
fn classify_only_bot_comments_is_unreplied() {
    let threads = [thread(vec![bot_comment("bot1"), bot_comment("bot2")])];
    let result = classify_threads(&threads);
    assert_eq!(result.unreplied, 1);
    assert_eq!(result.awaiting_reviewer, 0);
}

#[test]
fn classify_empty_thread_is_skipped() {
    let threads = [thread(vec![])];
    let result = classify_threads(&threads);
    assert_eq!(result.unreplied, 0);
    assert_eq!(result.awaiting_reviewer, 0);
}

#[test]
fn classify_multiple_reviewers_tracked_separately() {
    let threads = [
        thread(vec![comment(OTHER), comment(AUTHOR)]),
        thread(vec![comment("dave"), comment(AUTHOR)]),
        thread(vec![comment(OTHER), comment(AUTHOR)]),
    ];
    let result = classify_threads(&threads);
    assert_eq!(result.awaiting_reviewer, 3);
    let bob = result
        .awaiting_by_reviewer
        .iter()
        .find(|e| e.reviewer == OTHER);
    let dave = result
        .awaiting_by_reviewer
        .iter()
        .find(|e| e.reviewer == "dave");
    assert_eq!(bob.map(|e| e.count), Some(2));
    assert_eq!(dave.map(|e| e.count), Some(1));
}

#[test]
fn classify_mixed_unreplied_and_awaiting() {
    let threads = [
        thread(vec![comment(OTHER)]),
        thread(vec![comment(OTHER), comment(AUTHOR)]),
    ];
    let result = classify_threads(&threads);
    assert_eq!(result.unreplied, 1);
    assert_eq!(result.awaiting_reviewer, 1);
}

#[test]
fn classify_tracks_oldest_reply_date() {
    let threads = [
        thread(vec![
            comment(OTHER),
            comment_at(AUTHOR, "2026-03-10T00:00:00Z"),
        ]),
        thread(vec![
            comment(OTHER),
            comment_at(AUTHOR, "2026-03-12T00:00:00Z"),
        ]),
    ];
    let result = classify_threads(&threads);
    let bob = result
        .awaiting_by_reviewer
        .iter()
        .find(|e| e.reviewer == OTHER)
        .expect("OTHER tracked");
    assert_eq!(bob.oldest_reply_date, date("2026-03-10T00:00:00Z"));
}

// ── classify_thread (single) ────────────────────────────────────────────────

#[test]
fn classify_thread_resolved() {
    let t = thread_with(vec![comment(OTHER), comment(AUTHOR)], true);
    assert_eq!(classify_thread(&t), ThreadKind::Resolved);
}

#[test]
fn classify_thread_unreplied_when_only_starter() {
    let t = thread(vec![comment(OTHER)]);
    assert_eq!(classify_thread(&t), ThreadKind::Unreplied);
}

#[test]
fn classify_thread_awaiting_reviewer_when_other_replied_last() {
    let t = thread(vec![comment(OTHER), comment(AUTHOR)]);
    assert_eq!(classify_thread(&t), ThreadKind::AwaitingReviewer);
}

#[test]
fn classify_thread_unreplied_when_starter_replied_last() {
    let t = thread(vec![comment(OTHER), comment(AUTHOR), comment(OTHER)]);
    assert_eq!(classify_thread(&t), ThreadKind::Unreplied);
}

#[test]
fn classify_thread_ignores_trailing_bot_comment() {
    let t = thread(vec![
        comment(OTHER),
        comment(AUTHOR),
        bot_comment("github-actions[bot]"),
    ]);
    assert_eq!(classify_thread(&t), ThreadKind::AwaitingReviewer);
}

#[test]
fn classify_thread_only_bots_is_unreplied() {
    let t = thread(vec![bot_comment("bot1"), bot_comment("bot2")]);
    assert_eq!(classify_thread(&t), ThreadKind::Unreplied);
}

#[test]
fn classify_thread_empty_is_unreplied() {
    let t = thread(vec![]);
    assert_eq!(classify_thread(&t), ThreadKind::Unreplied);
}

// ── compute_blocker: unreplied vs awaiting-reviewer ─────────────────────────────

#[test]
fn unreplied_threads_singular_via_compute() {
    let pr = make_pr(AUTHOR);
    let threads = [thread(vec![comment(OTHER)])];
    let result = compute_blocker(&pr, ME, &opts(&[], &[], Some(&threads)));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert_eq!(result.blocker, AUTHOR);
    assert_eq!(result.reason, "1 unreplied thread");
}

#[test]
fn plural_unreplied_threads_via_compute() {
    let pr = make_pr(AUTHOR);
    let threads = [thread(vec![comment(OTHER)]), thread(vec![comment("dave")])];
    let result = compute_blocker(&pr, ME, &opts(&[], &[], Some(&threads)));
    assert_eq!(result.reason, "2 unreplied threads");
}

#[test]
fn all_awaiting_reviewer_is_needs_review_with_reviewer_blocker() {
    let pr = make_pr(AUTHOR);
    let threads = [thread(vec![comment(OTHER), comment(AUTHOR)])];
    let result = compute_blocker(&pr, ME, &opts(&[], &[], Some(&threads)));
    assert_eq!(result.tier, Tier::NeedsReview);
    assert_eq!(result.blocker, OTHER);
    assert_eq!(result.reason, format!("1 thread awaiting {OTHER}"));
}

#[test]
fn awaiting_reviewer_with_current_user_is_me_blocking() {
    let pr = make_pr(AUTHOR);
    let threads = [thread(vec![comment(ME), comment(AUTHOR)])];
    let result = compute_blocker(&pr, ME, &opts(&[], &[], Some(&threads)));
    assert_eq!(result.tier, Tier::MeBlocking);
    assert_eq!(result.blocker, ME);
    assert_eq!(result.reason, format!("1 thread awaiting {ME}"));
}

#[test]
fn mixed_unreplied_and_awaiting_is_waiting_on_author() {
    let pr = make_pr(AUTHOR);
    let threads = [
        thread(vec![comment(OTHER)]),
        thread(vec![comment(OTHER), comment(AUTHOR)]),
    ];
    let result = compute_blocker(&pr, ME, &opts(&[], &[], Some(&threads)));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert_eq!(result.blocker, AUTHOR);
    assert_eq!(result.reason, "1 unreplied thread");
}

#[test]
fn approved_beats_awaiting_reviewer() {
    let mut pr = make_pr(AUTHOR);
    pr.review_decision = "APPROVED".to_owned();
    let threads = [thread(vec![comment(OTHER), comment(AUTHOR)])];
    let result = compute_blocker(&pr, ME, &opts(&[], &[], Some(&threads)));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert_eq!(result.blocker, AUTHOR);
    assert!(result.reason.contains("Approved"));
}

#[test]
fn unreplied_threads_beat_approved_via_compute() {
    let mut pr = make_pr(AUTHOR);
    pr.review_decision = "APPROVED".to_owned();
    let threads = [thread(vec![comment(OTHER)])];
    let result = compute_blocker(&pr, ME, &opts(&[], &[], Some(&threads)));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert!(result.reason.contains("unreplied"));
}

#[test]
fn changes_requested_beats_unreplied_threads_via_compute() {
    let mut pr = make_pr(AUTHOR);
    pr.review_decision = "CHANGES_REQUESTED".to_owned();
    let threads = [thread(vec![comment(OTHER)])];
    let result = compute_blocker(&pr, ME, &opts(&[], &[], Some(&threads)));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert!(result.reason.contains("Changes"));
}

#[test]
fn ci_failing_beats_unreplied_threads_via_compute() {
    let pr = make_pr(AUTHOR);
    let threads = [thread(vec![comment(OTHER)])];
    let checks = [failed_check()];
    let result = compute_blocker(&pr, ME, &opts(&checks, &[], Some(&threads)));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert!(result.reason.contains("CI"));
}

#[test]
fn empty_threads_array_falls_through() {
    let pr = make_pr(AUTHOR);
    let threads: [FullReviewThread; 0] = [];
    let result = compute_blocker(&pr, ME, &opts(&[], &[], Some(&threads)));
    assert_eq!(result.tier, Tier::NeedsReview);
}

#[test]
fn all_resolved_threads_no_thread_blocking() {
    let pr = make_pr(AUTHOR);
    let threads = [thread_with(vec![comment(OTHER), comment(AUTHOR)], true)];
    let result = compute_blocker(&pr, ME, &opts(&[], &[], Some(&threads)));
    assert_eq!(result.tier, Tier::NeedsReview);
}

#[test]
fn reviewer_with_most_awaiting_threads_is_blocker() {
    let pr = make_pr(AUTHOR);
    let threads = [
        thread(vec![comment(OTHER), comment(AUTHOR)]),
        thread(vec![comment(OTHER), comment(AUTHOR)]),
        thread(vec![comment("dave"), comment(AUTHOR)]),
    ];
    let result = compute_blocker(&pr, ME, &opts(&[], &[], Some(&threads)));
    assert_eq!(result.blocker, OTHER);
    assert_eq!(result.reason, format!("3 threads awaiting {OTHER}"));
}

#[test]
fn tie_break_reviewer_waiting_longest_is_blocker() {
    let pr = make_pr(AUTHOR);
    let threads = [
        thread(vec![
            comment(OTHER),
            comment_at(AUTHOR, "2026-03-12T00:00:00Z"),
        ]),
        thread(vec![
            comment("dave"),
            comment_at(AUTHOR, "2026-03-10T00:00:00Z"),
        ]),
    ];
    let result = compute_blocker(&pr, ME, &opts(&[], &[], Some(&threads)));
    // dave has been waiting since 03-10, bob since 03-12 -> dave wins the tie.
    assert_eq!(result.blocker, "dave");
}

#[test]
fn effective_author_with_unreplied_threads_is_me_blocking() {
    let mut pr = make_pr(AUTHOR);
    pr.assignees = vec![ME.to_owned()];
    let threads = [thread(vec![comment(OTHER)])];
    let result = compute_blocker(&pr, ME, &opts(&[], &[], Some(&threads)));
    assert_eq!(result.tier, Tier::MeBlocking);
    assert_eq!(result.blocker, ME);
}

#[test]
fn pr_568_one_unreplied_thread_still_waiting_on_author() {
    let pr = make_pr("mayfieldiv");
    let threads = [
        thread(vec![
            comment_at("cmbankester", "2026-04-01T00:00:00Z"),
            comment_at("cmbankester", "2026-04-02T00:00:00Z"),
            comment_at("mayfieldiv", "2026-04-02T12:00:00Z"),
        ]),
        thread(vec![comment_at("cmbankester", "2026-04-02T00:00:00Z")]),
    ];
    let result = compute_blocker(&pr, "someuser", &opts(&[], &[], Some(&threads)));
    assert_eq!(result.tier, Tier::WaitingOnAuthor);
    assert_eq!(result.blocker, "mayfieldiv");
    assert_eq!(result.reason, "1 unreplied thread");
}

#[test]
fn pr_568_all_replied_is_awaiting_reviewer() {
    let pr = make_pr("mayfieldiv");
    let threads = [
        thread(vec![
            comment("cmbankester"),
            comment("cmbankester"),
            comment("mayfieldiv"),
        ]),
        thread(vec![comment("cmbankester"), comment("mayfieldiv")]),
    ];
    let result = compute_blocker(&pr, "someuser", &opts(&[], &[], Some(&threads)));
    assert_eq!(result.tier, Tier::NeedsReview);
    assert_eq!(result.blocker, "cmbankester");
    assert_eq!(result.reason, "2 threads awaiting cmbankester");
}

// ── Tier metadata ──────────────────────────────────────────────────────────────

#[test]
fn tier_labels_are_headings() {
    assert_eq!(Tier::MeBlocking.label(), "Me blocking");
    assert_eq!(Tier::NeedsReview.label(), "Needs review");
    assert_eq!(Tier::WaitingOnAuthor.label(), "Waiting on author");
}

#[test]
fn tier_order_is_me_blocking_then_needs_review_then_waiting() {
    assert!(Tier::MeBlocking.order() < Tier::NeedsReview.order());
    assert!(Tier::NeedsReview.order() < Tier::WaitingOnAuthor.order());
}
