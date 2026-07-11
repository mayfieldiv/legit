use chrono::TimeZone;

use super::{DisplayRow, Grouping, PrList};
use crate::blocker::Tier;
use crate::github::rest::PR;
use crate::github::types::PRState;

fn sample_pr(number: u64) -> PR {
    PR {
        number,
        repo_slug: "owner/repo".to_owned(),
        title: format!("PR #{number}"),
        author: "octocat".to_owned(),
        created_at: chrono::Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap(),
        updated_at: chrono::Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap(),
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
        review_status_loaded: false,
        head_ref: format!("feature/{number}"),
        base_ref: "main".to_owned(),
        head_repository_owner: "mayfieldiv".to_owned(),
        state: PRState::Open,
    }
}

/// Build a list with `n` PRs, laid out flat (no grouping) so navigation and
/// scroll tests exercise the row mechanics without headers in the way.
fn flat_list(n: u64) -> PrList {
    let mut list = PrList::new();
    for i in 1..=n {
        list.push(sample_pr(i));
    }
    list.grouping = Grouping::None;
    list.relayout(None, |_| None);
    list
}

/// PR indices among the currently visible display rows.
fn visible_pr_indices(list: &PrList) -> Vec<usize> {
    list.visible_rows()
        .filter_map(|(row, _)| match row {
            DisplayRow::Pr(i) => Some(*i),
            DisplayRow::Header(_) => None,
        })
        .collect()
}

#[test]
fn pushed_pr_appears_in_the_list() {
    let mut list = PrList::new();

    list.push(sample_pr(42));

    assert_eq!(list.prs().len(), 1);
    assert_eq!(list.prs()[0].number, 42);
}

#[test]
fn new_list_has_no_fetch_in_flight_and_no_failure() {
    let list = PrList::new();
    assert!(!list.is_loading(None));
    assert_eq!(list.failure(), None);
}

#[test]
fn new_list_defaults_to_smart_status_grouping() {
    let list = PrList::new();
    assert_eq!(list.grouping(), Grouping::SmartStatus);
}

#[test]
fn smart_status_groups_order_prs_by_most_recent_github_activity() {
    let mut list = PrList::new();
    for (number, day) in [(1, 1), (2, 3), (3, 2)] {
        let mut pr = sample_pr(number);
        pr.updated_at = chrono::Utc.with_ymd_and_hms(2026, 5, day, 0, 0, 0).unwrap();
        list.push(pr);
    }

    list.relayout(None, |_| Some(Tier::NeedsReview));

    assert_eq!(list.pr_numbers_in_display_order(), vec![2, 3, 1]);
}

#[test]
fn equal_activity_times_put_the_newer_pr_first() {
    let mut list = PrList::new();
    for (number, created_day) in [(1, 1), (2, 2)] {
        let mut pr = sample_pr(number);
        pr.created_at = chrono::Utc
            .with_ymd_and_hms(2026, 5, created_day, 0, 0, 0)
            .unwrap();
        list.push(pr);
    }

    list.relayout(None, |_| Some(Tier::NeedsReview));

    assert_eq!(list.pr_numbers_in_display_order(), vec![2, 1]);
}

#[test]
fn untouched_selection_follows_the_top_row_as_arrivals_resort() {
    let mut list = PrList::new();
    list.push(sample_pr(1));
    list.relayout(None, |_| Some(Tier::NeedsReview));
    assert_eq!(list.prs()[list.selected()].number, 1);

    // A more recently active PR arrives and sorts above the first-streamed
    // one; the never-touched cursor follows the top row.
    let mut newer = sample_pr(2);
    newer.updated_at = chrono::Utc.with_ymd_and_hms(2026, 5, 2, 0, 0, 0).unwrap();
    list.push(newer);
    list.relayout(None, |_| Some(Tier::NeedsReview));
    assert_eq!(
        list.prs()[list.selected()].number,
        2,
        "the default selection follows the top row"
    );

    // Once the user navigates, the selection sticks to its PR instead.
    list.move_down();
    assert_eq!(list.prs()[list.selected()].number, 1);
    let mut newest = sample_pr(3);
    newest.updated_at = chrono::Utc.with_ymd_and_hms(2026, 5, 3, 0, 0, 0).unwrap();
    list.push(newest);
    list.relayout(None, |_| Some(Tier::NeedsReview));
    assert_eq!(
        list.prs()[list.selected()].number,
        1,
        "a user-chosen selection sticks through re-sorts"
    );
}

#[test]
fn wheel_up_on_empty_list_does_not_detach_default_selection() {
    let mut list = PrList::new();
    list.resize(10);
    list.relayout(None, |_| None);

    list.scroll_up(3);

    // PRs stream in oldest-first; the second sorts above the first. A
    // wheel-up over the empty list must not have pinned the selection to
    // the first arrival.
    list.push(sample_pr(1));
    list.relayout(None, |_| Some(Tier::NeedsReview));
    let mut newer = sample_pr(2);
    newer.updated_at = chrono::Utc.with_ymd_and_hms(2026, 5, 2, 0, 0, 0).unwrap();
    list.push(newer);
    list.relayout(None, |_| Some(Tier::NeedsReview));

    assert_eq!(
        list.prs()[list.selected()].number,
        2,
        "the default selection still follows the top row"
    );
}

#[test]
fn merge_listed_reports_whether_the_pool_changed() {
    let mut list = PrList::new();
    list.begin_fetch("owner/repo");
    assert!(list.merge_listed(sample_pr(1)), "a new PR changes the pool");
    assert!(
        !list.merge_listed(sample_pr(1)),
        "an identical re-stream changes nothing"
    );

    // A listing-level change (retitle, draft flip) updates the survivor.
    let mut retitled = sample_pr(1);
    retitled.title = "New title".to_owned();
    retitled.is_draft = true;
    assert!(
        list.merge_listed(retitled),
        "a survivor taking fresh fields changes the pool"
    );
    assert_eq!(list.prs()[0].title, "New title");
    assert!(list.prs()[0].is_draft);
}

#[test]
fn begin_fetch_marks_only_that_repo_loading() {
    let mut list = PrList::new();
    list.begin_fetch("acme/web");

    assert!(list.is_loading(None), "any-repo scope sees the fetch");
    assert!(list.is_loading(Some("acme/web")));
    assert!(
        !list.is_loading(Some("acme/api")),
        "an untouched repo is not loading"
    );
}

#[test]
fn complete_fetch_clears_loading_for_that_repo_only() {
    let mut list = PrList::new();
    list.begin_fetch("acme/web");
    list.begin_fetch("acme/api");

    list.complete_fetch("acme/web");

    assert!(!list.is_loading(Some("acme/web")));
    assert!(list.is_loading(Some("acme/api")));
    assert!(list.is_loading(None), "another repo is still in flight");
    assert_eq!(list.phase_of("acme/web"), Some(&super::Phase::Loaded));
}

#[test]
fn move_down_advances_selection_within_bounds() {
    let mut list = flat_list(3);

    list.move_down();
    assert_eq!(list.selected(), 1);
    list.move_down();
    list.move_down();
    list.move_down();
    // Last PR is index 2; further moves clamp.
    assert_eq!(list.selected(), 2);
}

#[test]
fn move_up_retreats_selection_and_clamps_at_zero() {
    let mut list = flat_list(3);
    list.move_down();
    list.move_down();
    assert_eq!(list.selected(), 2);

    list.move_up();
    list.move_up();
    list.move_up();
    assert_eq!(list.selected(), 0);
}

#[test]
fn navigation_skips_group_headers() {
    // Two tiers: me-blocking (PR #1) and waiting-on-author (PR #2).
    // Layout: [Header, Pr(0), Header, Pr(1)]. j must step Pr(0) -> Pr(1).
    let mut list = PrList::new();
    list.push(sample_pr(1));
    list.push(sample_pr(2));
    list.relayout(None, |pr| {
        Some(if pr.number == 1 {
            Tier::MeBlocking
        } else {
            Tier::WaitingOnAuthor
        })
    });

    assert_eq!(list.selected(), 0);
    list.move_down();
    assert_eq!(list.selected(), 1, "j steps over the second group's header");
    list.move_up();
    assert_eq!(list.selected(), 0, "k steps back over the header");
}

#[test]
fn cycle_grouping_advances_mode_and_resets_selection() {
    let mut list = flat_list(3);
    list.move_down();
    list.move_down();
    assert_eq!(list.selected(), 2);

    // flat_list set grouping to None; cycling wraps None -> SmartStatus.
    list.cycle_grouping();
    assert_eq!(list.grouping(), Grouping::SmartStatus);
    assert_eq!(list.selected(), 0, "selection resets on regroup");
}

#[test]
fn visible_rows_yields_window_starting_at_scroll_offset() {
    let mut list = flat_list(20);
    list.resize(5);
    for _ in 0..10 {
        list.move_down();
    }
    let offset = list.scroll_offset();

    let indices = visible_pr_indices(&list);

    assert_eq!(indices.len(), 5);
    // Flat layout: display row N is PR index N, so the first visible PR
    // index equals the scroll offset.
    assert_eq!(indices[0], offset);
    assert_eq!(indices[4], offset + 4);
}

#[test]
fn visible_rows_caps_at_list_length_when_window_extends_past_end() {
    let mut list = flat_list(3);
    list.resize(10);

    let count = list.visible_rows().count();

    assert_eq!(
        count, 3,
        "viewport is larger than list; should yield all rows"
    );
}

#[test]
fn moving_below_bottom_margin_advances_scroll() {
    let mut list = flat_list(20);
    list.resize(10);

    for _ in 0..9 {
        list.move_down();
    }

    assert!(
        list.scroll_offset() >= 1,
        "scroll should advance into the bottom margin, got {}",
        list.scroll_offset(),
    );
}

#[test]
fn shrinking_viewport_re_clamps_scroll_to_keep_selection_visible() {
    let mut list = flat_list(30);
    list.resize(20);
    for _ in 0..25 {
        list.move_down();
    }
    let selected_row = list.selected(); // flat: row == index
    assert!(selected_row < list.scroll_offset() + 20);

    list.resize(5);

    assert!(
        list.selected() >= list.scroll_offset() && list.selected() < list.scroll_offset() + 5,
        "selection {} must stay within window {}..{} after shrink",
        list.selected(),
        list.scroll_offset(),
        list.scroll_offset() + 5,
    );
}

#[test]
fn single_row_viewport_keeps_selection_visible() {
    let mut list = flat_list(10);
    list.resize(1);

    // At viewport_height = 1 the margin must collapse to 0, otherwise the
    // top and bottom margins are jointly unsatisfiable and the selected row
    // scrolls out of the single visible line.
    for _ in 0..5 {
        list.move_down();
    }

    // Flat layout: selected PR index == its display row.
    assert_eq!(
        list.scroll_offset(),
        list.selected(),
        "the only visible row must be the selected one",
    );
}

#[test]
fn fail_fetch_records_failure_without_masking_other_repos() {
    let mut list = PrList::new();
    list.begin_fetch("acme/web");
    list.begin_fetch("acme/api");

    list.fail_fetch("acme/web", "network down".to_owned());

    assert_eq!(list.failure(), Some("network down"));
    assert!(
        list.is_loading(Some("acme/api")),
        "the other repo's fetch keeps going"
    );
    assert!(!list.is_loading(Some("acme/web")));
}

#[test]
fn failure_reports_first_failed_repo_in_slug_order() {
    let mut list = PrList::new();
    list.fail_fetch("zeta/repo", "zeta down".to_owned());
    list.fail_fetch("acme/web", "acme down".to_owned());

    assert_eq!(
        list.failure(),
        Some("acme down"),
        "BTreeMap order makes the report deterministic"
    );
}
