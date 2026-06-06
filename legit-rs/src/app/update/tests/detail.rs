use ratatui::crossterm::event::KeyCode;

use crate::{
    app::{cmd::Cmd, model::ViewMode, msg::Msg, update::update},
    git_remote::RepoInfo,
    github::rest::PrKey,
    secret::Secret,
};

/// The body string of the open detail view, or `None` if not in Detail mode or
/// the body hasn't arrived. Lets the tests assert on the consolidated
/// `ViewMode::Detail(DetailState)` shape without repeating the match.
fn detail_body(model: &crate::app::model::Model) -> Option<&str> {
    match &model.view_mode {
        ViewMode::Detail(detail) => detail.body.as_deref(),
        ViewMode::List => None,
    }
}

/// The scroll offset of the open detail view; panics if not in Detail mode.
fn detail_scroll(model: &crate::app::model::Model) -> u16 {
    match &model.view_mode {
        ViewMode::Detail(detail) => detail.scroll,
        ViewMode::List => panic!("expected Detail mode"),
    }
}

/// Set the open detail view's body; panics if not in Detail mode.
fn set_detail_body(model: &mut crate::app::model::Model, body: Option<String>) {
    match &mut model.view_mode {
        ViewMode::Detail(detail) => detail.body = body,
        ViewMode::List => panic!("expected Detail mode"),
    }
}

use super::{enriched_model, key_event};

/// A model with auth + repo detected and one PR streamed in and selected.
fn model_with_one_pr() -> crate::app::model::Model {
    let mut model = enriched_model(&[42]);
    model.config_loaded = true;
    model.auth_token = Some(Secret::new("ghp_test".to_owned()));
    model.repo = crate::app::model::RepoDetection::Detected(RepoInfo {
        owner: "mayfieldiv".to_owned(),
        repo: "legit".to_owned(),
    });
    model.list.complete_fetch("mayfieldiv/legit");
    model.relayout();
    model
}

fn pr_key_42() -> PrKey {
    PrKey {
        repo_slug: "mayfieldiv/legit".to_owned(),
        number: 42,
    }
}

/// A model entered into Detail with a tall body and a small viewport, so there
/// is ample room to scroll down before hitting the clamp. Used by the scroll
/// tests so they exercise pure step arithmetic, not the bottom-of-content
/// clamp (which has its own dedicated test).
fn scrollable_detail_model() -> crate::app::model::Model {
    let mut model = model_with_one_pr();
    // A short viewport so the 100-line body leaves a large max scroll.
    model.terminal_height = 10;
    update(&mut model, key_event(KeyCode::Enter));
    let body: String = (1..=100).map(|n| format!("Line {n}\n\n")).collect();
    set_detail_body(&mut model, Some(body));
    model
}

#[test]
fn enter_on_selected_pr_transitions_to_detail_and_dispatches_fetch() {
    let mut model = model_with_one_pr();
    assert_eq!(model.view_mode, ViewMode::List);

    let cmds = update(&mut model, key_event(KeyCode::Enter));

    match &model.view_mode {
        ViewMode::Detail(detail) => assert_eq!(
            detail.key,
            pr_key_42(),
            "view must switch to Detail for the selected PR"
        ),
        other => panic!("view must switch to Detail, got {other:?}"),
    }
    // The fetch command must be dispatched
    assert!(
        cmds.iter().any(|c| matches!(c, Cmd::FetchPRDetail { .. })),
        "FetchPRDetail must be dispatched on Enter: {cmds:?}"
    );
}

#[test]
fn enter_constructs_a_fresh_detail_state_with_no_body() {
    // Entering Detail builds a fresh `DetailState`, so there is no stale body to
    // clear by hand — the body starts `None` and the loading placeholder shows.
    let mut model = model_with_one_pr();

    update(&mut model, key_event(KeyCode::Enter));

    assert!(
        detail_body(&model).is_none(),
        "a freshly-entered detail view must have no body yet"
    );
}

#[test]
fn enter_into_detail_does_not_dispatch_list_files_fetch() {
    // The keypress starts in List but ends in Detail; the just-in-time files
    // fetch is a list-mode concern and must not fire for this keypress, even
    // though the selected PR's files were never requested.
    let mut model = model_with_one_pr();
    assert!(
        !model.enrichment.files.contains_key(&pr_key_42()),
        "precondition: files must not be requested yet"
    );

    let cmds = update(&mut model, key_event(KeyCode::Enter));

    assert!(
        matches!(model.view_mode, ViewMode::Detail(_)),
        "precondition: Enter must have entered Detail"
    );
    assert!(
        !cmds.iter().any(|c| matches!(c, Cmd::FetchFiles { .. })),
        "files fetch must not fire for a keypress that ended in Detail: {cmds:?}"
    );
}

#[test]
fn esc_in_detail_returns_to_list() {
    let mut model = model_with_one_pr();
    update(&mut model, key_event(KeyCode::Enter));
    assert!(matches!(model.view_mode, ViewMode::Detail(_)));

    let cmds = update(&mut model, key_event(KeyCode::Esc));

    assert_eq!(model.view_mode, ViewMode::List, "Esc must return to List");
    assert!(cmds.is_empty(), "Esc should not dispatch any command");
}

#[test]
fn esc_in_detail_drops_the_detail_state() {
    let mut model = model_with_one_pr();
    update(&mut model, key_event(KeyCode::Enter));
    set_detail_body(&mut model, Some("Some body".to_owned()));

    update(&mut model, key_event(KeyCode::Esc));

    assert_eq!(
        model.view_mode,
        ViewMode::List,
        "Esc must drop the whole DetailState (body included) and return to List"
    );
}

#[test]
fn pr_detail_arrived_stores_detail_when_still_in_detail_view() {
    let mut model = model_with_one_pr();
    update(&mut model, key_event(KeyCode::Enter));
    assert!(detail_body(&model).is_none(), "detail not yet arrived");

    let body = "The body".to_owned();
    update(
        &mut model,
        Msg::PRDetailArrived {
            key: pr_key_42(),
            body: body.clone(),
        },
    );

    assert_eq!(
        detail_body(&model),
        Some(body.as_str()),
        "arrived body must be stored"
    );
}

#[test]
fn pr_detail_arrived_discarded_after_navigating_back() {
    let mut model = model_with_one_pr();
    update(&mut model, key_event(KeyCode::Enter));
    // Navigate back before the fetch completes
    update(&mut model, key_event(KeyCode::Esc));
    assert_eq!(model.view_mode, ViewMode::List);

    update(
        &mut model,
        Msg::PRDetailArrived {
            key: pr_key_42(),
            body: "The body".to_owned(),
        },
    );

    assert_eq!(
        model.view_mode,
        ViewMode::List,
        "a late-arriving body for a closed view must be discarded"
    );
}

#[test]
fn r_in_detail_dispatches_refetch_and_clears_detail() {
    let mut model = model_with_one_pr();
    update(&mut model, key_event(KeyCode::Enter));
    set_detail_body(&mut model, Some("current body".to_owned()));

    let cmds = update(&mut model, key_event(KeyCode::Char('r')));

    // Detail cleared to show loading state again
    assert!(
        detail_body(&model).is_none(),
        "r must clear the detail to show the loading placeholder during refresh"
    );
    // Refetch dispatched
    assert!(
        cmds.iter().any(|c| matches!(c, Cmd::FetchPRDetail { .. })),
        "r must dispatch FetchPRDetail: {cmds:?}"
    );
    // Still in detail view
    assert!(
        matches!(model.view_mode, ViewMode::Detail(_)),
        "r must not exit the detail view"
    );
}

#[test]
fn r_in_list_mode_does_not_dispatch_fetch_pr_detail() {
    // 'r' in list mode is unbound (no handler). It must not accidentally
    // dispatch FetchPRDetail.
    let mut model = model_with_one_pr();
    assert_eq!(model.view_mode, ViewMode::List);

    let cmds = update(&mut model, key_event(KeyCode::Char('r')));

    assert!(
        !cmds.iter().any(|c| matches!(c, Cmd::FetchPRDetail { .. })),
        "r in list mode must not dispatch FetchPRDetail: {cmds:?}"
    );
}

#[test]
fn entering_detail_starts_scroll_at_zero() {
    let mut model = model_with_one_pr();

    update(&mut model, key_event(KeyCode::Enter));

    assert_eq!(
        detail_scroll(&model),
        0,
        "a freshly-entered detail view must start scrolled to the top"
    );
}

#[test]
fn j_in_detail_increments_scroll() {
    let mut model = scrollable_detail_model();
    assert_eq!(detail_scroll(&model), 0);

    update(&mut model, key_event(KeyCode::Char('j')));
    assert_eq!(detail_scroll(&model), 1, "j must scroll down by 1");

    update(&mut model, key_event(KeyCode::Char('j')));
    assert_eq!(detail_scroll(&model), 2, "second j must scroll down again");
}

#[test]
fn k_in_detail_decrements_scroll_and_clamps_at_zero() {
    let mut model = scrollable_detail_model();
    update(&mut model, key_event(KeyCode::Char('j')));
    update(&mut model, key_event(KeyCode::Char('j')));
    assert_eq!(detail_scroll(&model), 2);

    update(&mut model, key_event(KeyCode::Char('k')));
    assert_eq!(detail_scroll(&model), 1, "k must scroll up by 1");

    update(&mut model, key_event(KeyCode::Char('k')));
    update(&mut model, key_event(KeyCode::Char('k')));
    assert_eq!(
        detail_scroll(&model),
        0,
        "k must clamp at zero, not underflow"
    );
}

#[test]
fn page_down_in_detail_scrolls_by_ten() {
    let mut model = scrollable_detail_model();

    update(&mut model, key_event(KeyCode::PageDown));
    assert_eq!(detail_scroll(&model), 10, "PageDown must scroll down by 10");
}

#[test]
fn page_up_in_detail_scrolls_by_ten_and_clamps_at_zero() {
    let mut model = scrollable_detail_model();
    update(&mut model, key_event(KeyCode::PageDown));
    assert_eq!(detail_scroll(&model), 10);

    update(&mut model, key_event(KeyCode::PageUp));
    assert_eq!(detail_scroll(&model), 0, "PageUp must scroll up by 10");

    // Another PageUp from zero must not underflow.
    update(&mut model, key_event(KeyCode::PageUp));
    assert_eq!(
        detail_scroll(&model),
        0,
        "PageUp must clamp at zero, not underflow"
    );
}

#[test]
fn down_arrow_in_detail_increments_scroll() {
    let mut model = scrollable_detail_model();

    update(&mut model, key_event(KeyCode::Down));
    assert_eq!(detail_scroll(&model), 1, "Down arrow must scroll down by 1");
}

#[test]
fn up_arrow_in_detail_decrements_scroll() {
    let mut model = scrollable_detail_model();
    update(&mut model, key_event(KeyCode::Down));
    update(&mut model, key_event(KeyCode::Down));
    assert_eq!(detail_scroll(&model), 2);

    update(&mut model, key_event(KeyCode::Up));
    assert_eq!(detail_scroll(&model), 1, "Up arrow must scroll up by 1");
}

#[test]
fn esc_in_detail_drops_scroll_with_the_detail_state() {
    let mut model = scrollable_detail_model();
    // Scroll down a few lines.
    for _ in 0..5 {
        update(&mut model, key_event(KeyCode::Char('j')));
    }
    assert_eq!(detail_scroll(&model), 5);

    update(&mut model, key_event(KeyCode::Esc));

    // Esc drops the whole DetailState, so the next open structurally starts at
    // the top — there is no scroll field to leak across opens.
    assert_eq!(model.view_mode, ViewMode::List, "Esc returns to List");
}
