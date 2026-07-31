use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use crate::{
    app::{msg::Msg, update::update},
    palette::DARK,
};

use super::*;

/// A deliberately tall, fully enriched summary matching the regression shape
/// from #95: Fetch Age plus every bottom section at a 24-row terminal.
fn fully_enriched_short_model() -> Model {
    let mut model = fully_loaded_model();
    let key = model.list.selected_pr().expect("a PR is selected").key();
    if let Some(pr) = model.list.pr_mut(&key) {
        pr.requested_reviewers = vec!["dana".to_owned(), "erin".to_owned()];
        pr.labels = vec![
            label("enhancement", Some("a2eeef")),
            label("ready-for-agent", Some("0e8a16")),
            label("needs-design-review", Some("d4c5f9")),
        ];
        pr.assignees = vec!["bottom-owner".to_owned()];
    }
    with_reviews(
        &mut model,
        vec![
            review("alice", "APPROVED"),
            review("bob", "CHANGES_REQUESTED"),
            review("carol", "COMMENTED"),
        ],
    );
    with_checks(
        &mut model,
        "abc123",
        (0..16)
            .map(|i| check(&format!("check-{i:02}"), "completed", Some("success")))
            .collect(),
    );
    model.stamp_fetched(key, fixed_now() - chrono::Duration::minutes(3));
    model
}

#[test]
fn fully_enriched_short_panel_can_scroll_to_assignees_with_visible_overflow() {
    let mut model = fully_enriched_short_model();
    update(
        &mut model,
        Msg::TerminalEvent(Event::Resize(140, 24)),
        fixed_now(),
    );

    let top_rows = panel_rows(&model, 140, 24);
    assert!(
        !top_rows.join("\n").contains("bottom-owner"),
        "the short viewport begins above the bottom section: {top_rows:?}"
    );
    assert!(
        top_rows.last().is_some_and(|row| row.contains("more ↓")),
        "the last visible row must say that more summary content exists: {top_rows:?}"
    );
    assert!(
        top_rows
            .last()
            .is_some_and(|row| row.trim_end().ends_with('↓')),
        "the affordance must clear the clipped row instead of leaving stale text after it: {top_rows:?}"
    );
    let top_buffer = frame_buffer(&model, 140, 24);
    assert_eq!(
        top_buffer[(panel_split_x(140), 22)].fg,
        DARK.muted,
        "the overflow affordance is visually de-emphasised"
    );

    for _ in 0..10 {
        update(
            &mut model,
            Msg::TerminalEvent(Event::Key(KeyEvent::new(
                KeyCode::PageDown,
                KeyModifiers::NONE,
            ))),
            fixed_now(),
        );
    }

    let bottom_rows = panel_rows(&model, 140, 24);
    assert!(
        bottom_rows.join("\n").contains("assignees: bottom-owner"),
        "the bottom-most summary section is reachable: {bottom_rows:?}"
    );
    assert!(
        !bottom_rows.iter().any(|row| row.contains("more ↓")),
        "the overflow affordance disappears at the end: {bottom_rows:?}"
    );
}
