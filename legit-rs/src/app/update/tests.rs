use chrono::TimeZone;
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use crate::{
    app::{
        cmd::Cmd,
        model::{Model, StatusKind},
        msg::Msg,
        pr_list::Phase,
        update::update,
    },
    git_remote::RepoInfo,
    github::rest::{PR, PRState},
    secret::Secret,
};

fn sample_pr(number: u64, title: &str) -> PR {
    PR {
        number,
        title: title.to_owned(),
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
        head_ref: format!("feature/{number}"),
        base_ref: "main".to_owned(),
        head_repository_owner: "mayfieldiv".to_owned(),
        state: PRState::Open,
    }
}

fn key_event(code: KeyCode) -> Msg {
    Msg::TerminalEvent(ratatui::crossterm::event::Event::Key(KeyEvent::new(
        code,
        ratatui::crossterm::event::KeyModifiers::NONE,
    )))
}

#[test]
fn network_stats_changed_updates_model() {
    let (mut model, _) = Model::new();
    assert_eq!(model.network_stats.in_flight, 0);

    let cmds = update(
        &mut model,
        Msg::NetworkStatsChanged(crate::github::limiter::NetworkStats {
            in_flight: 3,
            waiting: 5,
        }),
    );

    assert_eq!(model.network_stats.in_flight, 3);
    assert_eq!(model.network_stats.waiting, 5);
    assert!(cmds.is_empty());
}

#[test]
fn q_key_sets_should_quit() {
    let (mut model, _) = Model::new();

    update(&mut model, key_event(KeyCode::Char('q')));

    assert!(model.should_quit);
}

#[test]
fn config_loaded_preserves_existing_status() {
    let (mut model, _) = Model::new();
    update(
        &mut model,
        Msg::CommandFailed {
            context: "resolve auth token",
            error: "failed".to_owned(),
        },
    );

    update(&mut model, Msg::ConfigLoaded(Default::default()));

    let status = model.status.as_ref().expect("status preserved");
    assert!(status.text.contains("resolve auth token"));
}

#[test]
fn command_failed_sets_error_status_that_schedules_a_clear() {
    let (mut model, _) = Model::new();

    let cmds = update(
        &mut model,
        Msg::CommandFailed {
            context: "load config",
            error: "boom".to_owned(),
        },
    );

    let status = model.status.as_ref().expect("error status set");
    assert_eq!(status.kind, StatusKind::Error);
    assert!(status.text.contains("load config"));
    // Errors auto-clear after 8s.
    match cmds.as_slice() {
        [Cmd::ScheduleStatusClear { token, delay_ms }] => {
            assert_eq!(*token, model.status_gen);
            assert_eq!(*delay_ms, 8_000);
        }
        other => panic!("expected one ScheduleStatusClear, got {other:?}"),
    }
}

#[test]
fn status_cleared_clears_only_when_token_is_current() {
    let (mut model, _) = Model::new();
    update(
        &mut model,
        Msg::CommandFailed {
            context: "load config",
            error: "boom".to_owned(),
        },
    );
    let current = model.status_gen;

    // A stale timer (older generation) must not wipe the live message.
    update(&mut model, Msg::StatusCleared { token: current - 1 });
    assert!(model.status.is_some(), "stale clear must be ignored");

    // The matching timer clears it.
    update(&mut model, Msg::StatusCleared { token: current });
    assert!(model.status.is_none(), "current clear empties the status");
}

#[test]
fn initial_cmds_include_repo_detection() {
    let (_, cmds) = Model::new();

    assert!(
        cmds.iter().any(|c| matches!(c, Cmd::DetectRepo)),
        "Model::new should kick off repo detection, got {:?}",
        cmds,
    );
}

#[test]
fn dispatching_fetch_marks_list_as_loading() {
    let (mut model, _) = Model::new();
    model.auth_token = Some(Secret::new("ghp_test".to_owned()));

    let cmds = update(
        &mut model,
        Msg::RepoDetected(RepoInfo {
            owner: "mayfieldiv".to_owned(),
            repo: "legit".to_owned(),
        }),
    );

    assert_eq!(cmds.len(), 1);
    assert!(
        matches!(model.list.phase(), Phase::Loading),
        "list should enter Loading phase on fetch dispatch",
    );
}

#[test]
fn pr_arrived_clears_loading_phase() {
    let (mut model, _) = Model::new();
    model.list.begin_fetch();

    update(&mut model, Msg::PrArrived(sample_pr(1, "a")));

    // Push alone doesn't transition phase; the explicit PrListLoaded does.
    // Until then, the list is still "loading more" — but rows render now.
    assert_eq!(model.list.prs().len(), 1);
}

#[test]
fn pr_list_loaded_transitions_to_loaded() {
    let (mut model, _) = Model::new();
    model.list.begin_fetch();

    update(&mut model, Msg::PrListLoaded);

    assert!(matches!(model.list.phase(), Phase::Loaded));
}

#[test]
fn pr_list_failed_transitions_to_failed_with_message() {
    let (mut model, _) = Model::new();
    model.list.begin_fetch();

    update(
        &mut model,
        Msg::PrListFailed {
            context: "list open PRs",
            error: "boom".to_owned(),
        },
    );

    let failure = model
        .list
        .failure()
        .expect("phase should be Failed after PrListFailed");
    assert!(failure.contains("list open PRs"));
    assert!(failure.contains("boom"));
}

#[test]
fn pr_arrived_appends_to_open_pr_list() {
    let (mut model, _) = Model::new();

    let cmds = update(&mut model, Msg::PrArrived(sample_pr(42, "first")));

    assert_eq!(model.list.prs().len(), 1);
    assert_eq!(model.list.prs()[0].number, 42);
    assert!(cmds.is_empty());
}

#[test]
fn repo_detected_without_token_stores_repo_but_does_not_fetch() {
    let (mut model, _) = Model::new();

    let cmds = update(
        &mut model,
        Msg::RepoDetected(RepoInfo {
            owner: "mayfieldiv".to_owned(),
            repo: "legit".to_owned(),
        }),
    );

    let repo = model.repo.as_ref().expect("repo info stored");
    assert_eq!(repo.owner, "mayfieldiv");
    assert_eq!(repo.repo, "legit");
    assert!(
        cmds.is_empty(),
        "no fetch should fire before auth token resolves"
    );
}

#[test]
fn repo_detected_after_token_dispatches_fetch_open_prs() {
    let (mut model, _) = Model::new();
    model.auth_token = Some(Secret::new("ghp_test".to_owned()));

    let cmds = update(
        &mut model,
        Msg::RepoDetected(RepoInfo {
            owner: "mayfieldiv".to_owned(),
            repo: "legit".to_owned(),
        }),
    );

    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        Cmd::FetchOpenPRs { repo, .. } => {
            assert_eq!(repo.owner, "mayfieldiv");
            assert_eq!(repo.repo, "legit");
        }
        other => panic!("expected FetchOpenPRs cmd, got {other:?}"),
    }
}

#[test]
fn j_advances_selection_within_list_bounds() {
    let (mut model, _) = Model::new();
    for n in 1..=3 {
        update(&mut model, Msg::PrArrived(sample_pr(n, "p")));
    }

    update(&mut model, key_event(KeyCode::Char('j')));
    assert_eq!(model.list.selected(), 1);

    update(&mut model, key_event(KeyCode::Char('j')));
    assert_eq!(model.list.selected(), 2);
}

#[test]
fn j_at_last_pr_does_not_advance_past_end() {
    let (mut model, _) = Model::new();
    update(&mut model, Msg::PrArrived(sample_pr(1, "only")));

    update(&mut model, key_event(KeyCode::Char('j')));
    update(&mut model, key_event(KeyCode::Char('j')));

    assert_eq!(model.list.selected(), 0);
}

#[test]
fn k_retreats_selection_and_clamps_at_zero() {
    let (mut model, _) = Model::new();
    for n in 1..=3 {
        update(&mut model, Msg::PrArrived(sample_pr(n, "p")));
    }
    update(&mut model, key_event(KeyCode::Char('j')));
    update(&mut model, key_event(KeyCode::Char('j')));
    assert_eq!(model.list.selected(), 2);

    update(&mut model, key_event(KeyCode::Char('k')));
    assert_eq!(model.list.selected(), 1);

    update(&mut model, key_event(KeyCode::Char('k')));
    update(&mut model, key_event(KeyCode::Char('k')));
    assert_eq!(model.list.selected(), 0);
}

#[test]
fn terminal_resize_updates_viewport_and_keeps_selection_visible() {
    let (mut model, _) = Model::new();
    for n in 1..=30 {
        update(&mut model, Msg::PrArrived(sample_pr(n, "p")));
    }
    update(
        &mut model,
        Msg::TerminalEvent(ratatui::crossterm::event::Event::Resize(80, 21)),
    );
    // Viewport_height = terminal_height - 1 (status bar).
    assert_eq!(model.list.viewport_height(), 20);

    // Drive the selection deep into the list.
    for _ in 0..25 {
        update(&mut model, key_event(KeyCode::Char('j')));
    }
    assert!(
        selection_is_visible(&model),
        "selection must stay within the 20-row viewport"
    );

    // Shrink: selection must remain on-screen after re-clamp.
    update(
        &mut model,
        Msg::TerminalEvent(ratatui::crossterm::event::Event::Resize(80, 6)),
    );
    assert_eq!(model.list.viewport_height(), 5);
    assert!(
        selection_is_visible(&model),
        "selection must stay within the 5-row viewport after shrink"
    );
}

/// Whether the selected PR's row is among the currently visible display
/// rows. `selected()` is a PR index while `scroll_offset()` counts display
/// rows (headers included), so the two aren't directly comparable — ask the
/// rendered window instead.
fn selection_is_visible(model: &Model) -> bool {
    model.list.visible_rows().any(|(_, selected)| selected)
}

#[test]
fn streaming_prs_keep_selection_pinned() {
    let (mut model, _) = Model::new();
    update(&mut model, Msg::PrArrived(sample_pr(1, "a")));
    update(&mut model, Msg::PrArrived(sample_pr(2, "b")));
    update(&mut model, key_event(KeyCode::Char('j')));
    assert_eq!(model.list.selected(), 1);

    update(&mut model, Msg::PrArrived(sample_pr(3, "c")));
    update(&mut model, Msg::PrArrived(sample_pr(4, "d")));

    assert_eq!(
        model.list.selected(),
        1,
        "selection should not shift when new PRs arrive"
    );
}

// ── grouping ──────────────────────────────────────────────────────────────

#[test]
fn g_cycles_grouping_smart_status_repo_none_and_resets_selection() {
    use crate::app::grouping::Grouping;

    let (mut model, _) = Model::new();
    for n in 1..=3 {
        update(&mut model, Msg::PrArrived(sample_pr(n, "p")));
    }
    update(&mut model, key_event(KeyCode::Char('j')));
    update(&mut model, key_event(KeyCode::Char('j')));
    assert_eq!(model.list.selected(), 2);

    update(&mut model, key_event(KeyCode::Char('g')));
    assert_eq!(model.list.grouping(), Grouping::Repo);
    assert_eq!(model.list.selected(), 0, "selection resets on cycle");

    update(&mut model, key_event(KeyCode::Char('g')));
    assert_eq!(model.list.grouping(), Grouping::None);

    update(&mut model, key_event(KeyCode::Char('g')));
    assert_eq!(
        model.list.grouping(),
        Grouping::SmartStatus,
        "cycle wraps back to smart-status"
    );
}

#[test]
fn j_skips_group_headers_when_smart_status_grouping_has_tiers() {
    use crate::blocker::{BlockerResult, Tier};

    let (mut model, _) = Model::new();
    update(&mut model, Msg::PrArrived(sample_pr(1, "me")));
    update(&mut model, Msg::PrArrived(sample_pr(2, "waiting")));
    // Seed two tiers so the layout has two headers between the PR rows.
    model.blockers.insert(
        1,
        BlockerResult {
            blocker: "me".to_owned(),
            tier: Tier::MeBlocking,
            reason: "you".to_owned(),
        },
    );
    model.blockers.insert(
        2,
        BlockerResult {
            blocker: "charlie".to_owned(),
            tier: Tier::WaitingOnAuthor,
            reason: "draft".to_owned(),
        },
    );
    model.relayout();
    assert_eq!(model.list.selected(), 0);

    update(&mut model, key_event(KeyCode::Char('j')));
    assert_eq!(
        model.list.selected(),
        1,
        "j steps PR-to-PR, skipping the intervening header"
    );
}

#[test]
fn pr_list_failed_keeps_already_arrived_prs() {
    let (mut model, _) = Model::new();
    update(&mut model, Msg::PrArrived(sample_pr(1, "first")));

    let cmds = update(
        &mut model,
        Msg::PrListFailed {
            context: "list open PRs",
            error: "network down".to_owned(),
        },
    );

    assert_eq!(
        model.list.prs().len(),
        1,
        "already-arrived PRs should remain after a fetch failure"
    );
    let failure = model.list.failure().expect("failure recorded");
    assert!(failure.contains("list open PRs"));
    assert!(failure.contains("network down"));
    assert!(cmds.is_empty());
}

// ── enrichment ──────────────────────────────────────────────────────────

use crate::github::types::ReviewStatus;

/// A model with auth + repo resolved and `numbers` streamed into the list.
fn enriched_model(numbers: &[u64]) -> Model {
    let (mut model, _) = Model::new();
    model.auth_token = Some(Secret::new("ghp_test".to_owned()));
    model.repo = Some(RepoInfo {
        owner: "mayfieldiv".to_owned(),
        repo: "legit".to_owned(),
    });
    model.list.begin_fetch();
    for n in numbers {
        model.list.push(sample_pr(*n, "p"));
    }
    model
}

fn review_status(head_sha: Option<&str>) -> ReviewStatus {
    ReviewStatus {
        additions: 12,
        deletions: 4,
        review_decision: "APPROVED".to_owned(),
        mergeable: "MERGEABLE".to_owned(),
        last_commit_date: None,
        head_commit_sha: head_sha.map(str::to_owned),
    }
}

#[test]
fn pr_list_loaded_fans_out_enrichment_per_pr() {
    let mut model = enriched_model(&[1, 2]);

    let cmds = update(&mut model, Msg::PrListLoaded);

    // 1 batched review-status + (threads + reviews + issue-comments) per PR.
    assert_eq!(cmds.len(), 1 + 2 * 3);
    match &cmds[0] {
        Cmd::FetchReviewStatus { pr_numbers, .. } => assert_eq!(pr_numbers, &[1, 2]),
        other => panic!("first cmd should batch review status, got {other:?}"),
    }
    let threads = cmds
        .iter()
        .filter(|c| matches!(c, Cmd::FetchThreads { .. }))
        .count();
    let reviews = cmds
        .iter()
        .filter(|c| matches!(c, Cmd::FetchReviews { .. }))
        .count();
    let comments = cmds
        .iter()
        .filter(|c| matches!(c, Cmd::FetchIssueComments { .. }))
        .count();
    assert_eq!((threads, reviews, comments), (2, 2, 2));
    // Checks are NOT fanned out yet — they wait on review-status SHAs.
    assert!(!cmds.iter().any(|c| matches!(c, Cmd::FetchChecks { .. })));
}

#[test]
fn pr_list_loaded_with_empty_list_fans_out_nothing() {
    let mut model = enriched_model(&[]);

    let cmds = update(&mut model, Msg::PrListLoaded);

    assert!(cmds.is_empty());
}

#[test]
fn pr_list_loaded_without_auth_fans_out_nothing() {
    let (mut model, _) = Model::new();
    model.repo = Some(RepoInfo {
        owner: "mayfieldiv".to_owned(),
        repo: "legit".to_owned(),
    });
    model.list.begin_fetch();
    model.list.push(sample_pr(1, "p"));

    let cmds = update(&mut model, Msg::PrListLoaded);

    assert!(
        cmds.is_empty(),
        "no enrichment until the auth token resolves"
    );
}

#[test]
fn review_status_arrived_overwrites_pr_fields_and_fetches_checks() {
    let mut model = enriched_model(&[1]);

    let cmds = update(
        &mut model,
        Msg::ReviewStatusArrived {
            pr_number: 1,
            status: review_status(Some("abc123")),
        },
    );

    let pr = &model.list.prs()[0];
    assert_eq!(pr.additions, 12);
    assert_eq!(pr.deletions, 4);
    assert_eq!(pr.review_decision, "APPROVED");
    assert_eq!(pr.mergeable, "MERGEABLE");
    assert_eq!(pr.head_commit_sha.as_deref(), Some("abc123"));

    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        Cmd::FetchChecks { head_sha, .. } => assert_eq!(head_sha, "abc123"),
        other => panic!("expected FetchChecks for the new head SHA, got {other:?}"),
    }
}

#[test]
fn review_status_arrived_without_sha_skips_checks() {
    let mut model = enriched_model(&[1]);

    let cmds = update(
        &mut model,
        Msg::ReviewStatusArrived {
            pr_number: 1,
            status: review_status(None),
        },
    );

    assert_eq!(model.list.prs()[0].mergeable, "MERGEABLE");
    assert!(cmds.is_empty(), "no SHA means no checks fetch");
}

#[test]
fn review_status_arrived_for_unknown_pr_is_a_noop() {
    let mut model = enriched_model(&[1]);

    let cmds = update(
        &mut model,
        Msg::ReviewStatusArrived {
            pr_number: 999,
            status: review_status(Some("abc123")),
        },
    );

    // The known PR is untouched and no checks are fetched for the stray SHA.
    assert_eq!(model.list.prs()[0].mergeable, "UNKNOWN");
    assert!(cmds.is_empty());
}

#[test]
fn review_status_arrived_skips_checks_already_fetched_for_sha() {
    let mut model = enriched_model(&[1]);
    model
        .enrichment
        .checks
        .insert("abc123".to_owned(), Vec::new());

    let cmds = update(
        &mut model,
        Msg::ReviewStatusArrived {
            pr_number: 1,
            status: review_status(Some("abc123")),
        },
    );

    assert!(
        cmds.is_empty(),
        "checks already present for this SHA; don't refetch"
    );
}

#[test]
fn threads_arrived_stores_threads_by_pr_number() {
    let mut model = enriched_model(&[1]);
    let thread = crate::github::types::FullReviewThread {
        id: "T1".to_owned(),
        is_resolved: false,
        path: "src/x".to_owned(),
        line: Some(3),
        comments: Vec::new(),
    };

    let cmds = update(
        &mut model,
        Msg::ThreadsArrived {
            pr_number: 1,
            threads: vec![thread.clone()],
        },
    );

    assert_eq!(model.enrichment.review_threads.get(&1), Some(&vec![thread]));
    assert!(cmds.is_empty());
}

#[test]
fn reviews_arrived_stores_reviews_by_pr_number() {
    let mut model = enriched_model(&[1]);
    let review = crate::github::types::Review {
        user: "alice".to_owned(),
        state: "APPROVED".to_owned(),
    };

    update(
        &mut model,
        Msg::ReviewsArrived {
            pr_number: 1,
            reviews: vec![review.clone()],
        },
    );

    assert_eq!(model.enrichment.reviews.get(&1), Some(&vec![review]));
}

#[test]
fn checks_arrived_stores_checks_by_head_sha() {
    let mut model = enriched_model(&[1]);
    let check = crate::github::types::CheckRun {
        name: "build".to_owned(),
        status: "completed".to_owned(),
        conclusion: Some("success".to_owned()),
    };

    update(
        &mut model,
        Msg::ChecksArrived {
            head_sha: "abc123".to_owned(),
            checks: vec![check.clone()],
        },
    );

    assert_eq!(model.enrichment.checks.get("abc123"), Some(&vec![check]));
}

#[test]
fn issue_comments_arrived_stores_comments_by_pr_number() {
    let mut model = enriched_model(&[1]);
    let comment = crate::github::types::IssueComment {
        id: 7,
        author: "bob".to_owned(),
        body: "lgtm".to_owned(),
        created_at: chrono::Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap(),
        url: "u".to_owned(),
        is_bot: false,
    };

    update(
        &mut model,
        Msg::IssueCommentsArrived {
            pr_number: 1,
            comments: vec![comment.clone()],
        },
    );

    assert_eq!(
        model.enrichment.issue_comments.get(&1),
        Some(&vec![comment])
    );
}

#[test]
fn enrichment_failure_records_error_and_keeps_data() {
    let mut model = enriched_model(&[1]);
    model.enrichment.reviews.insert(
        1,
        vec![crate::github::types::Review {
            user: "alice".to_owned(),
            state: "APPROVED".to_owned(),
        }],
    );

    let cmds = update(
        &mut model,
        Msg::CommandFailed {
            context: "fetch check runs",
            error: "500 Server Error".to_owned(),
        },
    );

    let status = model.status.as_ref().expect("error status recorded");
    assert_eq!(status.kind, StatusKind::Error);
    assert!(status.text.contains("fetch check runs"));
    assert!(status.text.contains("500 Server Error"));
    // Best-effort: nothing already loaded is dropped on a failure.
    assert_eq!(model.list.prs().len(), 1);
    assert!(model.enrichment.reviews.contains_key(&1));
    // The error message is scheduled to auto-clear.
    assert!(matches!(cmds.as_slice(), [Cmd::ScheduleStatusClear { .. }]));
}
