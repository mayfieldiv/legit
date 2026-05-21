use crossterm::event::{Event, KeyCode, KeyEventKind};

use super::{cmd::Cmd, model::Model, msg::Msg};

/// Advance selection by one PR, clamped to the last row. No-op on an empty
/// list — keeps `selected = 0` as a safe sentinel.
fn move_selection_down(model: &mut Model) {
    if model.prs.is_empty() {
        return;
    }
    let last = model.prs.len() - 1;
    if model.selected < last {
        model.selected += 1;
    }
}

/// Retreat selection by one PR, clamped at the first row.
fn move_selection_up(model: &mut Model) {
    if model.selected > 0 {
        model.selected -= 1;
    }
}

/// Fire `Cmd::FetchOpenPRs` once both auth token and repo detection have
/// landed in the model. The repo defines what to fetch; the token authorizes
/// the request. Either alone yields no command — we wait for the second one.
fn maybe_fetch_open_prs(model: &Model) -> Vec<Cmd> {
    let (Some(token), Some(repo)) = (model.auth_token.as_ref(), model.repo.as_ref()) else {
        return Vec::new();
    };
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
                    KeyCode::Char('j') => move_selection_down(model),
                    KeyCode::Char('k') => move_selection_up(model),
                    _ => {}
                }
            }
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
            model.prs.push(pr);
            Vec::new()
        }
        Msg::PrListFailed { context, error } => {
            let message = format!("{context}: {error}");
            tracing::warn!(%message, "pr listing failed");
            model.list_error = Some(message);
            Vec::new()
        }
        Msg::CommandFailed { context, error } => {
            let message = format!("{context}: {error}");
            tracing::warn!(%message);
            model.last_error = Some(message);
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
    use crossterm::event::{KeyCode, KeyEvent};

    use crate::{
        app::{cmd::Cmd, model::Model, msg::Msg, update::update},
        git_remote::RepoInfo,
        github::rest::{PR, PRState},
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

    #[test]
    fn q_key_sets_should_quit() {
        let (mut model, _) = Model::new();

        update(
            &mut model,
            Msg::TerminalEvent(crossterm::event::Event::Key(KeyEvent::new(
                KeyCode::Char('q'),
                crossterm::event::KeyModifiers::NONE,
            ))),
        );

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
    fn pr_arrived_appends_to_open_pr_list() {
        let (mut model, _) = Model::new();

        let cmds = update(&mut model, Msg::PrArrived(sample_pr(42, "first")));

        assert_eq!(model.prs.len(), 1);
        assert_eq!(model.prs[0].number, 42);
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

        assert_eq!(
            model.repo.as_ref().map(|r| r.slug()),
            Some("mayfieldiv/legit".to_owned()),
        );
        assert!(
            cmds.is_empty(),
            "no fetch should fire before auth token resolves"
        );
    }

    #[test]
    fn repo_detected_after_token_dispatches_fetch_open_prs() {
        let (mut model, _) = Model::new();
        model.auth_token = Some("ghp_test".to_owned());

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

    fn key_event(code: KeyCode) -> Msg {
        Msg::TerminalEvent(crossterm::event::Event::Key(KeyEvent::new(
            code,
            crossterm::event::KeyModifiers::NONE,
        )))
    }

    #[test]
    fn j_advances_selection_within_list_bounds() {
        let (mut model, _) = Model::new();
        for n in 1..=3 {
            update(&mut model, Msg::PrArrived(sample_pr(n, "p")));
        }

        update(&mut model, key_event(KeyCode::Char('j')));
        assert_eq!(model.selected, 1);

        update(&mut model, key_event(KeyCode::Char('j')));
        assert_eq!(model.selected, 2);
    }

    #[test]
    fn j_at_last_pr_does_not_advance_past_end() {
        let (mut model, _) = Model::new();
        update(&mut model, Msg::PrArrived(sample_pr(1, "only")));

        update(&mut model, key_event(KeyCode::Char('j')));
        update(&mut model, key_event(KeyCode::Char('j')));

        assert_eq!(model.selected, 0);
    }

    #[test]
    fn k_retreats_selection_and_clamps_at_zero() {
        let (mut model, _) = Model::new();
        for n in 1..=3 {
            update(&mut model, Msg::PrArrived(sample_pr(n, "p")));
        }
        update(&mut model, key_event(KeyCode::Char('j')));
        update(&mut model, key_event(KeyCode::Char('j')));
        assert_eq!(model.selected, 2);

        update(&mut model, key_event(KeyCode::Char('k')));
        assert_eq!(model.selected, 1);

        update(&mut model, key_event(KeyCode::Char('k')));
        update(&mut model, key_event(KeyCode::Char('k')));
        assert_eq!(model.selected, 0);
    }

    #[test]
    fn streaming_prs_keep_selection_pinned() {
        let (mut model, _) = Model::new();
        update(&mut model, Msg::PrArrived(sample_pr(1, "a")));
        update(&mut model, Msg::PrArrived(sample_pr(2, "b")));
        update(&mut model, key_event(KeyCode::Char('j')));
        assert_eq!(model.selected, 1);

        update(&mut model, Msg::PrArrived(sample_pr(3, "c")));
        update(&mut model, Msg::PrArrived(sample_pr(4, "d")));

        assert_eq!(
            model.selected, 1,
            "selection should not shift when new PRs arrive"
        );
    }

    #[test]
    fn pr_list_failed_records_error_without_dropping_arrived_prs() {
        let (mut model, _) = Model::new();
        update(&mut model, Msg::PrArrived(sample_pr(1, "first")));

        let cmds = update(
            &mut model,
            Msg::PrListFailed {
                context: "list open PRs",
                error: "network down".to_owned(),
            },
        );

        assert_eq!(model.prs.len(), 1, "already-arrived PRs should remain");
        let error = model
            .list_error
            .as_deref()
            .expect("list_error should be recorded");
        assert!(error.contains("list open PRs"));
        assert!(error.contains("network down"));
        assert!(cmds.is_empty());
    }
}
