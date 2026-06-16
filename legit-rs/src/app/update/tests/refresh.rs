// ── refresh: direct dispatch, tier order, indicator, drain, checks, retry ───

use super::*;
use crate::{
    blocker::{BlockerResult, Tier},
    github::types::ReviewStatus,
};

/// A review-status whose mergeable is `UNKNOWN` (the state GitHub returns until
/// it finishes computing mergeability) carrying an optional head SHA.
fn unknown_status(head_sha: Option<&str>) -> ReviewStatus {
    ReviewStatus {
        additions: 0,
        deletions: 0,
        review_decision: String::new(),
        mergeable: "UNKNOWN".to_owned(),
        last_commit_date: None,
        head_commit_sha: head_sha.map(str::to_owned),
    }
}

/// A settled review-status (mergeable known, so it triggers no UNKNOWN retry)
/// carrying a head SHA — used to drive the checks fan-out on arrival.
fn mergeable_status(head_sha: &str) -> ReviewStatus {
    ReviewStatus {
        mergeable: "MERGEABLE".to_owned(),
        head_commit_sha: Some(head_sha.to_owned()),
        ..unknown_status(None)
    }
}

/// Cache a smart-status tier for a PR so `R` derives its dispatch order.
fn set_tier(model: &mut Model, number: u64, tier: Tier) {
    model.blockers.insert(
        key(number),
        BlockerResult {
            blocker: String::new(),
            tier,
            reason: String::new(),
        },
    );
}

/// Seed cached check runs for `number`'s head commit, so a later
/// `maybe_fetch_checks` would suppress the fetch as already-present unless the
/// entry is evicted first.
fn seed_checks(model: &mut Model, number: u64, head_sha: &str) {
    model
        .list
        .pr_mut(&key(number))
        .expect("seeded PR exists")
        .head_commit_sha = Some(head_sha.to_owned());
    model.enrichment.checks.insert(
        ("mayfieldiv/legit".to_owned(), head_sha.to_owned()),
        Vec::new(),
    );
}

/// The PR keys of every `RefreshPr` in `cmds`, in dispatch order.
fn refreshed_keys(cmds: &[Cmd]) -> Vec<PrKey> {
    cmds.iter()
        .filter_map(|c| match c {
            Cmd::RefreshPr { key, .. } => Some(key.clone()),
            _ => None,
        })
        .collect()
}

/// A list-ready model: auth + repo resolved, `numbers` streamed and the listing
/// marked loaded, relaid out so a selection and visible rows exist.
fn list_model(numbers: &[u64]) -> Model {
    let mut model = enriched_model(numbers);
    model.list.complete_fetch("mayfieldiv/legit");
    model.relayout();
    model
}

/// Flatten grouping so the visible order is insertion order — distinct from any
/// tier ordering, which lets a test prove dispatch reorders by tier.
fn flatten(model: &mut Model) {
    model.list.cycle_grouping(); // SmartStatus -> Repo
    model.list.cycle_grouping(); // Repo -> None
    model.relayout();
}

#[test]
fn r_refreshes_the_selected_pr_with_files() {
    let mut model = list_model(&[1, 2, 3]);
    assert_eq!(model.list.selected_pr().unwrap().number, 1);

    let cmds = update(&mut model, key_event(KeyCode::Char('r')));

    let refreshed = refreshed_keys(&cmds);
    assert_eq!(refreshed, [key(1)], "exactly the selected PR refreshes");
    match cmds.iter().find(|c| matches!(c, Cmd::RefreshPr { .. })) {
        Some(Cmd::RefreshPr { include_files, .. }) => {
            assert!(
                include_files,
                "selected refresh includes files for the summary"
            );
        }
        other => panic!("expected a RefreshPr, got {other:?}"),
    }
    assert!(
        model.is_refreshing(&model.list.prs()[0]),
        "the selected PR shows the in-flight indicator",
    );
    assert!(
        model.status.is_none(),
        "a single-PR refresh posts no status; the row indicator is enough",
    );
}

#[test]
fn shift_r_refreshes_visible_prs_in_tier_order_and_reloads_config() {
    let mut model = list_model(&[1, 2, 3]);
    flatten(&mut model);
    set_tier(&mut model, 1, Tier::WaitingOnAuthor); // rank 2
    set_tier(&mut model, 2, Tier::MeBlocking); // rank 0
    set_tier(&mut model, 3, Tier::NeedsReview); // rank 1
    model.relayout();

    let cmds = update(&mut model, key_event(KeyCode::Char('R')));

    assert!(
        cmds.iter().any(|c| matches!(c, Cmd::LoadConfig)),
        "R re-reads config to pick up repos added since launch: {cmds:?}",
    );
    assert_eq!(
        refreshed_keys(&cmds),
        [key(2), key(3), key(1)],
        "me-blocking dispatches first, then needs-review, then waiting-on-author",
    );
    for cmd in &cmds {
        if let Cmd::RefreshPr { include_files, .. } = cmd {
            assert!(!include_files, "R refreshes do not fetch files");
        }
    }
    let status = model.status.as_ref().expect("R posts an info status");
    assert_eq!(status.kind, StatusKind::Info);
    assert_eq!(status.text, "Refreshing 3 PRs…");
    for n in 1..=3 {
        assert!(
            model.is_refreshing(&model.list.prs()[(n - 1) as usize]),
            "every visible PR shows the refresh indicator",
        );
    }
}

#[test]
fn shift_r_dispatches_every_visible_pr_without_a_cap() {
    // The old design capped concurrent PR refreshes; dispatch now goes straight
    // to the limiter, so every visible PR refreshes at once (the limiter, not a
    // refresh cap, bounds in-flight HTTP).
    let mut model = list_model(&[1, 2, 3, 4, 5]);
    flatten(&mut model);

    let cmds = update(&mut model, key_event(KeyCode::Char('R')));

    assert_eq!(
        refreshed_keys(&cmds),
        [key(1), key(2), key(3), key(4), key(5)],
        "all five dispatch immediately — no cap holds any back",
    );
    let status = model.status.as_ref().expect("R posts an info status");
    assert_eq!(status.text, "Refreshing 5 PRs…");
}

#[test]
fn shift_r_with_no_visible_prs_posts_no_status() {
    let mut model = list_model(&[]);

    let cmds = update(&mut model, key_event(KeyCode::Char('R')));

    assert!(
        refreshed_keys(&cmds).is_empty(),
        "nothing to refresh: {cmds:?}",
    );
    assert!(
        model.status.is_none(),
        "an empty refresh-all posts no count message",
    );
    // The config reload still fires so a newly added repo can appear.
    assert!(cmds.iter().any(|c| matches!(c, Cmd::LoadConfig)));
}

#[test]
fn re_pressing_r_while_refreshing_is_deduped() {
    let mut model = list_model(&[1]);
    let first = update(&mut model, key_event(KeyCode::Char('r')));
    assert_eq!(refreshed_keys(&first), [key(1)], "first press dispatches");

    // The PR is still in flight (no RefreshComplete yet), so a second press is
    // a no-op rather than a duplicate fan-out.
    let second = update(&mut model, key_event(KeyCode::Char('r')));
    assert!(
        refreshed_keys(&second).is_empty(),
        "re-pressing r while refreshing dispatches nothing: {second:?}",
    );
}

#[test]
fn draining_all_refreshes_posts_a_success_summary() {
    let mut model = list_model(&[1]);
    update(&mut model, key_event(KeyCode::Char('r')));
    assert!(model.status.is_none(), "r posts no status while in flight");

    let cmds = update(&mut model, Msg::RefreshComplete { pr: key(1) });

    assert!(model.refreshing.is_empty(), "every refresh drained");
    let status = model.status.as_ref().expect("drain posts a success status");
    assert_eq!(status.kind, StatusKind::Success);
    assert_eq!(status.text, "Refreshed 1 PR");
    assert!(
        cmds.iter()
            .any(|c| matches!(c, Cmd::ScheduleStatusClear { .. })),
        "the success message auto-clears: {cmds:?}",
    );
}

#[test]
fn completing_a_refresh_all_run_reports_the_count_then_resets() {
    let mut model = list_model(&[1, 2, 3]);
    flatten(&mut model);
    update(&mut model, key_event(KeyCode::Char('R')));

    // Complete two of three: still in flight, no summary yet.
    update(&mut model, Msg::RefreshComplete { pr: key(1) });
    update(&mut model, Msg::RefreshComplete { pr: key(2) });
    assert!(!model.refreshing.is_empty(), "one refresh still in flight");

    // The last completion drains the run and reports the plural count.
    update(&mut model, Msg::RefreshComplete { pr: key(3) });
    let status = model.status.as_ref().expect("drain posts a success status");
    assert_eq!(status.text, "Refreshed 3 PRs");
    assert_eq!(
        model.refresh_completed, 0,
        "the run count resets after it is reported",
    );
}

#[test]
fn refresh_complete_on_an_unknown_pr_is_harmless() {
    let mut model = list_model(&[1]);

    // No refresh was ever dispatched, so completing one is a no-op: no panic,
    // no spurious success message (nothing was refreshed in this run).
    let cmds = update(&mut model, Msg::RefreshComplete { pr: key(1) });

    assert!(cmds.is_empty(), "nothing to report: {cmds:?}");
    assert!(model.status.is_none());
}

#[test]
fn refresh_refetches_checks_even_when_head_sha_is_unchanged() {
    // The prime refresh case: CI re-ran on the same commit. Evicting the cached
    // checks on refresh lets the canonical `maybe_fetch_checks` re-fetch them
    // when review-status arrives with the unchanged SHA.
    let mut model = list_model(&[1]);
    seed_checks(&mut model, 1, "abc123");

    update(&mut model, key_event(KeyCode::Char('r')));
    let cmds = update(
        &mut model,
        Msg::ReviewStatusArrived {
            pr: key(1),
            status: mergeable_status("abc123"),
        },
    );

    assert!(
        cmds.iter()
            .any(|c| matches!(c, Cmd::FetchChecks { head_sha, .. } if head_sha == "abc123")),
        "a refresh re-fetches checks for the unchanged head commit: {cmds:?}",
    );
}

#[test]
fn review_status_arrival_without_a_refresh_keeps_present_checks() {
    // The converse: outside a refresh, a review-status arrival for a head SHA
    // whose checks are already cached must NOT re-fetch — that would double the
    // work on every list refresh.
    let mut model = list_model(&[1]);
    seed_checks(&mut model, 1, "abc123");

    let cmds = update(
        &mut model,
        Msg::ReviewStatusArrived {
            pr: key(1),
            status: mergeable_status("abc123"),
        },
    );

    assert!(
        !cmds.iter().any(|c| matches!(c, Cmd::FetchChecks { .. })),
        "present checks are not re-fetched without a refresh: {cmds:?}",
    );
}

#[test]
fn unknown_mergeable_on_open_pr_schedules_one_shot_retry() {
    let mut model = list_model(&[1]); // sample PR #1 is OPEN

    let cmds = update(
        &mut model,
        Msg::ReviewStatusArrived {
            pr: key(1),
            status: unknown_status(Some("abc123")),
        },
    );

    match cmds.iter().find(|c| matches!(c, Cmd::DelayedRetry { .. })) {
        Some(Cmd::DelayedRetry { pr, delay_ms }) => {
            assert_eq!(*pr, key(1));
            assert_eq!(*delay_ms, 3_000);
        }
        other => panic!("expected a DelayedRetry, got {other:?} in {cmds:?}"),
    }

    // A second UNKNOWN arrival for the same PR must NOT re-schedule — one-shot.
    let cmds = update(
        &mut model,
        Msg::ReviewStatusArrived {
            pr: key(1),
            status: unknown_status(Some("abc123")),
        },
    );
    assert!(
        !cmds.iter().any(|c| matches!(c, Cmd::DelayedRetry { .. })),
        "the mergeable retry fires at most once per PR: {cmds:?}",
    );
}

#[test]
fn unknown_mergeable_on_merged_or_closed_pr_does_not_retry() {
    for state in [PRState::Merged, PRState::Closed] {
        let mut model = list_model(&[1]);
        model.list.pr_mut(&key(1)).unwrap().state = state.clone();

        let cmds = update(
            &mut model,
            Msg::ReviewStatusArrived {
                pr: key(1),
                status: unknown_status(None),
            },
        );

        assert!(
            !cmds.iter().any(|c| matches!(c, Cmd::DelayedRetry { .. })),
            "GitHub reports UNKNOWN permanently for {state:?}; do not retry: {cmds:?}",
        );
    }
}

#[test]
fn mergeable_retry_due_refetches_review_status_only() {
    let mut model = list_model(&[1]);

    let cmds = update(&mut model, Msg::MergeableRetryDue { pr: key(1) });

    match cmds.as_slice() {
        [Cmd::FetchReviewStatus { pr_numbers, .. }] => {
            assert_eq!(
                pr_numbers,
                &[1],
                "retry re-fetches review-status for the one PR"
            );
        }
        other => panic!("expected a single FetchReviewStatus, got {other:?}"),
    }
}

#[test]
fn manual_refresh_re_arms_the_mergeable_retry() {
    let mut model = list_model(&[1]);
    // First UNKNOWN arms and consumes the one-shot guard.
    update(
        &mut model,
        Msg::ReviewStatusArrived {
            pr: key(1),
            status: unknown_status(None),
        },
    );

    // A manual refresh of the PR clears the guard...
    update(&mut model, key_event(KeyCode::Char('r')));

    // ...so the next UNKNOWN schedules a fresh retry.
    let cmds = update(
        &mut model,
        Msg::ReviewStatusArrived {
            pr: key(1),
            status: unknown_status(None),
        },
    );
    assert!(
        cmds.iter().any(|c| matches!(c, Cmd::DelayedRetry { .. })),
        "a manual refresh re-arms the UNKNOWN retry: {cmds:?}",
    );
}

#[test]
fn config_reload_fetches_only_newly_tracked_repos() {
    // mayfieldiv/legit is already fetched (it has a phase). A reload that adds
    // acme/web must fetch only the new repo, never re-streaming the existing
    // list (which would duplicate PRs).
    let mut model = list_model(&[1]);

    let cmds = update(
        &mut model,
        Msg::ConfigLoaded(config_with_repos(&["acme/web"])),
    );

    assert_eq!(
        fetched_slugs(&cmds),
        ["acme/web"],
        "only the newly tracked repo fetches on reload",
    );
}

#[test]
fn config_reload_retries_a_failed_repo_and_clears_its_partial_prs() {
    // A listing that failed may have streamed some PRs before erroring. A reload
    // must retry the repo — `R` means "refresh everything" — and drop those
    // partial PRs first, since the re-stream appends and would otherwise
    // duplicate them.
    let mut model = enriched_model(&[1]); // mayfieldiv/legit, one PR streamed
    model
        .list
        .fail_fetch("mayfieldiv/legit", "list open PRs: boom".to_owned());
    assert_eq!(
        model.list.prs().len(),
        1,
        "precondition: a partial PR pooled"
    );

    let cmds = update(&mut model, Msg::ConfigLoaded(config_with_repos(&[])));

    assert_eq!(
        fetched_slugs(&cmds),
        ["mayfieldiv/legit"],
        "the failed repo re-fetches on reload",
    );
    assert!(
        model.list.prs().is_empty(),
        "the failed attempt's partial PRs are cleared before the retry re-streams",
    );
}
