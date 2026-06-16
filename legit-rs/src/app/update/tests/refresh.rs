// ── refresh queue: priority, indicator, drain, mergeable UNKNOWN retry ──────

use super::*;
use crate::{
    app::refresh_queue::RefreshPhase,
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

/// Cache a smart-status tier for a PR so `R` derives its refresh priority.
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

#[test]
fn r_enqueues_the_selected_pr_at_priority_zero_with_files() {
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
    assert_eq!(
        model.refresh_phase_for(&model.list.prs()[0]),
        Some(RefreshPhase::Refreshing),
        "the selected PR shows the in-flight indicator",
    );
    assert!(
        model.status.is_none(),
        "a single-PR refresh posts no status; the row indicator is enough",
    );
}

#[test]
fn shift_r_enqueues_visible_prs_by_tier_and_reloads_config() {
    let mut model = list_model(&[1, 2, 3]);
    // Flat (no grouping) so the visible order is insertion order — distinct
    // from the priority order, proving the queue reorders by tier.
    model.list.cycle_grouping(); // SmartStatus -> Repo
    model.list.cycle_grouping(); // Repo -> None
    set_tier(&mut model, 1, Tier::WaitingOnAuthor); // priority 3
    set_tier(&mut model, 2, Tier::MeBlocking); // priority 1
    set_tier(&mut model, 3, Tier::NeedsReview); // priority 2
    model.relayout();

    let cmds = update(&mut model, key_event(KeyCode::Char('R')));

    assert!(
        cmds.iter().any(|c| matches!(c, Cmd::LoadConfig)),
        "R re-reads config to pick up repos added since launch: {cmds:?}",
    );
    assert_eq!(
        refreshed_keys(&cmds),
        [key(2), key(3), key(1)],
        "me-blocking refreshes first, then needs-review, then waiting-on-author",
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
        assert_eq!(
            model.refresh_phase_for(&model.list.prs()[(n - 1) as usize]),
            Some(RefreshPhase::Refreshing),
            "every visible PR shows the refresh indicator",
        );
    }
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
fn active_cap_holds_excess_prs_queued_until_a_slot_frees() {
    // Five PRs, all the same (un-derived) priority, so they queue FIFO. With the
    // active cap at 4, the fifth waits as Queued until one completes.
    let mut model = list_model(&[1, 2, 3, 4, 5]);
    model.list.cycle_grouping();
    model.list.cycle_grouping(); // flat: visible order == insertion order
    model.relayout();

    let cmds = update(&mut model, key_event(KeyCode::Char('R')));

    assert_eq!(
        refreshed_keys(&cmds),
        [key(1), key(2), key(3), key(4)],
        "only the first four dispatch; the cap holds the rest",
    );
    assert_eq!(
        model.refresh_phase_for(&model.list.prs()[4]),
        Some(RefreshPhase::Queued),
        "PR #5 waits in the queue",
    );

    // Completing PR #1 frees a slot; the queued PR #5 dispatches next.
    let cmds = update(&mut model, Msg::RefreshComplete { pr: key(1) });

    assert_eq!(refreshed_keys(&cmds), [key(5)], "the queued PR drains next");
    assert_eq!(
        model.refresh_phase_for(&model.list.prs()[0]),
        None,
        "the completed PR clears its indicator",
    );
    assert_eq!(
        model.refresh_phase_for(&model.list.prs()[4]),
        Some(RefreshPhase::Refreshing),
    );
}

#[test]
fn draining_the_queue_posts_a_success_summary() {
    let mut model = list_model(&[1]);
    update(&mut model, key_event(KeyCode::Char('r')));
    assert!(model.status.is_none(), "r posts no status while in flight");

    let cmds = update(&mut model, Msg::RefreshComplete { pr: key(1) });

    assert!(model.refresh_queue.is_idle(), "the queue fully drained");
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
fn refresh_complete_on_an_unknown_pr_is_harmless() {
    let mut model = list_model(&[1]);

    // No refresh was ever enqueued, so completing one is a no-op: no panic, no
    // spurious success message (nothing was refreshed in this run).
    let cmds = update(&mut model, Msg::RefreshComplete { pr: key(1) });

    assert!(cmds.is_empty(), "nothing to pump or report: {cmds:?}");
    assert!(model.status.is_none());
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
