use super::{Direction, ListCursor, SelectableRow, Viewport};

impl SelectableRow for Option<u8> {
    type Id = u8;
    fn id(&self) -> Option<&u8> {
        self.as_ref()
    }
}

/// A display layout of headers (`None`) and items (`Some(id)`).
const ROWS: [Option<u8>; 6] = [None, Some(1), Some(2), None, Some(3), None];

/// A flat layout of `n` items, ids `1..=n`, so display row `i` is item `i + 1`.
fn items(n: u8) -> Vec<Option<u8>> {
    (1..=n).map(Some).collect()
}

fn selected(cursor: &ListCursor<u8>) -> Option<u8> {
    cursor.selected().copied()
}

/// The ids on screen, with the selected one wrapped in brackets.
fn screen(cursor: &ListCursor<u8>, rows: &[Option<u8>]) -> Vec<String> {
    cursor
        .visible_rows(rows)
        .map(|(row, selected)| match (row, selected) {
            (Some(id), true) => format!("[{id}]"),
            (Some(id), false) => id.to_string(),
            (None, _) => "──".to_owned(),
        })
        .collect()
}

// ── viewport ─────────────────────────────────────────────────────────────────

#[test]
fn an_unsized_viewport_shows_every_row() {
    let viewport = Viewport::default();
    assert_eq!(viewport.window(6), 0..6);
    assert_eq!(viewport.window(0), 0..0);
}

#[test]
fn the_window_never_extends_past_the_last_row() {
    let mut viewport = Viewport {
        offset: 100,
        height: 4,
    };
    viewport.clamp(10);
    assert_eq!(viewport.window(10), 6..10, "clamped to the last screenful");

    viewport.clamp(5);
    assert_eq!(
        viewport.window(5),
        1..5,
        "fewer rows: the window pulls back"
    );
}

#[test]
fn follow_keeps_the_selected_row_inside_the_window_with_a_margin() {
    let mut viewport = Viewport {
        offset: 0,
        height: 10,
    };

    viewport.follow(9, 20);
    assert!(
        viewport.window(20).contains(&9) && viewport.offset >= 1,
        "moving into the bottom margin advances the window: {viewport:?}"
    );

    viewport.follow(0, 20);
    assert_eq!(viewport.offset, 0);
}

#[test]
fn a_single_row_viewport_shows_exactly_the_selected_row() {
    let mut viewport = Viewport {
        offset: 0,
        height: 1,
    };
    for row in 0..5 {
        viewport.follow(row, 10);
        assert_eq!(viewport.window(10), row..row + 1);
    }
}

// ── cursor ───────────────────────────────────────────────────────────────────

#[test]
fn the_selection_follows_the_top_item_until_the_user_navigates() {
    let mut cursor = ListCursor::default();
    assert_eq!(selected(&cursor), None);

    cursor.re_anchor(&ROWS);
    assert_eq!(selected(&cursor), Some(1));

    // A re-sort puts another item on top: the never-touched cursor follows.
    let resorted = [Some(2), Some(1), Some(3)];
    cursor.re_anchor(&resorted);
    assert_eq!(selected(&cursor), Some(2));

    // Once the user has moved, the selection sticks to its item.
    cursor.step(&resorted, Direction::Down);
    assert_eq!(selected(&cursor), Some(1));
    cursor.re_anchor(&[Some(3), Some(1), Some(2)]);
    assert_eq!(selected(&cursor), Some(1));
}

#[test]
fn a_pinned_selection_whose_item_vanished_snaps_to_the_top() {
    let mut cursor = ListCursor::default();
    cursor.re_anchor(&ROWS);
    cursor.step(&ROWS, Direction::Down);
    assert_eq!(selected(&cursor), Some(2));

    cursor.re_anchor(&[None, Some(3), Some(1)]);

    assert_eq!(selected(&cursor), Some(3));
}

#[test]
fn stepping_skips_headers_and_clamps_at_the_ends() {
    let mut cursor = ListCursor::default();
    cursor.re_anchor(&ROWS);

    cursor.step(&ROWS, Direction::Down);
    assert_eq!(selected(&cursor), Some(2));
    cursor.step(&ROWS, Direction::Down);
    assert_eq!(selected(&cursor), Some(3), "past the header");
    cursor.step(&ROWS, Direction::Down);
    assert_eq!(selected(&cursor), Some(3), "clamps at the last item");

    cursor.step(&ROWS, Direction::Up);
    cursor.step(&ROWS, Direction::Up);
    assert_eq!(selected(&cursor), Some(1));
    cursor.step(&ROWS, Direction::Up);
    assert_eq!(selected(&cursor), Some(1), "clamps at the first item");
}

#[test]
fn an_empty_list_selects_nothing() {
    let mut cursor = ListCursor::<u8>::default();
    let empty: [Option<u8>; 0] = [];
    cursor.re_anchor(&empty);
    cursor.step(&empty, Direction::Down);
    assert_eq!(selected(&cursor), None);
    assert_eq!(cursor.visible_rows(&empty).count(), 0);

    let headers_only = [None, None];
    cursor.re_anchor(&headers_only);
    assert_eq!(selected(&cursor), None, "a header is never selected");
}

#[test]
fn the_window_follows_keyboard_movement_and_survives_a_shrink() {
    let rows = items(20);
    let mut cursor = ListCursor::default();
    cursor.resize(&rows, 5);
    cursor.re_anchor(&rows);
    assert_eq!(screen(&cursor, &rows), ["[1]", "2", "3", "4", "5"]);

    for _ in 0..10 {
        cursor.step(&rows, Direction::Down);
    }
    assert_eq!(selected(&cursor), Some(11));
    let on_screen = screen(&cursor, &rows);
    assert_eq!(on_screen.len(), 5);
    assert!(
        on_screen.contains(&"[11]".to_owned()),
        "the window followed the selection: {on_screen:?}"
    );

    cursor.resize(&rows, 2);
    assert!(
        screen(&cursor, &rows).contains(&"[11]".to_owned()),
        "a shrink re-clamps around the selection: {:?}",
        screen(&cursor, &rows)
    );
}

#[test]
fn wheel_scrolling_detaches_the_window_until_the_user_selects_again() {
    let rows = items(20);
    let mut cursor = ListCursor::default();
    cursor.resize(&rows, 5);
    cursor.re_anchor(&rows);

    cursor.scroll_down(&rows, 3);
    assert_eq!(cursor.offset(), 3);
    assert_eq!(
        selected(&cursor),
        Some(1),
        "the wheel never moves the selection"
    );
    assert_eq!(screen(&cursor, &rows), ["4", "5", "6", "7", "8"]);

    // Background re-anchors and resizes preserve the detached window.
    cursor.re_anchor(&rows);
    assert_eq!(cursor.offset(), 3);
    cursor.resize(&rows, 5);
    assert_eq!(cursor.offset(), 3);

    cursor.scroll_down(&rows, 100);
    assert_eq!(cursor.offset(), 15, "stops at the last screenful");
    cursor.scroll_up(&rows, 100);
    assert_eq!(cursor.offset(), 0);

    // Keyboard movement re-attaches: the window follows again.
    cursor.scroll_down(&rows, 10);
    cursor.step(&rows, Direction::Down);
    assert_eq!(selected(&cursor), Some(2));
    assert!(
        screen(&cursor, &rows).contains(&"[2]".to_owned()),
        "{:?}",
        screen(&cursor, &rows)
    );
}

#[test]
fn a_wheel_event_over_an_empty_or_unsized_list_is_not_engagement() {
    // Unsized: the window shows everything, so there is nothing to scroll.
    let mut cursor = ListCursor::default();
    cursor.scroll_down(&items(3), 2);
    assert_eq!(cursor.offset(), 0);
    cursor.re_anchor(&[Some(9), Some(1)]);
    assert_eq!(selected(&cursor), Some(9), "still following the top");

    // Sized but empty: items streaming in afterwards must not find the
    // cursor detached and pinned to the first arrival.
    let mut cursor = ListCursor::default();
    let empty: [Option<u8>; 0] = [];
    cursor.resize(&empty, 5);
    cursor.scroll_up(&empty, 3);
    cursor.re_anchor(&[Some(1)]);
    cursor.re_anchor(&[Some(2), Some(1)]);
    assert_eq!(selected(&cursor), Some(2));
}

#[test]
fn clicking_a_visible_row_selects_its_item_and_ignores_headers() {
    let mut cursor = ListCursor::default();
    cursor.resize(&ROWS, 3);
    cursor.re_anchor(&ROWS);

    assert!(cursor.select_visible_row(&ROWS, 2));
    assert_eq!(selected(&cursor), Some(2));
    assert!(!cursor.select_visible_row(&ROWS, 0), "a header");
    assert_eq!(selected(&cursor), Some(2));
    assert!(!cursor.select_visible_row(&ROWS, 99), "past the end");

    // Visible rows are counted from the window's first row.
    cursor.scroll_down(&ROWS, 3);
    assert_eq!(cursor.offset(), 3);
    assert!(cursor.select_visible_row(&ROWS, 1));
    assert_eq!(selected(&cursor), Some(3));
    assert_eq!(cursor.offset(), 3, "a click leaves the window where it is");
}

#[test]
fn reset_to_top_unpins_the_cursor_and_scrolls_to_the_top() {
    let rows = items(20);
    let mut cursor = ListCursor::default();
    cursor.resize(&rows, 5);
    cursor.re_anchor(&rows);
    for _ in 0..10 {
        cursor.step(&rows, Direction::Down);
    }

    cursor.reset_to_top(&rows);

    assert_eq!(selected(&cursor), Some(1));
    assert_eq!(cursor.offset(), 0);
    cursor.re_anchor(&[Some(7), Some(1)]);
    assert_eq!(selected(&cursor), Some(7), "following the top again");
}
