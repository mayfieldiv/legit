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
