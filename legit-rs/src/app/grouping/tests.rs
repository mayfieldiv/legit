//! Unit tests for the pure grouping layer. Mirror the smart-status / repo /
//! none coverage of `tests/group-filter-engine.test.ts` that issue #46 needs.

use super::{DisplayRow, Grouping, display_rows};
use crate::blocker::Tier;

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
    let rows = display_rows(3, Grouping::None, |_| None, "acme/web");
    assert!(headers(&rows).is_empty());
    assert_eq!(pr_indices(&rows), vec![0, 1, 2]);
}

#[test]
fn none_empty_list_is_empty() {
    let rows = display_rows(0, Grouping::None, |_| None, "acme/web");
    assert!(rows.is_empty());
}

// ── grouping: repo ─────────────────────────────────────────────────────────────

#[test]
fn repo_groups_under_single_slug_header() {
    let rows = display_rows(2, Grouping::Repo, |_| None, "acme/web");
    assert_eq!(headers(&rows), vec!["acme/web"]);
    assert_eq!(pr_indices(&rows), vec![0, 1]);
}

#[test]
fn repo_with_no_slug_falls_back_to_unknown() {
    let rows = display_rows(1, Grouping::Repo, |_| None, "");
    assert_eq!(headers(&rows), vec!["unknown"]);
}

#[test]
fn repo_empty_list_is_empty() {
    let rows = display_rows(0, Grouping::Repo, |_| None, "acme/web");
    assert!(rows.is_empty());
}

// ── grouping: smart-status ─────────────────────────────────────────────────────

#[test]
fn smart_status_orders_tiers_me_blocking_needs_review_waiting() {
    // index 0: waiting, 1: needs-review, 2: me-blocking, 3: needs-review
    let tiers = [
        Tier::WaitingOnAuthor,
        Tier::NeedsReview,
        Tier::MeBlocking,
        Tier::NeedsReview,
    ];
    let rows = display_rows(4, Grouping::SmartStatus, |i| Some(tiers[i]), "acme/web");
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
    let rows = display_rows(2, Grouping::SmartStatus, |_| Some(Tier::NeedsReview), "r");
    assert_eq!(headers(&rows), vec!["Needs review"]);
}

#[test]
fn smart_status_single_tier_list() {
    let rows = display_rows(3, Grouping::SmartStatus, |_| Some(Tier::MeBlocking), "r");
    assert_eq!(headers(&rows), vec!["Me blocking"]);
    assert_eq!(pr_indices(&rows), vec![0, 1, 2]);
}

#[test]
fn smart_status_undelivered_tiers_collect_under_loading() {
    // index 0 derived, 1 and 2 still loading.
    let rows = display_rows(
        3,
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
    let rows = display_rows(0, Grouping::SmartStatus, |_| None, "r");
    assert!(rows.is_empty());
}

#[test]
fn smart_status_preserves_input_order_within_tier() {
    // Indices stay in input order within a single tier.
    let rows = display_rows(3, Grouping::SmartStatus, |_| Some(Tier::NeedsReview), "r");
    assert_eq!(pr_indices(&rows), vec![0, 1, 2]);
}
