use crate::repo_slug::RepoSlug;
use ratatui::crossterm::event::{Event, KeyCode};

use crate::app::msg::Msg;

use super::{enriched_model, key_event, update, wheel_event_at};

#[test]
fn wheel_over_the_summary_scrolls_it_instead_of_the_list_viewport() {
    let mut model = enriched_model(&[1, 2, 3, 4, 5, 6]);
    model
        .list
        .complete_fetch(&RepoSlug::new("mayfieldiv/legit"));
    model.relayout();
    update(&mut model, Msg::TerminalEvent(Event::Resize(100, 8)));
    assert_eq!(model.list.selected_pr().unwrap().number, 1);
    assert_eq!(model.list.scroll_offset(), 0);
    assert_eq!(model.summary.offset(), 0);

    update(&mut model, wheel_event_at(true, 70, 2));

    assert_eq!(
        model.summary.offset(),
        3,
        "wheel-down over the panel scrolls its content"
    );
    assert_eq!(
        model.list.scroll_offset(),
        0,
        "the Open PR List viewport stays put"
    );
    assert_eq!(
        model.list.selected_pr().unwrap().number,
        1,
        "wheel scrolling never moves list selection"
    );

    update(&mut model, wheel_event_at(false, 70, 2));
    assert_eq!(
        model.summary.offset(),
        0,
        "wheel-up over the panel scrolls back toward its top"
    );
}

#[test]
fn page_keys_scroll_the_summary_without_moving_selection_or_passing_the_end() {
    let mut model = enriched_model(&[1, 2]);
    model
        .list
        .complete_fetch(&RepoSlug::new("mayfieldiv/legit"));
    model.relayout();
    update(&mut model, Msg::TerminalEvent(Event::Resize(100, 8)));
    assert_eq!(model.list.selected_pr().unwrap().number, 1);
    assert_eq!(model.list.scroll_offset(), 0);

    update(&mut model, key_event(KeyCode::PageDown));

    assert_eq!(
        model.list.selected_pr().unwrap().number,
        1,
        "PageDown must not move the selected PR"
    );
    assert_eq!(
        model.list.scroll_offset(),
        0,
        "PageDown must not scroll the Open PR List"
    );
    assert_eq!(
        model.summary.offset(),
        6,
        "PageDown clamps to the summary's last screenful"
    );
    update(&mut model, key_event(KeyCode::PageDown));
    assert_eq!(
        model.summary.offset(),
        6,
        "holding PageDown cannot drift past the summary's end"
    );

    update(&mut model, key_event(KeyCode::PageUp));
    assert_eq!(
        model.summary.offset(),
        0,
        "PageUp scrolls toward the top without underflow"
    );
}

#[test]
fn changing_the_selected_pr_resets_the_summary_to_the_top() {
    let mut model = enriched_model(&[1, 2]);
    model
        .list
        .complete_fetch(&RepoSlug::new("mayfieldiv/legit"));
    model.relayout();
    update(&mut model, Msg::TerminalEvent(Event::Resize(100, 8)));
    update(&mut model, key_event(KeyCode::PageDown));
    assert!(model.summary.offset() > 0, "precondition: summary scrolled");

    update(&mut model, key_event(KeyCode::Char('j')));

    assert_eq!(model.list.selected_pr().unwrap().number, 2);
    assert_eq!(
        model.summary.offset(),
        0,
        "a different selected PR always starts at the top of its summary"
    );
}
