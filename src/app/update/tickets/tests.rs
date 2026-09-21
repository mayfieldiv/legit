use crate::{
    app::{
        cmd::Cmd,
        model::{Model, ViewMode},
        ticket_list::QueueRow,
        update::tests::{config_with_repos, fixed_now, key_event, update},
    },
    auth::AuthToken,
    config::RepoIdentity,
    repo_slug::RepoSlug,
    ticket::{Effort, EffortKey, EffortRead, Ticket, TicketKey, TicketState, TicketType},
};
use ratatui::crossterm::event::KeyCode;

fn model() -> Model {
    let (mut model, _) = Model::new();
    model.config = config_with_repos(&["acme/web", "acme/api"]);
    model.auth_token = Some(AuthToken::parse("test-token").unwrap());
    model.view_mode = ViewMode::TicketList;
    for (repo, map, title, types) in [
        ("acme/web", 10, "Alpha", vec!["research", "grilling"]),
        ("acme/web", 20, "Beta", vec!["task"]),
        ("acme/api", 30, "API", vec!["task"]),
    ] {
        let repo_slug = RepoSlug::new(repo);
        let tickets = types
            .into_iter()
            .enumerate()
            .map(|(i, ty)| Ticket {
                key: TicketKey::GitHub {
                    repo_slug: repo_slug.clone(),
                    number: map + i as u64 + 1,
                },
                title: format!("{title} {ty}"),
                state: TicketState::Open,
                claim: None,
                updated_at: None,
                ty: TicketType(ty.to_owned()),
                dependencies: vec![],
            })
            .collect();
        model.tickets.merge_effort(
            RepoIdentity::Slug(repo_slug.clone()),
            EffortRead::Ready(
                Effort::new(
                    EffortKey::GitHub {
                        repo_slug,
                        map_number: map,
                    },
                    title.to_owned(),
                    Some(format!("{title} destination")),
                    tickets,
                )
                .unwrap(),
            ),
            fixed_now(),
        );
    }
    model
}

fn refs(model: &Model) -> Vec<String> {
    model
        .tickets
        .visible_rows()
        .filter_map(|(row, _)| match row {
            QueueRow::Ticket(row) => Some(row.display_ref.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn wheel_over_the_queue_scrolls_without_changing_selection_or_fetching() {
    let mut model = model();
    update(
        &mut model,
        crate::app::msg::Msg::TerminalEvent(ratatui::crossterm::event::Event::Resize(140, 7)),
    );
    let selected = model.tickets.selected_ticket().cloned();
    let cmds = update(
        &mut model,
        crate::app::update::tests::wheel_event_at(true, 80, 5),
    );
    assert!(cmds.is_empty());
    assert_eq!(model.tickets.selected_ticket(), selected.as_ref());
    assert!(model.tickets.scroll_offset() > 0);
    let offset = model.tickets.scroll_offset();
    update(
        &mut model,
        crate::app::update::tests::wheel_event_at(false, 0, 5),
    );
    assert_eq!(
        model.tickets.scroll_offset(),
        offset,
        "the rail is not the queue viewport"
    );
}

#[test]
fn copy_keys_emit_self_contained_github_and_local_references() {
    let mut model = model();
    let cmds = update(&mut model, key_event(KeyCode::Char('p')));
    assert!(
        matches!(cmds.as_slice(), [Cmd::CopyToClipboard { text }] if text == "API task | /wayfinder https://github.com/acme/api/issues/30 - resolve https://github.com/acme/api/issues/31")
    );
    let cmds = update(&mut model, key_event(KeyCode::Char('y')));
    assert!(
        matches!(cmds.as_slice(), [Cmd::CopyToClipboard { text }] if text == "https://github.com/acme/api/issues/31")
    );

    let (mut local, _) = Model::new();
    local.view_mode = ViewMode::TicketList;
    local.tickets.merge_effort(
        RepoIdentity::Slug(RepoSlug::new("acme/web")),
        EffortRead::Ready(
            Effort::new(
                EffortKey::Local {
                    dir: crate::canonical_path::CanonicalPathBuf::assume_canonical(
                        "/work/my effort",
                    ),
                },
                "Local map".to_owned(),
                None,
                vec![Ticket {
                    key: TicketKey::Local {
                        path: crate::canonical_path::CanonicalPathBuf::assume_canonical(
                            "/work/my effort/issues/01-design.md",
                        ),
                    },
                    title: "Design".to_owned(),
                    state: TicketState::Open,
                    claim: None,
                    updated_at: None,
                    ty: TicketType("task".to_owned()),
                    dependencies: vec![],
                }],
            )
            .unwrap(),
        ),
        fixed_now(),
    );
    let cmds = update(&mut local, key_event(KeyCode::Char('p')));
    assert!(
        matches!(cmds.as_slice(), [Cmd::CopyToClipboard { text }] if text == "Design | /wayfinder /work/my effort - resolve /work/my effort/issues/01-design.md")
    );
    let cmds = update(&mut local, key_event(KeyCode::Char('y')));
    assert!(
        matches!(cmds.as_slice(), [Cmd::CopyToClipboard { text }] if text == "/work/my effort/issues/01-design.md")
    );
}

#[test]
fn tab_effort_and_mode_keys_filter_without_fetching_and_refresh_uses_that_scope() {
    let mut model = model();
    assert!(update(&mut model, key_event(KeyCode::Char('l'))).is_empty());
    assert_eq!(refs(&model), ["#11", "#12", "#21"]);
    assert!(update(&mut model, key_event(KeyCode::Char('J'))).is_empty());
    assert_eq!(refs(&model), ["#11", "#12"]);
    assert!(update(&mut model, key_event(KeyCode::Char('m'))).is_empty());
    assert_eq!(refs(&model), ["#11"]);
    let cmds = update(&mut model, key_event(KeyCode::Char('R')));
    assert!(
        matches!(cmds.as_slice(), [Cmd::ReadGitHubEfforts { repo, .. }] if repo.as_str() == "acme/web")
    );
    assert!(update(&mut model, key_event(KeyCode::Char('l'))).is_empty());
    assert_eq!(refs(&model), ["#31"]);
    assert!(update(&mut model, key_event(KeyCode::Char('h'))).is_empty());
    assert_eq!(
        refs(&model),
        ["#11", "#21"],
        "tab changes reset the effort filter and retain mode"
    );
    update(&mut model, key_event(KeyCode::Char('t')));
    assert!(matches!(model.view_mode, ViewMode::List));
    assert_eq!(model.active_scope().unwrap().as_str(), "acme/web");
}
