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
        chrono::DateTime::UNIX_EPOCH,
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
                    name: "web".to_owned(),
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
            unit: DiscoveryUnit::Cwd,
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
            incomplete: None,
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
            unit: unit.clone(),
            error: "main worktree /src/local-only does not exist".to_owned(),
        },
    );

    assert!(cmds.is_empty(), "{cmds:?}");
    assert_eq!(
        model.tickets.rail().collect::<Vec<_>>(),
        [RailCard::Failure {
            unit: &unit,
            error: "main worktree /src/local-only does not exist",
        }]
    );
    assert_eq!(
        model.status, None,
        "the rail card carries the error; the status bar is for transient failures"
    );
}

// ── GitHub map reads ──────────────────────────────────────────────────────

fn map_read_slugs(cmds: &[Cmd]) -> Vec<RepoSlug> {
    cmds.iter()
        .filter_map(|cmd| match cmd {
            Cmd::ReadGitHubEfforts { repo, .. } => Some(repo.clone()),
            _ => None,
        })
        .collect()
}

fn github_unit(slug: &str) -> DiscoveryUnit {
    DiscoveryUnit::GitHubRepo {
        slug: RepoSlug::new(slug),
    }
}

#[test]
fn github_map_reads_dispatch_once_auth_config_and_detection_settle() {
    let (mut model, _) = Model::new();
    update(&mut model, Msg::ConfigLoaded(discovery_config()));
    let cmds = update(
        &mut model,
        Msg::RepoDetected(Some(RepoSlug::new("mayfieldiv/legit"))),
    );
    assert!(
        map_read_slugs(&cmds).is_empty(),
        "a map read is an HTTP request, so unlike the local probes it waits on auth: {cmds:?}"
    );
    assert!(
        model.tickets.needs_discovery(&github_unit("acme/web")),
        "the unit must not read as in flight before its read dispatches"
    );

    let token = AuthToken::parse("ghp_test").unwrap();
    let cmds = update(&mut model, Msg::AuthTokenResolved(token.clone()));

    assert_eq!(
        map_read_slugs(&cmds),
        ["acme/web", "acme/slug-only", "mayfieldiv/legit"],
        "one map read per PR-capable Tracked Repo — configured slugs in config order, then \
         the cwd repo; a slug-less repo has no GitHub tracker to read: {cmds:?}"
    );
    assert!(cmds.contains(&Cmd::ReadGitHubEfforts {
        repo: RepoSlug::new("acme/web"),
        token,
    }));
    assert!(
        !model
            .tickets
            .needs_discovery(&github_unit("acme/slug-only"))
    );
    assert!(model.tickets.is_loading());

    // A config reload (`R`) must not re-read maps already in flight or loaded.
    let cmds = update(&mut model, Msg::ConfigLoaded(discovery_config()));
    assert!(map_read_slugs(&cmds).is_empty(), "{cmds:?}");
}

#[test]
fn config_landing_last_releases_the_map_reads_with_the_pr_listings() {
    let (mut model, _) = Model::new();
    model.auth_token = Some(AuthToken::parse("ghp_test").unwrap());
    update(&mut model, Msg::RepoDetected(None));

    let cmds = update(
        &mut model,
        Msg::ConfigLoaded(config_with_repos(&["acme/api"])),
    );

    assert_eq!(fetched_slugs(&cmds), ["acme/api"]);
    assert_eq!(map_read_slugs(&cmds), ["acme/api"]);
}

#[test]
fn a_failed_map_read_is_recorded_on_the_queue_under_its_repo() {
    let (mut model, _) = Model::new();
    model.auth_token = Some(AuthToken::parse("ghp_test").unwrap());
    model.config_loaded = true;
    update(
        &mut model,
        Msg::RepoDetected(Some(RepoSlug::new("mayfieldiv/legit"))),
    );
    assert!(model.tickets.is_loading());

    let cmds = update(
        &mut model,
        Msg::DiscoveryFailed {
            unit: github_unit("mayfieldiv/legit"),
            error: "GitHub GraphQL error: 404 Not Found".to_owned(),
        },
    );

    assert!(cmds.is_empty(), "{cmds:?}");
    assert_eq!(
        model.tickets.rail().collect::<Vec<_>>(),
        [RailCard::Failure {
            unit: &github_unit("mayfieldiv/legit"),
            error: "GitHub GraphQL error: 404 Not Found",
        }]
    );
    assert!(
        model
            .tickets
            .needs_discovery(&github_unit("mayfieldiv/legit")),
        "a failed read retries on the next gate release"
    );
    assert_eq!(model.status, None);
}

#[test]
fn an_incomplete_map_read_settles_its_unit_and_records_the_caveat_on_the_queue() {
    let (mut model, _) = Model::new();
    model.auth_token = Some(AuthToken::parse("ghp_test").unwrap());
    model.config_loaded = true;
    update(
        &mut model,
        Msg::RepoDetected(Some(RepoSlug::new("immense/immybot"))),
    );

    let cmds = update(
        &mut model,
        Msg::DiscoveryFinished {
            unit: github_unit("immense/immybot"),
            incomplete: Some("more than 10 open maps; showing the first 10".to_owned()),
        },
    );

    assert!(cmds.is_empty(), "{cmds:?}");
    assert_eq!(
        model.tickets.rail().collect::<Vec<_>>(),
        [RailCard::Incomplete {
            unit: &github_unit("immense/immybot"),
            caveat: "more than 10 open maps; showing the first 10",
        }]
    );
    assert!(
        !model
            .tickets
            .needs_discovery(&github_unit("immense/immybot")),
        "the read settled — a config reload must not re-read the same window"
    );
    assert_eq!(model.status, None);
}

#[test]
fn r_refreshes_the_selected_local_effort_once_until_it_settles() {
    let mut model = ticket_model(&["01-a", "02-b"]);

    let cmds = update(&mut model, key_event(KeyCode::Char('r')));

    assert_eq!(
        cmds.iter().map(Cmd::name).collect::<Vec<_>>(),
        ["ReadLocalEffort"]
    );
    update(&mut model, key_event(KeyCode::Char('j')));
    assert!(update(&mut model, key_event(KeyCode::Char('r'))).is_empty());
}

fn github_read(map_number: u64, number: u64) -> EffortRead {
    github_read_in("acme/web", map_number, number)
}

fn github_read_in(slug: &str, map_number: u64, number: u64) -> EffortRead {
    let slug = RepoSlug::new(slug);
    EffortRead::Ready(
        Effort::new(
            EffortKey::GitHub {
                repo_slug: slug.clone(),
                map_number,
            },
            format!("Map {map_number}"),
            None,
            vec![Ticket {
                key: TicketKey::GitHub {
                    repo_slug: slug,
                    number,
                },
                title: format!("Ticket {number}"),
                state: TicketState::Open,
                claim: None,
                ty: TicketType("task".to_owned()),
                dependencies: Vec::new(),
            }],
        )
        .unwrap(),
    )
}

#[test]
fn a_github_refresh_shares_one_read_across_maps_and_can_repeat_after_completion() {
    let (mut model, _) = Model::new();
    model.auth_token = Some(AuthToken::parse("test").unwrap());
    for (map, number) in [(10, 11), (20, 21)] {
        update(
            &mut model,
            Msg::EffortArrived {
                unit: github_unit("acme/web"),
                repo: RepoIdentity::Slug(RepoSlug::new("acme/web")),
                read: github_read(map, number),
            },
        );
    }
    model.view_mode = ViewMode::TicketList;
    let cmds = update(&mut model, key_event(KeyCode::Char('r')));
    assert_eq!(map_read_slugs(&cmds), ["acme/web"]);
    update(&mut model, key_event(KeyCode::Char('j')));
    assert!(update(&mut model, key_event(KeyCode::Char('r'))).is_empty());

    for (map, number) in [(10, 11), (20, 21)] {
        update(
            &mut model,
            Msg::EffortArrived {
                unit: github_unit("acme/web"),
                repo: RepoIdentity::Slug(RepoSlug::new("acme/web")),
                read: github_read(map, number),
            },
        );
    }
    update(
        &mut model,
        Msg::DiscoveryFinished {
            unit: github_unit("acme/web"),
            incomplete: None,
        },
    );

    assert_eq!(model.status.as_ref().unwrap().text, "Refreshed 2 efforts");
    assert_eq!(
        map_read_slugs(&update(&mut model, key_event(KeyCode::Char('r')))),
        ["acme/web"]
    );
}

#[test]
fn refresh_all_covers_each_unit_once_including_efforts_with_no_open_tickets() {
    let mut model = ticket_model(&["01-a"]);
    model.auth_token = Some(AuthToken::parse("test").unwrap());
    for (map, number) in [(10, 11), (20, 21)] {
        update(
            &mut model,
            Msg::EffortArrived {
                unit: github_unit("acme/web"),
                repo: RepoIdentity::Slug(RepoSlug::new("acme/web")),
                read: github_read(map, number),
            },
        );
    }
    update(
        &mut model,
        Msg::EffortArrived {
            unit: github_unit("acme/web"),
            repo: RepoIdentity::Slug(RepoSlug::new("acme/web")),
            read: EffortRead::Ready(
                Effort::new(
                    EffortKey::Local {
                        dir: CanonicalPathBuf::assume_canonical("/w/empty"),
                    },
                    "Empty".to_owned(),
                    None,
                    Vec::new(),
                )
                .unwrap(),
            ),
        },
    );

    let cmds = update(&mut model, key_event(KeyCode::Char('R')));

    assert_eq!(map_read_slugs(&cmds), ["acme/web"]);
    let mut dirs = cmds
        .iter()
        .filter_map(|cmd| match cmd {
            Cmd::ReadLocalEffort { dir, .. } => Some(dir.display().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>();
    dirs.sort();
    assert_eq!(dirs, ["/w/alpha", "/w/empty"]);
    assert!(update(&mut model, key_event(KeyCode::Char('R'))).is_empty());
}

fn first_effort_card(model: &Model) -> &crate::app::ticket_list::EffortCard {
    model
        .tickets
        .rail()
        .find_map(|card| match card {
            RailCard::Effort(card) => Some(card),
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_failed_github_refresh_keeps_stale_tickets_and_age_and_allows_retry() {
    let (mut model, _) = Model::new();
    model.auth_token = Some(AuthToken::parse("test").unwrap());
    model.view_mode = ViewMode::TicketList;
    update(
        &mut model,
        Msg::EffortArrived {
            unit: github_unit("acme/web"),
            repo: RepoIdentity::Slug(RepoSlug::new("acme/web")),
            read: github_read(10, 11),
        },
    );
    assert_eq!(
        first_effort_card(&model).fetch.fetched_at,
        Some(fixed_now())
    );
    update(&mut model, key_event(KeyCode::Char('r')));
    assert!(first_effort_card(&model).fetch.refreshing);

    let cmds = update_at(
        &mut model,
        Msg::DiscoveryFailed {
            unit: github_unit("acme/web"),
            error: "offline".to_owned(),
        },
        fixed_now() + chrono::Duration::minutes(5),
    );

    assert_eq!(selected_ref(&model), Some("#11".to_owned()));
    assert_eq!(
        first_effort_card(&model).fetch.fetched_at,
        Some(fixed_now())
    );
    assert!(!first_effort_card(&model).fetch.refreshing);
    assert_eq!(model.status.as_ref().unwrap().kind, StatusKind::Error);
    assert!(cmds.iter().any(|cmd| matches!(
        cmd,
        Cmd::ScheduleStatusClear {
            delay_ms: 8_000,
            ..
        }
    )));
    assert_eq!(
        map_read_slugs(&update(&mut model, key_event(KeyCode::Char('r')))),
        ["acme/web"]
    );
}

#[test]
fn a_malformed_local_refresh_preserves_data_until_a_success_reconciles_membership() {
    let mut model = ticket_model(&["01-a", "02-b"]);
    let dir = CanonicalPathBuf::assume_canonical("/w/alpha");
    let repo = RepoIdentity::Slug(RepoSlug::new("acme/web"));
    update(&mut model, key_event(KeyCode::Char('r')));
    update(
        &mut model,
        Msg::LocalEffortRead {
            dir: dir.clone(),
            repo: repo.clone(),
            result: Ok(EffortRead::Degraded {
                key: EffortKey::Local { dir: dir.clone() },
                title: "Alpha".to_owned(),
                destination: None,
                reason: "missing status".to_owned(),
            }),
        },
    );
    assert_eq!(
        first_effort_card(&model).fetch.fetched_at,
        Some(chrono::DateTime::UNIX_EPOCH)
    );
    assert_eq!(selected_ref(&model), Some("01-a".to_owned()));
    assert!(
        model
            .status
            .as_ref()
            .unwrap()
            .text
            .contains("missing status")
    );
    assert!(!update(&mut model, key_event(KeyCode::Char('r'))).is_empty());

    let read = EffortRead::Ready(
        Effort::new(
            EffortKey::Local { dir: dir.clone() },
            "Alpha".to_owned(),
            None,
            vec![Ticket {
                key: local_ticket_key("02-b"),
                title: "Survivor".to_owned(),
                state: TicketState::Open,
                claim: None,
                ty: TicketType("task".to_owned()),
                dependencies: Vec::new(),
            }],
        )
        .unwrap(),
    );
    update(
        &mut model,
        Msg::LocalEffortRead {
            dir,
            repo,
            result: Ok(read),
        },
    );
    assert_eq!(selected_ref(&model), Some("02-b".to_owned()));
    assert_eq!(
        first_effort_card(&model).fetch.fetched_at,
        Some(fixed_now())
    );
    assert!(!first_effort_card(&model).fetch.refreshing);
    assert_eq!(model.status.as_ref().unwrap().text, "Refreshed 1 effort");
}

#[test]
fn a_complete_map_read_removes_absent_maps_but_an_incomplete_read_retains_them() {
    let mut model = ticket_model(&["01-local"]);
    model.auth_token = Some(AuthToken::parse("test").unwrap());
    let repo = RepoIdentity::Slug(RepoSlug::new("acme/web"));
    for (map, number) in [(10, 11), (20, 21)] {
        update(
            &mut model,
            Msg::EffortArrived {
                unit: github_unit("acme/web"),
                repo: repo.clone(),
                read: github_read(map, number),
            },
        );
    }
    for incomplete in [Some("more maps outside the window".to_owned()), None] {
        update(&mut model, key_event(KeyCode::Char('R')));
        update(
            &mut model,
            Msg::EffortArrived {
                unit: github_unit("acme/web"),
                repo: repo.clone(),
                read: github_read(20, 21),
            },
        );
        update(
            &mut model,
            Msg::DiscoveryFinished {
                unit: github_unit("acme/web"),
                incomplete: incomplete.clone(),
            },
        );
        let refs = model
            .tickets
            .visible_rows()
            .filter_map(|(row, _)| match row {
                crate::app::ticket_list::QueueRow::Ticket(ticket) => {
                    Some(ticket.display_ref.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(
            refs.contains(&"01-local".to_owned()),
            "other Fetch Units stay pooled"
        );
        assert!(refs.contains(&"#21".to_owned()));
        assert_eq!(refs.contains(&"#11".to_owned()), incomplete.is_some());
    }
}

#[test]
fn r_retries_startup_failures_even_when_another_effort_has_the_selection() {
    let mut model = ticket_model(&["01-a"]);
    model.auth_token = Some(AuthToken::parse("test").unwrap());
    update(
        &mut model,
        Msg::DiscoveryFailed {
            unit: github_unit("acme/api"),
            error: "404".to_owned(),
        },
    );
    update(
        &mut model,
        Msg::EffortArrived {
            unit: DiscoveryUnit::Cwd,
            repo: RepoIdentity::Slug(RepoSlug::new("acme/web")),
            read: EffortRead::Degraded {
                key: EffortKey::Local {
                    dir: CanonicalPathBuf::assume_canonical("/w/broken"),
                },
                title: "Broken".to_owned(),
                destination: None,
                reason: "missing status".to_owned(),
            },
        },
    );

    let cmds = update(&mut model, key_event(KeyCode::Char('r')));

    assert_eq!(map_read_slugs(&cmds), ["acme/api"]);
    assert_eq!(
        cmds.iter()
            .filter(|cmd| matches!(cmd, Cmd::ReadLocalEffort { .. }))
            .count(),
        2
    );
    assert!(update(&mut model, key_event(KeyCode::Char('r'))).is_empty());
}

#[test]
fn all_successful_efforts_in_one_map_read_share_its_settlement_time() {
    let (mut model, _) = Model::new();
    model.auth_token = Some(AuthToken::parse("test").unwrap());
    model.config_loaded = true;
    update(
        &mut model,
        Msg::RepoDetected(Some(RepoSlug::new("acme/web"))),
    );
    for (seconds, map, number) in [(1, 10, 11), (2, 20, 21)] {
        update_at(
            &mut model,
            Msg::EffortArrived {
                unit: github_unit("acme/web"),
                repo: RepoIdentity::Slug(RepoSlug::new("acme/web")),
                read: github_read(map, number),
            },
            fixed_now() + chrono::Duration::seconds(seconds),
        );
    }
    let settled = fixed_now() + chrono::Duration::seconds(5);
    update_at(
        &mut model,
        Msg::DiscoveryFinished {
            unit: github_unit("acme/web"),
            incomplete: None,
        },
        settled,
    );

    let ages = model
        .tickets
        .rail()
        .filter_map(|card| match card {
            RailCard::Effort(card) => Some(card.fetch.fetched_at),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(ages, [Some(settled), Some(settled)]);
}

#[test]
fn refresh_all_rechecks_a_github_unit_that_previously_had_no_maps() {
    let (mut model, _) = Model::new();
    model.auth_token = Some(AuthToken::parse("test").unwrap());
    model.view_mode = ViewMode::TicketList;
    update(
        &mut model,
        Msg::DiscoveryFinished {
            unit: github_unit("acme/web"),
            incomplete: None,
        },
    );

    let cmds = update(&mut model, key_event(KeyCode::Char('R')));

    assert_eq!(map_read_slugs(&cmds), ["acme/web"]);
    assert!(update(&mut model, key_event(KeyCode::Char('R'))).is_empty());
}

#[test]
fn r_with_no_selected_ticket_re_reads_nothing_while_shift_r_still_rechecks_the_view() {
    let (mut model, _) = Model::new();
    model.auth_token = Some(AuthToken::parse("test").unwrap());
    model.view_mode = ViewMode::TicketList;
    update(
        &mut model,
        Msg::DiscoveryFinished {
            unit: github_unit("acme/web"),
            incomplete: None,
        },
    );
    assert_eq!(selected_ref(&model), None);

    assert!(update(&mut model, key_event(KeyCode::Char('r'))).is_empty());
    assert_eq!(
        map_read_slugs(&update(&mut model, key_event(KeyCode::Char('R')))),
        ["acme/web"]
    );
}

#[test]
fn a_failed_unit_retried_while_another_is_in_flight_still_settles_the_run() {
    let (mut model, _) = Model::new();
    model.auth_token = Some(AuthToken::parse("test").unwrap());
    model.view_mode = ViewMode::TicketList;
    for slug in ["acme/api", "acme/web"] {
        update(
            &mut model,
            Msg::EffortArrived {
                unit: github_unit(slug),
                repo: RepoIdentity::Slug(RepoSlug::new(slug)),
                read: github_read_in(slug, 10, 11),
            },
        );
    }
    assert_eq!(
        map_read_slugs(&update(&mut model, key_event(KeyCode::Char('R')))),
        ["acme/api", "acme/web"]
    );
    update(
        &mut model,
        Msg::DiscoveryFailed {
            unit: github_unit("acme/api"),
            error: "offline".to_owned(),
        },
    );
    assert_eq!(model.status.as_ref().unwrap().kind, StatusKind::Error);

    assert_eq!(
        map_read_slugs(&update(&mut model, key_event(KeyCode::Char('R')))),
        ["acme/api"],
        "web is still in flight"
    );
    for slug in ["acme/api", "acme/web"] {
        update(
            &mut model,
            Msg::EffortArrived {
                unit: github_unit(slug),
                repo: RepoIdentity::Slug(RepoSlug::new(slug)),
                read: github_read_in(slug, 10, 11),
            },
        );
        update(
            &mut model,
            Msg::DiscoveryFinished {
                unit: github_unit(slug),
                incomplete: None,
            },
        );
    }

    assert_eq!(model.status.as_ref().unwrap().text, "Refreshed 2 efforts");
}

#[test]
fn an_incomplete_refresh_counts_only_the_efforts_it_read() {
    let (mut model, _) = Model::new();
    model.auth_token = Some(AuthToken::parse("test").unwrap());
    model.view_mode = ViewMode::TicketList;
    let repo = RepoIdentity::Slug(RepoSlug::new("acme/web"));
    for (map, number) in [(10, 11), (20, 21)] {
        update(
            &mut model,
            Msg::EffortArrived {
                unit: github_unit("acme/web"),
                repo: repo.clone(),
                read: github_read(map, number),
            },
        );
    }
    update(&mut model, key_event(KeyCode::Char('r')));
    update(
        &mut model,
        Msg::EffortArrived {
            unit: github_unit("acme/web"),
            repo,
            read: github_read(20, 21),
        },
    );
    update(
        &mut model,
        Msg::DiscoveryFinished {
            unit: github_unit("acme/web"),
            incomplete: Some("more maps outside the window".to_owned()),
        },
    );
    assert_eq!(
        model
            .tickets
            .rail()
            .filter(|card| matches!(card, RailCard::Effort(_)))
            .count(),
        2
    );
    assert_eq!(model.status.as_ref().unwrap().text, "Refreshed 1 effort");
}

fn model_with_failed_local_discovery() -> (Model, DiscoveryUnit) {
    let mut model = ticket_model(&["01-a"]);
    model.config = discovery_config();
    let unit = DiscoveryUnit::for_repo(&model.config.repos[1]).unwrap();
    update(
        &mut model,
        Msg::DiscoveryFailed {
            unit: unit.clone(),
            error: "missing worktree".to_owned(),
        },
    );
    (model, unit)
}

#[test]
fn a_local_discovery_retry_keeps_the_refresh_pending_until_it_settles() {
    let (mut model, unit) = model_with_failed_local_discovery();
    let cmds = update(&mut model, key_event(KeyCode::Char('r')));
    assert_eq!(discovery_cmds(&cmds).len(), 1);
    assert!(update(&mut model, key_event(KeyCode::Char('r'))).is_empty());
    let repo = RepoIdentity::Slug(RepoSlug::new("acme/web"));
    let dir = CanonicalPathBuf::assume_canonical("/w/alpha");
    let read = EffortRead::Ready(
        Effort::new(
            EffortKey::Local { dir: dir.clone() },
            "Alpha".to_owned(),
            None,
            Vec::new(),
        )
        .unwrap(),
    );
    update(
        &mut model,
        Msg::LocalEffortRead {
            dir,
            repo: repo.clone(),
            result: Ok(read),
        },
    );
    assert_eq!(model.status, None, "the recovery probe still runs");
    let read = EffortRead::Ready(
        Effort::new(
            EffortKey::Local {
                dir: CanonicalPathBuf::assume_canonical("/w/recovered"),
            },
            "Recovered".to_owned(),
            None,
            Vec::new(),
        )
        .unwrap(),
    );
    update(
        &mut model,
        Msg::EffortArrived {
            unit: unit.clone(),
            repo,
            read,
        },
    );
    update(
        &mut model,
        Msg::DiscoveryFinished {
            unit,
            incomplete: None,
        },
    );
    assert_eq!(model.status.as_ref().unwrap().text, "Refreshed 2 efforts");
}

#[test]
fn a_failed_local_discovery_retry_posts_an_error_and_suppresses_later_success() {
    let (mut model, unit) = model_with_failed_local_discovery();
    update(&mut model, key_event(KeyCode::Char('r')));
    let cmds = update(
        &mut model,
        Msg::DiscoveryFailed {
            unit,
            error: "still missing".to_owned(),
        },
    );
    assert!(cmds.iter().any(|cmd| matches!(
        cmd,
        Cmd::ScheduleStatusClear {
            delay_ms: 8_000,
            ..
        }
    )));
    let dir = CanonicalPathBuf::assume_canonical("/w/alpha");
    let read = EffortRead::Ready(
        Effort::new(
            EffortKey::Local { dir: dir.clone() },
            "Alpha".to_owned(),
            None,
            Vec::new(),
        )
        .unwrap(),
    );
    update(
        &mut model,
        Msg::LocalEffortRead {
            dir,
            repo: RepoIdentity::Slug(RepoSlug::new("acme/web")),
            result: Ok(read),
        },
    );
    assert_eq!(model.status.as_ref().unwrap().kind, StatusKind::Error);
    assert!(
        model
            .status
            .as_ref()
            .unwrap()
            .text
            .contains("still missing")
    );
}
