// ── repo tabs ─────────────────────────────────────────────────────────────

use super::*;

#[test]
fn l_and_h_step_tabs_with_wraparound() {
    let mut model = tabbed_model();
    assert_eq!(model.active_tab, 0);

    update(&mut model, key_event(KeyCode::Char('l')));
    assert_eq!(model.active_tab, 1);
    update(&mut model, key_event(KeyCode::Char('l')));
    assert_eq!(model.active_tab, 2);
    update(&mut model, key_event(KeyCode::Char('l')));
    assert_eq!(model.active_tab, 0, "l wraps past the last tab to All");

    update(&mut model, key_event(KeyCode::Char('h')));
    assert_eq!(model.active_tab, 2, "h wraps back from All to the last tab");
}

#[test]
fn brackets_and_arrows_also_step_tabs() {
    let mut model = tabbed_model();

    update(&mut model, key_event(KeyCode::Char(']')));
    assert_eq!(model.active_tab, 1);
    update(&mut model, key_event(KeyCode::Right));
    assert_eq!(model.active_tab, 2);
    update(&mut model, key_event(KeyCode::Char('[')));
    assert_eq!(model.active_tab, 1);
    update(&mut model, key_event(KeyCode::Left));
    assert_eq!(model.active_tab, 0);
}

#[test]
fn digits_jump_to_tabs_and_out_of_range_digits_are_ignored() {
    let mut model = tabbed_model();

    update(&mut model, key_event(KeyCode::Char('2')));
    assert_eq!(model.active_tab, 2);

    update(&mut model, key_event(KeyCode::Char('9')));
    assert_eq!(model.active_tab, 2, "digit past the last tab is ignored");

    update(&mut model, key_event(KeyCode::Char('0')));
    assert_eq!(model.active_tab, 0, "0 jumps to the All tab");
}

#[test]
fn tab_switch_scopes_the_list_and_resets_selection_to_top() {
    let mut model = tabbed_model();
    // All tab shows both PRs; move the selection off the top.
    update(&mut model, key_event(KeyCode::Char('j')));
    assert_eq!(model.list.selected(), 1);

    // Tab 2 = mayfieldiv/legit — only its PR (absolute index 1) is visible.
    update(&mut model, key_event(KeyCode::Char('2')));
    let visible: Vec<usize> = model.list.visible_pr_indices().collect();
    assert_eq!(visible, vec![1], "only the scoped repo's PR remains");
    assert_eq!(
        model.list.selected(),
        1,
        "selection sits on the tab's top PR"
    );

    // Tab 1 = acme/web — its PR is absolute index 0.
    update(&mut model, key_event(KeyCode::Char('1')));
    let visible: Vec<usize> = model.list.visible_pr_indices().collect();
    assert_eq!(visible, vec![0]);
    assert_eq!(
        model.list.selected(),
        0,
        "selection resets to top on tab change"
    );
}
