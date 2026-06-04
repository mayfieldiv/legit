use ratatui::crossterm::event::KeyCode;

use crate::{
    app::{cmd::Cmd, model::ViewMode, msg::Msg, update::update},
    git_remote::RepoInfo,
    github::rest::{PRDetail, PrKey},
    secret::Secret,
};

use super::{enriched_model, key_event, sample_pr};

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

#[test]
fn enter_on_selected_pr_transitions_to_detail_and_dispatches_fetch() {
    let mut model = model_with_one_pr();
    assert_eq!(model.view_mode, ViewMode::List);

    let cmds = update(&mut model, key_event(KeyCode::Enter));

    assert_eq!(
        model.view_mode,
        ViewMode::Detail(pr_key_42()),
        "view must switch to Detail for the selected PR"
    );
    // The fetch command must be dispatched
    assert!(
        cmds.iter().any(|c| matches!(c, Cmd::FetchPRDetail { .. })),
        "FetchPRDetail must be dispatched on Enter: {cmds:?}"
    );
}

#[test]
fn enter_on_selected_pr_clears_stale_detail() {
    let mut model = model_with_one_pr();
    // Pre-seed a stale detail from a previous open
    model.detail = Some(PRDetail {
        pr: sample_pr(42, "old"),
        body: "stale body".to_owned(),
    });

    update(&mut model, key_event(KeyCode::Enter));

    assert!(
        model.detail.is_none(),
        "stale detail must be cleared when entering a new detail view"
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
fn esc_in_detail_clears_the_fetched_detail() {
    let mut model = model_with_one_pr();
    update(&mut model, key_event(KeyCode::Enter));
    model.detail = Some(PRDetail {
        pr: sample_pr(42, "Add streaming PR list"),
        body: "Some body".to_owned(),
    });

    update(&mut model, key_event(KeyCode::Esc));

    assert!(
        model.detail.is_none(),
        "detail must be cleared when returning to list"
    );
}

#[test]
fn pr_detail_arrived_stores_detail_when_still_in_detail_view() {
    let mut model = model_with_one_pr();
    update(&mut model, key_event(KeyCode::Enter));
    assert!(model.detail.is_none(), "detail not yet arrived");

    let detail = PRDetail {
        pr: sample_pr(42, "Add streaming PR list"),
        body: "The body".to_owned(),
    };
    update(&mut model, Msg::PRDetailArrived(detail.clone()));

    assert_eq!(
        model.detail.as_ref(),
        Some(&detail),
        "arrived detail must be stored"
    );
}

#[test]
fn pr_detail_arrived_discarded_after_navigating_back() {
    let mut model = model_with_one_pr();
    update(&mut model, key_event(KeyCode::Enter));
    // Navigate back before the fetch completes
    update(&mut model, key_event(KeyCode::Esc));
    assert_eq!(model.view_mode, ViewMode::List);

    let detail = PRDetail {
        pr: sample_pr(42, "Add streaming PR list"),
        body: "The body".to_owned(),
    };
    update(&mut model, Msg::PRDetailArrived(detail));

    assert!(
        model.detail.is_none(),
        "a late-arriving detail for a closed view must be discarded"
    );
}

#[test]
fn r_in_detail_dispatches_refetch_and_clears_detail() {
    let mut model = model_with_one_pr();
    update(&mut model, key_event(KeyCode::Enter));
    model.detail = Some(PRDetail {
        pr: sample_pr(42, "Add streaming PR list"),
        body: "current body".to_owned(),
    });

    let cmds = update(&mut model, key_event(KeyCode::Char('r')));

    // Detail cleared to show loading state again
    assert!(
        model.detail.is_none(),
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
