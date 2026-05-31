//! Unit tests for the pure grouping layer. Mirror the smart-status / repo /
//! none coverage of `tests/group-filter-engine.test.ts` that issue #46 needs.

use chrono::{TimeZone, Utc};

use super::{DisplayRow, Grouping, display_rows};
use crate::blocker::Tier;
use crate::github::rest::{PR, PRState};

fn pr(number: u64) -> PR {
    PR {
        number,
        title: format!("PR #{number}"),
        author: "octocat".to_owned(),
        created_at: Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap(),
        updated_at: Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap(),
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
        head_ref: format!("feat/{number}"),
        base_ref: "main".to_owned(),
        head_repository_owner: "mayfieldiv".to_owned(),
        state: PRState::Open,
    }
}

/// Headers in display order.
fn headers(rows: &[DisplayRow]) -> Vec<&str> {
    rows.iter()
        .filter_map(|r| match r {
            DisplayRow::Header(label) => Some(label.as_str()),
            DisplayRow::Pr(_) => None,
        })
        .collect()
}

/// PR indices in display order.
fn pr_indices(rows: &[DisplayRow]) -> Vec<usize> {
    rows.iter()
        .filter_map(|r| match r {
            DisplayRow::Pr(i) => Some(*i),
            DisplayRow::Header(_) => None,
        })
        .collect()
}

// ── Grouping::next cycles ──────────────────────────────────────────────────────

#[test]
fn grouping_cycles_smart_status_repo_none() {
    assert_eq!(Grouping::SmartStatus.next(), Grouping::Repo);
    assert_eq!(Grouping::Repo.next(), Grouping::None);
    assert_eq!(Grouping::None.next(), Grouping::SmartStatus);
}

#[test]
fn default_grouping_is_smart_status() {
    assert_eq!(Grouping::default(), Grouping::SmartStatus);
}

// ── grouping: none ─────────────────────────────────────────────────────────────

#[test]
fn none_emits_one_row_per_pr_no_headers() {
    let prs = [pr(1), pr(2), pr(3)];
    let rows = display_rows(&prs, Grouping::None, |_| None, "acme/web");
    assert!(headers(&rows).is_empty());
    assert_eq!(pr_indices(&rows), vec![0, 1, 2]);
}

#[test]
fn none_empty_list_is_empty() {
    let rows = display_rows(&[], Grouping::None, |_| None, "acme/web");
    assert!(rows.is_empty());
}

// ── grouping: repo ─────────────────────────────────────────────────────────────

#[test]
fn repo_groups_under_single_slug_header() {
    let prs = [pr(1), pr(2)];
    let rows = display_rows(&prs, Grouping::Repo, |_| None, "acme/web");
    assert_eq!(headers(&rows), vec!["acme/web"]);
    assert_eq!(pr_indices(&rows), vec![0, 1]);
}

#[test]
fn repo_with_no_slug_falls_back_to_unknown() {
    let prs = [pr(1)];
    let rows = display_rows(&prs, Grouping::Repo, |_| None, "");
    assert_eq!(headers(&rows), vec!["unknown"]);
}

#[test]
fn repo_empty_list_is_empty() {
    let rows = display_rows(&[], Grouping::Repo, |_| None, "acme/web");
    assert!(rows.is_empty());
}

// ── grouping: smart-status ─────────────────────────────────────────────────────

#[test]
fn smart_status_orders_tiers_me_blocking_needs_review_waiting() {
    // index 0: waiting, 1: needs-review, 2: me-blocking, 3: needs-review
    let prs = [pr(1), pr(2), pr(3), pr(4)];
    let tiers = [
        Tier::WaitingOnAuthor,
        Tier::NeedsReview,
        Tier::MeBlocking,
        Tier::NeedsReview,
    ];
    let rows = display_rows(&prs, Grouping::SmartStatus, |i| Some(tiers[i]), "acme/web");
    assert_eq!(
        headers(&rows),
        vec!["Me blocking", "Needs review", "Waiting on author"]
    );
    // Me blocking: [2], Needs review: [1, 3], Waiting: [0]
    assert_eq!(pr_indices(&rows), vec![2, 1, 3, 0]);
}

#[test]
fn smart_status_omits_empty_tiers() {
    // All needs-review.
    let prs = [pr(1), pr(2)];
    let rows = display_rows(
        &prs,
        Grouping::SmartStatus,
        |_| Some(Tier::NeedsReview),
        "r",
    );
    assert_eq!(headers(&rows), vec!["Needs review"]);
}

#[test]
fn smart_status_single_tier_list() {
    let prs = [pr(1), pr(2), pr(3)];
    let rows = display_rows(&prs, Grouping::SmartStatus, |_| Some(Tier::MeBlocking), "r");
    assert_eq!(headers(&rows), vec!["Me blocking"]);
    assert_eq!(pr_indices(&rows), vec![0, 1, 2]);
}

#[test]
fn smart_status_undelivered_tiers_collect_under_loading() {
    let prs = [pr(1), pr(2), pr(3)];
    // index 0 derived, 1 and 2 still loading.
    let rows = display_rows(
        &prs,
        Grouping::SmartStatus,
        |i| {
            if i == 0 {
                Some(Tier::NeedsReview)
            } else {
                None
            }
        },
        "r",
    );
    assert_eq!(headers(&rows), vec!["Needs review", "Loading details…"]);
    assert_eq!(pr_indices(&rows), vec![0, 1, 2]);
}

#[test]
fn smart_status_empty_list_is_empty() {
    let rows = display_rows(&[], Grouping::SmartStatus, |_| None, "r");
    assert!(rows.is_empty());
}

#[test]
fn smart_status_preserves_input_order_within_tier() {
    let prs = [pr(10), pr(20), pr(30)];
    let rows = display_rows(
        &prs,
        Grouping::SmartStatus,
        |_| Some(Tier::NeedsReview),
        "r",
    );
    // Indices stay in input order; the PR numbers map 0->10, 1->20, 2->30.
    assert_eq!(pr_indices(&rows), vec![0, 1, 2]);
}
