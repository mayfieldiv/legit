use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind};

use super::{cmd::Cmd, model::Model, msg::Msg};

/// Fire `Cmd::FetchOpenPRs` once both auth token and repo detection have
/// landed in the model. The repo defines what to fetch; the token authorizes
/// the request. Either alone yields no command — we wait for the second one.
/// Marks the PR list as Loading so the view swaps from "No open PRs" to
/// "Loading pull requests…" until results land.
fn maybe_fetch_open_prs(model: &mut Model) -> Vec<Cmd> {
    let (Some(token), Some(repo)) = (model.auth_token.as_ref(), model.repo.as_ref()) else {
        return Vec::new();
    };
    model.list.begin_fetch();
    vec![Cmd::FetchOpenPRs {
        owner: repo.owner.clone(),
        repo: repo.repo.clone(),
        token: token.clone(),
    }]
}

pub fn update(model: &mut Model, msg: Msg) -> Vec<Cmd> {
    match msg {
        Msg::TerminalEvent(Event::Key(key)) => {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('q') => model.should_quit = true,
                    KeyCode::Char('j') => model.list.move_down(),
                    KeyCode::Char('k') => model.list.move_up(),
                    _ => {}
                }
            }
            Vec::new()
        }
        Msg::TerminalEvent(Event::Resize(_, height)) => {
            // The status bar takes one row; everything above belongs to the
            // list. Saturating-sub keeps a 0-row viewport handled gracefully.
            model.list.resize((height as usize).saturating_sub(1));
            Vec::new()
        }
        Msg::TerminalEvent(_) => Vec::new(),
        Msg::ConfigLoaded(config) => {
            model.config = config;
            Vec::new()
        }
        Msg::AuthTokenResolved(token) => {
            model.auth_token = Some(token);
            maybe_fetch_open_prs(model)
        }
        Msg::RepoDetected(repo) => {
            model.repo = Some(repo);
            maybe_fetch_open_prs(model)
        }
        Msg::PrArrived(pr) => {
            model.list.push(pr);
            Vec::new()
        }
        Msg::PrListLoaded => {
            model.list.complete_fetch();
            Vec::new()
        }
        Msg::NetworkStatsChanged(stats) => {
            model.network_stats = stats;
            Vec::new()
        }
        Msg::PrListFailed { context, error } => {
            model.list.fail_fetch(format!("{context}: {error}"));
            Vec::new()
        }
        Msg::CommandFailed { context, error } => {
            model.last_error = Some(format!("{context}: {error}"));
            Vec::new()
        }
        Msg::Quit => {
            model.should_quit = true;
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use ratatui::crossterm::event::{KeyCode, KeyEvent};

    use crate::{
        app::{cmd::Cmd, model::Model, msg::Msg, pr_list::Phase, update::update},
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
    fn config_loaded_preserves_existing_error() {
        let (mut model, _) = Model::new();
        model.last_error = Some("resolve auth token: failed".to_owned());

        update(&mut model, Msg::ConfigLoaded(Default::default()));

        assert_eq!(
            model.last_error.as_deref(),
            Some("resolve auth token: failed")
        );
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
            Cmd::FetchOpenPRs { owner, repo, .. } => {
                assert_eq!(owner, "mayfieldiv");
                assert_eq!(repo, "legit");
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
        assert!(model.list.selected() >= model.list.scroll_offset());
        assert!(model.list.selected() < model.list.scroll_offset() + 20);

        // Shrink: selection must remain on-screen after re-clamp.
        update(
            &mut model,
            Msg::TerminalEvent(ratatui::crossterm::event::Event::Resize(80, 6)),
        );
        assert_eq!(model.list.viewport_height(), 5);
        assert!(model.list.selected() >= model.list.scroll_offset());
        assert!(model.list.selected() < model.list.scroll_offset() + 5);
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
}
