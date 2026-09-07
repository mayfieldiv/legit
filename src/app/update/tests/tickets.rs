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
    model.auth_token = Some(Secret::new("token".to_owned()));
    model.list.push(sample_pr(1, "One"));
    model.list.push(sample_pr(2, "Two"));
    model.relayout();
    let before = model.list.selected();

    let cmds = update(&mut model, key_event(KeyCode::Char('j')));

    assert!(cmds.is_empty(), "{cmds:?}");
    assert_eq!(model.list.selected(), before);
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
