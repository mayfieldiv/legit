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

fn model_with_all_search_fields_loaded() -> Model {
    let mut model = tabbed_model();
    let pr = model.list.pr_mut(&key(1)).expect("target PR");
    pr.labels.push(crate::github::rest::Label {
        name: "backend".to_owned(),
        color: None,
    });
    pr.requested_reviewers.push("alice".to_owned());
    update(
        &mut model,
        Msg::ReviewsArrived {
            pr: key(1),
            reviews: vec![crate::github::types::Review {
                user: "carol".to_owned(),
                state: "APPROVED".to_owned(),
            }],
        },
    );
    update(
        &mut model,
        Msg::FilesArrived {
            pr: key(1),
            files: vec![crate::file_category::FileChange {
                path: "src/search.rs".to_owned(),
                additions: 12,
                deletions: 3,
            }],
        },
    );
    update(
        &mut model,
        Msg::PRDetailArrived {
            pr: key(1),
            body: "Ready for the release train".to_owned(),
        },
    );
    model
}

#[test]
fn filter_matches_each_loaded_full_text_field() {
    for (field, needle, expected) in [
        ("label", "BACKEND", vec![1]),
        ("requested reviewer", "ALICE", vec![1]),
        ("reviewer", "CAROL", vec![1]),
        ("changed file path", "SRC/SEARCH.RS", vec![1]),
        ("description", "RELEASE TRAIN", vec![1]),
        ("no field", "not present", vec![]),
        ("empty needle", "", vec![0, 1]),
    ] {
        let mut model = model_with_all_search_fields_loaded();
        update(&mut model, key_event(KeyCode::Char('/')));
        type_filter(&mut model, needle);

        assert_eq!(visible(&model), expected, "{field} match");
    }
}

#[test]
fn filter_starts_matching_changed_paths_when_files_arrive() {
    let mut model = tabbed_model();
    update(&mut model, key_event(KeyCode::Char('/')));
    type_filter(&mut model, "migrations/123-add-users.sql");
    assert!(
        visible(&model).is_empty(),
        "unloaded files do not contribute"
    );

    update(
        &mut model,
        Msg::FilesArrived {
            pr: key(1),
            files: vec![crate::file_category::FileChange {
                path: "migrations/123-add-users.sql".to_owned(),
                additions: 12,
                deletions: 3,
            }],
        },
    );

    assert_eq!(
        visible(&model),
        vec![1],
        "the active filter relayouts when files arrive"
    );
}

#[test]
fn filter_starts_matching_description_when_body_arrives() {
    let mut model = tabbed_model();
    update(&mut model, key_event(KeyCode::Char('2')));
    update(&mut model, key_event(KeyCode::Enter));
    update(&mut model, key_event(KeyCode::Esc));

    update(&mut model, key_event(KeyCode::Char('/')));
    type_filter(&mut model, "release train");
    assert!(
        visible(&model).is_empty(),
        "an unloaded description does not contribute"
    );

    update(
        &mut model,
        Msg::PRDetailArrived {
            pr: key(1),
            body: "Ready for the Release Train".to_owned(),
        },
    );

    assert_eq!(
        visible(&model),
        vec![1],
        "the active filter relayouts when the description arrives"
    );
}

#[test]
fn typing_filter_text_never_fetches_enrichment() {
    let mut model = enriched_model(&[1, 2]);
    model.list.pr_mut(&key(1)).expect("PR 1").title = "alpha".to_owned();
    model.list.pr_mut(&key(2)).expect("PR 2").title = "beta".to_owned();
    model.relayout();

    let open_cmds = update(&mut model, key_event(KeyCode::Char('/')));
    assert!(
        open_cmds.is_empty(),
        "opening the in-memory filter must not fetch: {open_cmds:?}"
    );

    let cmds = update(&mut model, key_event(KeyCode::Char('b')));

    assert_eq!(visible(&model), vec![1], "the typed filter still relayouts");
    assert!(
        cmds.is_empty(),
        "typing must remain a pure in-memory operation: {cmds:?}"
    );
}

#[test]
fn filter_matches_pr_number() {
    // tabbed_model PRs: index 0 is #10 "web pr", index 1 is #1 "legit pr".
    let mut model = tabbed_model();

    update(&mut model, key_event(KeyCode::Char('/')));
    type_filter(&mut model, "10");
    assert_eq!(visible(&model), vec![0], "digits match the PR number");

    update(&mut model, key_event(KeyCode::Esc));
    update(&mut model, key_event(KeyCode::Char('/')));
    type_filter(&mut model, "#10");
    assert_eq!(
        visible(&model),
        vec![0],
        "a leading # is accepted with the number"
    );

    update(&mut model, key_event(KeyCode::Esc));
    update(&mut model, key_event(KeyCode::Char('/')));
    type_filter(&mut model, "1");
    assert_eq!(
        visible(&model),
        vec![0, 1],
        "partial number is a substring match (#1 and #10)"
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
    assert_eq!(
        model.list.viewport_height(),
        6,
        "app header + tab bar + table header + status bar"
    );

    update(&mut model, key_event(KeyCode::Char('/')));
    assert_eq!(model.list.viewport_height(), 5, "chip row takes one");

    update(&mut model, key_event(KeyCode::Esc));
    assert_eq!(model.list.viewport_height(), 6, "row returns on clear");
}
