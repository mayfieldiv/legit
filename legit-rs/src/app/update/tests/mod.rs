use chrono::TimeZone;
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use crate::{
    app::{
        cmd::Cmd,
        model::{Model, RepoDetection, StatusKind},
        msg::Msg,
        pr_list::Phase,
        update::update,
    },
    git_remote::RepoInfo,
    github::rest::{PR, PRState, PrKey},
    secret::Secret,
};

mod enrichment;
mod filter;
mod multi_repo;
mod tabs;

/// The `PrKey` of `sample_pr(number, ..)` — every sample PR is stamped with
/// the same Tracked Repo slug.
pub(super) fn key(number: u64) -> PrKey {
    PrKey {
        repo_slug: "mayfieldiv/legit".to_owned(),
        number,
    }
}

pub(super) fn sample_pr(number: u64, title: &str) -> PR {
    PR {
        number,
        repo_slug: "mayfieldiv/legit".to_owned(),
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

pub(super) fn key_event(code: KeyCode) -> Msg {
    Msg::TerminalEvent(ratatui::crossterm::event::Event::Key(KeyEvent::new(
        code,
        ratatui::crossterm::event::KeyModifiers::NONE,
    )))
}

/// A `sample_pr` stamped with a specific Tracked Repo slug.
pub(super) fn sample_pr_in(repo_slug: &str, number: u64, title: &str) -> PR {
    PR {
        repo_slug: repo_slug.to_owned(),
        ..sample_pr(number, title)
    }
}

/// A config tracking `slugs` (bare-slug entries), for fan-out tests.
pub(super) fn config_with_repos(slugs: &[&str]) -> crate::config::LegitConfig {
    crate::config::LegitConfig {
        repos: slugs
            .iter()
            .map(|slug| crate::config::RepoConfig {
                slug: (*slug).to_owned(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

/// The repo slugs of every `FetchOpenPRs` in `cmds`, in dispatch order.
pub(super) fn fetched_slugs(cmds: &[Cmd]) -> Vec<String> {
    cmds.iter()
        .map(|c| match c {
            Cmd::FetchOpenPRs { repo, .. } => repo.slug(),
            other => panic!("expected only FetchOpenPRs, got {other:?}"),
        })
        .collect()
}

/// A model with two Tracked Repos — acme/web (config) + mayfieldiv/legit
/// (CWD) — and one PR streamed in from each. Tabs: 0 All, 1 acme/web,
/// 2 mayfieldiv/legit.
pub(super) fn tabbed_model() -> Model {
    let (mut model, _) = Model::new();
    model.config = config_with_repos(&["acme/web"]);
    model.repo = RepoDetection::Detected(RepoInfo {
        owner: "mayfieldiv".to_owned(),
        repo: "legit".to_owned(),
    });
    update(
        &mut model,
        Msg::PrArrived(sample_pr_in("acme/web", 10, "web pr")),
    );
    update(&mut model, Msg::PrArrived(sample_pr(1, "legit pr")));
    model
}

/// A model with auth + repo resolved and `numbers` streamed into the list.
pub(super) fn enriched_model(numbers: &[u64]) -> Model {
    let (mut model, _) = Model::new();
    model.auth_token = Some(Secret::new("ghp_test".to_owned()));
    model.repo = RepoDetection::Detected(RepoInfo {
        owner: "mayfieldiv".to_owned(),
        repo: "legit".to_owned(),
    });
    model.list.begin_fetch("mayfieldiv/legit");
    for n in numbers {
        model.list.push(sample_pr(*n, "p"));
    }
    model
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
    model.config_loaded = true;

    let cmds = update(
        &mut model,
        Msg::RepoDetected(Some(RepoInfo {
            owner: "mayfieldiv".to_owned(),
            repo: "legit".to_owned(),
        })),
    );

    assert_eq!(cmds.len(), 1);
    assert!(
        model.list.is_loading(Some("mayfieldiv/legit")),
        "the repo should enter Loading phase on fetch dispatch",
    );
}

#[test]
fn pr_arrived_clears_loading_phase() {
    let (mut model, _) = Model::new();
    model.list.begin_fetch("mayfieldiv/legit");

    update(&mut model, Msg::PrArrived(sample_pr(1, "a")));

    // Push alone doesn't transition phase; the explicit PrListLoaded does.
    // Until then, the list is still "loading more" — but rows render now.
    assert_eq!(model.list.prs().len(), 1);
}

#[test]
fn pr_list_loaded_transitions_that_repo_to_loaded() {
    let (mut model, _) = Model::new();
    model.list.begin_fetch("mayfieldiv/legit");

    update(
        &mut model,
        Msg::PrListLoaded {
            repo_slug: "mayfieldiv/legit".to_owned(),
        },
    );

    assert_eq!(
        model.list.phase_of("mayfieldiv/legit"),
        Some(&Phase::Loaded)
    );
}

#[test]
fn pr_list_failed_transitions_that_repo_to_failed_with_message() {
    let (mut model, _) = Model::new();
    model.list.begin_fetch("mayfieldiv/legit");

    update(
        &mut model,
        Msg::PrListFailed {
            repo_slug: "mayfieldiv/legit".to_owned(),
            context: "list open PRs",
            error: "boom".to_owned(),
        },
    );

    let failure = model
        .list
        .failure()
        .expect("phase should be Failed after PrListFailed");
    assert!(failure.contains("list open PRs"));
    assert!(failure.contains("mayfieldiv/legit"), "names the repo");
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
        Msg::RepoDetected(Some(RepoInfo {
            owner: "mayfieldiv".to_owned(),
            repo: "legit".to_owned(),
        })),
    );

    let repo = model.repo.repo().expect("repo info stored");
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
    model.config_loaded = true;

    let cmds = update(
        &mut model,
        Msg::RepoDetected(Some(RepoInfo {
            owner: "mayfieldiv".to_owned(),
            repo: "legit".to_owned(),
        })),
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
fn fetch_waits_for_config_even_with_auth_and_repo() {
    let (mut model, _) = Model::new();
    model.auth_token = Some(Secret::new("ghp_test".to_owned()));
    // config has NOT settled yet — the gate must hold.

    let cmds = update(
        &mut model,
        Msg::RepoDetected(Some(RepoInfo {
            owner: "mayfieldiv".to_owned(),
            repo: "legit".to_owned(),
        })),
    );

    assert!(
        cmds.is_empty(),
        "fetch must wait for config so blockers aren't derived without the user"
    );
    assert!(
        !model.list.is_loading(None),
        "the list must not enter Loading until the fetch actually dispatches"
    );
}

#[test]
fn config_loaded_releases_the_fetch_when_auth_and_repo_already_landed() {
    let (mut model, _) = Model::new();
    model.auth_token = Some(Secret::new("ghp_test".to_owned()));
    model.repo = RepoDetection::Detected(RepoInfo {
        owner: "mayfieldiv".to_owned(),
        repo: "legit".to_owned(),
    });

    // Config arrives last; it must kick off the gated fetch.
    let cmds = update(&mut model, Msg::ConfigLoaded(Default::default()));

    assert!(model.config_loaded);
    assert_eq!(cmds.len(), 1);
    assert!(
        matches!(&cmds[0], Cmd::FetchOpenPRs { .. }),
        "config landing last should dispatch the fetch, got {cmds:?}"
    );
}

#[test]
fn config_load_failed_records_a_fatal_and_does_not_fetch() {
    let (mut model, _) = Model::new();
    model.auth_token = Some(Secret::new("ghp_test".to_owned()));
    model.repo = RepoDetection::Detected(RepoInfo {
        owner: "mayfieldiv".to_owned(),
        repo: "legit".to_owned(),
    });

    let cmds = update(
        &mut model,
        Msg::ConfigLoadFailed {
            error: "invalid bot_logins entry".to_owned(),
        },
    );

    assert!(
        cmds.is_empty(),
        "a malformed config is fatal: no fetch, and no scheduled clear (the failure is persistent)"
    );
    assert!(
        !model.config_loaded,
        "a failed load must not release the fetch gate"
    );
    let fatal = model
        .fatal
        .as_deref()
        .expect("a malformed config must record an app-level fatal error");
    assert!(fatal.contains("config error"));
    assert!(fatal.contains("invalid bot_logins entry"));
}

#[test]
fn detection_failure_with_config_repos_still_fetches_them() {
    // Outside a git repo (or no GitHub remote), `Cmd::DetectRepo` fails and
    // `update` sees `Msg::RepoDetected(None)`. That must settle the gate so the
    // configured Tracked Repos still fetch — not wedge the app at an empty list.
    let (mut model, _) = Model::new();
    model.auth_token = Some(Secret::new("ghp_test".to_owned()));
    model.config = config_with_repos(&["acme/web", "acme/api"]);
    model.config_loaded = true;

    let cmds = update(&mut model, Msg::RepoDetected(None));

    assert!(matches!(model.repo, RepoDetection::Failed));
    // No CWD repo to append; only the configured repos fetch.
    assert_eq!(fetched_slugs(&cmds), ["acme/web", "acme/api"]);
}

#[test]
fn detection_failure_without_config_repos_does_not_fetch_but_surfaces_error() {
    // Detection fails AND there are no configured repos: nothing to fetch, but
    // the failure must still reach the user. `Cmd::DetectRepo` emits a
    // `CommandFailed` (a transient error status) alongside `RepoDetected(None)`;
    // assert that status surface and that the settled gate yields no fetch.
    let (mut model, _) = Model::new();
    model.auth_token = Some(Secret::new("ghp_test".to_owned()));
    model.config_loaded = true;

    let status_cmds = update(
        &mut model,
        Msg::CommandFailed {
            context: "detect repo",
            error: "not a git repository".to_owned(),
        },
    );
    let fetch_cmds = update(&mut model, Msg::RepoDetected(None));

    assert!(matches!(model.repo, RepoDetection::Failed));
    assert!(
        fetch_cmds.is_empty(),
        "no Tracked Repos at all, so nothing fetches"
    );
    // The user can see the detection error in the status bar.
    let status = model.status.as_ref().expect("error status set");
    assert_eq!(status.kind, StatusKind::Error);
    assert!(status.text.contains("detect repo"));
    assert!(status.text.contains("not a git repository"));
    // And that error scheduled its own auto-clear.
    assert!(
        status_cmds
            .iter()
            .any(|c| matches!(c, Cmd::ScheduleStatusClear { .. })),
    );
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
        Msg::TerminalEvent(ratatui::crossterm::event::Event::Resize(80, 22)),
    );
    // Viewport_height = terminal_height - 2 (tab bar + status bar).
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
        Msg::TerminalEvent(ratatui::crossterm::event::Event::Resize(80, 7)),
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
        key(1),
        BlockerResult {
            blocker: "me".to_owned(),
            tier: Tier::MeBlocking,
            reason: "you".to_owned(),
        },
    );
    model.blockers.insert(
        key(2),
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
            repo_slug: "mayfieldiv/legit".to_owned(),
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
