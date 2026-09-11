// ── ticket surface ─────────────────────────────────────────────────────────

use super::*;
use crate::app::model::ViewMode;

#[test]
fn t_toggles_between_the_pr_list_and_the_ticket_list() {
    let (mut model, _) = Model::new();
    assert_eq!(model.view_mode, ViewMode::List);

    update(&mut model, key_event(KeyCode::Char('t')));
    assert_eq!(model.view_mode, ViewMode::TicketList);

    update(&mut model, key_event(KeyCode::Char('t')));
    assert_eq!(model.view_mode, ViewMode::List);
}

#[test]
fn t_in_the_ticket_list_does_not_touch_the_detail_filters() {
    let (mut model, _) = Model::new();
    assert!(!model.show_resolved);

    update(&mut model, key_event(KeyCode::Char('t')));
    update(&mut model, key_event(KeyCode::Char('t')));

    assert!(
        !model.show_resolved,
        "the surface toggle must not share the detail view's resolved-threads `t`"
    );
}

#[test]
fn q_quits_from_the_ticket_list() {
    let (mut model, _) = Model::new();
    update(&mut model, key_event(KeyCode::Char('t')));

    update(&mut model, key_event(KeyCode::Char('q')));

    assert!(model.should_quit);
}

// ── queue cursor ──────────────────────────────────────────────────────────

use crate::{
    canonical_path::CanonicalPathBuf,
    config::RepoIdentity,
    github::limiter::Affinity,
    ticket::{Effort, EffortKey, EffortRead, Ticket, TicketKey, TicketState, TicketType},
};

fn local_ticket_key(slug: &str) -> TicketKey {
    TicketKey::Local {
        path: CanonicalPathBuf::assume_canonical(format!("/w/alpha/tickets/{slug}.md")),
    }
}

/// A model on the ticket surface with one local Effort of `slugs` open,
/// unclaimed, dependency-free Tickets — every one on the Frontier.
fn ticket_model(slugs: &[&str]) -> Model {
    let (mut model, _) = Model::new();
    let effort = Effort::new(
        EffortKey::Local {
            dir: CanonicalPathBuf::assume_canonical("/w/alpha"),
        },
        "Alpha".to_owned(),
        None,
        slugs
            .iter()
            .map(|slug| Ticket {
                key: local_ticket_key(slug),
                title: format!("Ticket {slug}"),
                state: TicketState::Open,
                claim: None,
                ty: TicketType("task".to_owned()),
                dependencies: Vec::new(),
            })
            .collect(),
    )
    .unwrap();
    model.tickets.merge_effort(
        RepoIdentity::Slug(RepoSlug::new("acme/web")),
        EffortRead::Ready(effort),
    );
    update(&mut model, key_event(KeyCode::Char('t')));
    model
}

fn selected_ref(model: &Model) -> Option<String> {
    model.tickets.selected_ticket().map(TicketKey::display_ref)
}

#[test]
fn j_and_k_move_the_queue_cursor_on_the_ticket_surface() {
    let mut model = ticket_model(&["01-a", "02-b", "03-c"]);
    assert_eq!(selected_ref(&model), Some("01-a".to_owned()));

    let cmds = update(&mut model, key_event(KeyCode::Char('j')));
    assert!(
        cmds.is_empty(),
        "cursor movement is network-silent: {cmds:?}"
    );
    assert_eq!(selected_ref(&model), Some("02-b".to_owned()));
    update(&mut model, key_event(KeyCode::Down));
    assert_eq!(selected_ref(&model), Some("03-c".to_owned()));
    update(&mut model, key_event(KeyCode::Char('k')));
    assert_eq!(selected_ref(&model), Some("02-b".to_owned()));
    update(&mut model, key_event(KeyCode::Up));
    assert_eq!(selected_ref(&model), Some("01-a".to_owned()));
}

#[test]
fn ticket_surface_keys_never_move_the_pr_selection_or_fetch_files() {
    let mut model = ticket_model(&["01-a", "02-b"]);
    model.auth_token = Some(AuthToken::parse("token").unwrap());
    model.list.push(sample_pr(1, "One"));
    model.list.push(sample_pr(2, "Two"));
    model.relayout();
    let before = selected_number(&model);

    let cmds = update(&mut model, key_event(KeyCode::Char('j')));

    assert!(cmds.is_empty(), "{cmds:?}");
    assert_eq!(selected_number(&model), before);
    assert!(model.enrichment.files.is_empty());
}

#[test]
fn the_selected_ticket_is_the_focused_entity_while_the_ticket_surface_is_active() {
    let mut model = ticket_model(&["01-a", "02-b"]);
    update(&mut model, key_event(KeyCode::Char('j')));

    assert_eq!(
        model.focused_entity(),
        Some(Affinity::Ticket(local_ticket_key("02-b")))
    );

    // Toggling back restores the PR surface's focus (none here — no PRs).
    update(&mut model, key_event(KeyCode::Char('t')));
    assert_eq!(model.focused_entity(), None);
}

#[test]
fn an_empty_queue_focuses_nothing() {
    let (mut model, _) = Model::new();
    update(&mut model, key_event(KeyCode::Char('t')));
    assert_eq!(model.focused_entity(), None);
}

#[test]
fn a_resize_sizes_the_queue_viewport_by_the_ticket_chrome() {
    let mut model = ticket_model(&["01-a"]);

    update(
        &mut model,
        Msg::TerminalEvent(ratatui::crossterm::event::Event::Resize(120, 20)),
    );

    assert_eq!(
        model.tickets.viewport_height(),
        20 - crate::app::ticket_list_layout::chrome_rows()
    );
}

// ── local discovery ───────────────────────────────────────────────────────

use crate::{
    app::ticket_list::{DiscoveryUnit, RailCard},
    config::{LegitConfig, RepoConfig},
};

fn discovery_config() -> LegitConfig {
    LegitConfig {
        repos: vec![
            RepoConfig {
                slug: Some(RepoSlug::parse("acme/web").unwrap()),
                main_worktree_path: Some("/src/web".to_owned()),
                ..Default::default()
            },
            RepoConfig {
                main_worktree_path: Some("/src/local-only".to_owned()),
                ..Default::default()
            },
            RepoConfig {
                slug: Some(RepoSlug::parse("acme/slug-only").unwrap()),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}

fn discovery_cmds(cmds: &[Cmd]) -> Vec<&Cmd> {
    cmds.iter()
        .filter(|cmd| {
            matches!(
                cmd,
                Cmd::DiscoverRepoEfforts { .. } | Cmd::DiscoverCwdEfforts { .. }
            )
        })
        .collect()
}

#[test]
fn local_discovery_dispatches_once_config_and_repo_detection_settle() {
    let (mut model, _) = Model::new();

    let cmds = update(&mut model, Msg::ConfigLoaded(discovery_config()));
    assert!(
        discovery_cmds(&cmds).is_empty(),
        "detection hasn't settled, so the cwd walk can't be attributed yet: {cmds:?}"
    );
    assert!(!model.tickets.is_loading());

    let cmds = update(
        &mut model,
        Msg::RepoDetected(Some(RepoSlug::new("mayfieldiv/legit"))),
    );
    let discovery = discovery_cmds(&cmds);
    assert_eq!(
        discovery,
        vec![
            &Cmd::DiscoverRepoEfforts {
                unit: DiscoveryUnit::LocalRepo {
                    name: "acme/web".to_owned(),
                    main_worktree_path: "/src/web".to_owned()
                },
                repo: discovery_config().repos[0].clone(),
            },
            &Cmd::DiscoverRepoEfforts {
                unit: DiscoveryUnit::LocalRepo {
                    name: "local-only".to_owned(),
                    main_worktree_path: "/src/local-only".to_owned()
                },
                repo: discovery_config().repos[1].clone(),
            },
            &Cmd::DiscoverCwdEfforts {
                detected: Some(RepoSlug::new("mayfieldiv/legit")),
                config: discovery_config(),
            },
        ],
        "one task per Tracked Repo with a Main Worktree (a slug-only repo has no \
         filesystem to probe), plus the cwd walk; no auth token needed"
    );
    assert!(model.tickets.is_loading());

    // A config reload (`R`) must not re-probe units already in flight or loaded.
    let cmds = update(&mut model, Msg::ConfigLoaded(discovery_config()));
    assert!(discovery_cmds(&cmds).is_empty(), "{cmds:?}");
}

#[test]
fn a_failed_repo_detection_still_walks_the_cwd_unattributed() {
    let (mut model, _) = Model::new();
    update(&mut model, Msg::ConfigLoaded(LegitConfig::default()));

    let cmds = update(&mut model, Msg::RepoDetected(None));

    assert_eq!(
        discovery_cmds(&cmds),
        vec![&Cmd::DiscoverCwdEfforts {
            detected: None,
            config: LegitConfig::default(),
        }]
    );
}

#[test]
fn effort_arrivals_pool_and_probe_settlement_clears_loading() {
    let (mut model, _) = Model::new();
    update(&mut model, Msg::ConfigLoaded(LegitConfig::default()));
    update(&mut model, Msg::RepoDetected(None));
    assert!(model.tickets.is_loading());

    let effort = Effort::new(
        EffortKey::Local {
            dir: CanonicalPathBuf::assume_canonical("/w/alpha"),
        },
        "Alpha".to_owned(),
        None,
        vec![Ticket {
            key: local_ticket_key("01-a"),
            title: "A".to_owned(),
            state: TicketState::Open,
            claim: None,
            ty: TicketType("task".to_owned()),
            dependencies: Vec::new(),
        }],
    )
    .unwrap();
    let cmds = update(
        &mut model,
        Msg::EffortArrived {
            repo: RepoIdentity::Path(CanonicalPathBuf::assume_canonical("/w")),
            read: EffortRead::Ready(effort),
        },
    );
    assert!(cmds.is_empty());
    assert_eq!(model.tickets.rail().count(), 1);
    assert_eq!(selected_ref(&model), Some("01-a".to_owned()));

    update(
        &mut model,
        Msg::DiscoveryFinished {
            unit: DiscoveryUnit::Cwd,
        },
    );
    assert!(!model.tickets.is_loading());
}

#[test]
fn a_failed_probe_is_recorded_on_the_queue_not_as_a_status_error() {
    let (mut model, _) = Model::new();
    update(&mut model, Msg::ConfigLoaded(discovery_config()));
    update(&mut model, Msg::RepoDetected(None));
    let unit = DiscoveryUnit::LocalRepo {
        name: "local-only".to_owned(),
        main_worktree_path: "/src/local-only".to_owned(),
    };

    let cmds = update(
        &mut model,
        Msg::DiscoveryFailed {
            unit,
            error: "main worktree /src/local-only does not exist".to_owned(),
        },
    );

    assert!(cmds.is_empty(), "{cmds:?}");
    assert_eq!(
        model.tickets.rail().collect::<Vec<_>>(),
        [RailCard::Failure {
            unit: "local-only",
            error: "main worktree /src/local-only does not exist",
        }]
    );
    assert_eq!(
        model.status, None,
        "the rail card carries the error; the status bar is for transient failures"
    );
}
