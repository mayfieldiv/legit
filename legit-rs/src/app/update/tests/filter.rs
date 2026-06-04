// ── substring filter ──────────────────────────────────────────────────────

use super::*;

/// Type each char of `text` into the open filter editor.
fn type_filter(model: &mut Model, text: &str) {
    for c in text.chars() {
        update(model, key_event(KeyCode::Char(c)));
    }
}

/// Absolute indices of the PRs the current layout shows.
fn visible(model: &Model) -> Vec<usize> {
    model.list.visible_pr_indices().collect()
}

#[test]
fn slash_opens_filter_editing_and_typed_keys_append() {
    let mut model = tabbed_model();

    update(&mut model, key_event(KeyCode::Char('/')));
    assert!(model.list.filter().is_editing());

    type_filter(&mut model, "web");
    assert_eq!(model.list.filter().text(), "web");
}

#[test]
fn filter_matches_title_and_author_case_insensitively() {
    // tabbed_model PRs: index 0 "web pr", index 1 "legit pr", both by octocat.
    let mut model = tabbed_model();

    update(&mut model, key_event(KeyCode::Char('/')));
    type_filter(&mut model, "WEB");
    assert_eq!(visible(&model), vec![0], "title match is case-insensitive");

    update(&mut model, key_event(KeyCode::Esc));
    update(&mut model, key_event(KeyCode::Char('/')));
    type_filter(&mut model, "OCTO");
    assert_eq!(
        visible(&model),
        vec![0, 1],
        "author match is case-insensitive"
    );
}

#[test]
fn editing_consumes_every_key_instead_of_dispatching_normal_mode() {
    let mut model = tabbed_model();
    update(&mut model, key_event(KeyCode::Char('/')));

    update(&mut model, key_event(KeyCode::Char('2')));
    assert_eq!(model.active_tab, 0, "digits type, they don't switch tabs");

    update(&mut model, key_event(KeyCode::Char('h')));
    assert_eq!(model.active_tab, 0, "h types, it doesn't switch tabs");

    update(&mut model, key_event(KeyCode::Char('q')));
    assert!(!model.should_quit, "q types, it doesn't quit");

    assert_eq!(model.list.filter().text(), "2hq");
}

#[test]
fn enter_applies_the_filter_and_normal_keys_resume() {
    let mut model = tabbed_model();
    update(&mut model, key_event(KeyCode::Char('/')));
    type_filter(&mut model, "web");

    update(&mut model, key_event(KeyCode::Enter));

    assert!(!model.list.filter().is_editing());
    assert_eq!(model.list.filter().text(), "web", "filter stays applied");
    assert_eq!(visible(&model), vec![0], "matches stay narrowed");

    // Normal-mode keys work again: digits switch tabs.
    update(&mut model, key_event(KeyCode::Char('2')));
    assert_eq!(model.active_tab, 2);
}

#[test]
fn enter_with_empty_text_deactivates_the_filter() {
    let mut model = tabbed_model();
    update(&mut model, key_event(KeyCode::Char('/')));

    update(&mut model, key_event(KeyCode::Enter));

    assert!(!model.list.filter().is_visible());
}

#[test]
fn esc_while_editing_clears_the_filter() {
    let mut model = tabbed_model();
    update(&mut model, key_event(KeyCode::Char('/')));
    type_filter(&mut model, "web");
    assert_eq!(visible(&model), vec![0]);

    update(&mut model, key_event(KeyCode::Esc));

    assert!(!model.list.filter().is_visible());
    assert_eq!(visible(&model), vec![0, 1], "the full list returns");
}

#[test]
fn esc_clears_an_applied_filter_from_normal_mode() {
    let mut model = tabbed_model();
    update(&mut model, key_event(KeyCode::Char('/')));
    type_filter(&mut model, "web");
    update(&mut model, key_event(KeyCode::Enter));
    assert!(model.list.filter().is_visible());

    update(&mut model, key_event(KeyCode::Esc));

    assert!(!model.list.filter().is_visible());
    assert_eq!(visible(&model), vec![0, 1]);
}

#[test]
fn backspace_deletes_and_refilters_live() {
    let mut model = tabbed_model();
    update(&mut model, key_event(KeyCode::Char('/')));
    type_filter(&mut model, "webx");
    assert!(visible(&model).is_empty(), "no PR matches 'webx'");

    update(&mut model, key_event(KeyCode::Backspace));

    assert_eq!(model.list.filter().text(), "web");
    assert_eq!(visible(&model), vec![0], "matches return as text shrinks");
}

#[test]
fn filter_composes_with_the_active_tab_scope() {
    let mut model = tabbed_model();
    // Tab 2 = mayfieldiv/legit; "pr" matches both titles but the scope keeps
    // only that repo's PR.
    update(&mut model, key_event(KeyCode::Char('2')));
    update(&mut model, key_event(KeyCode::Char('/')));
    type_filter(&mut model, "pr");
    assert_eq!(visible(&model), vec![1]);

    // "web" only matches the other tab's PR — nothing here.
    type_filter(&mut model, "x");
    assert!(visible(&model).is_empty());
    assert!(
        model
            .list
            .filter_hid_everything(model.active_scope().as_deref())
    );
}

#[test]
fn filter_chip_row_shrinks_the_viewport_and_clearing_restores_it() {
    let mut model = tabbed_model();
    update(
        &mut model,
        Msg::TerminalEvent(ratatui::crossterm::event::Event::Resize(80, 10)),
    );
    assert_eq!(model.list.viewport_height(), 8, "tab bar + status bar");

    update(&mut model, key_event(KeyCode::Char('/')));
    assert_eq!(model.list.viewport_height(), 7, "chip row takes one");

    update(&mut model, key_event(KeyCode::Esc));
    assert_eq!(model.list.viewport_height(), 8, "row returns on clear");
}
