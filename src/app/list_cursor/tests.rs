use super::{Direction, Viewport, adjacent_item};

/// A display layout of headers (`None`) and items (`Some(id)`).
const ROWS: [Option<u8>; 6] = [None, Some(1), Some(2), None, Some(3), None];

#[test]
fn an_unsized_viewport_shows_every_row() {
    let viewport = Viewport::default();
    assert_eq!(viewport.window(6), 0..6);
    assert_eq!(viewport.window(0), 0..0);
}

#[test]
fn the_window_never_extends_past_the_last_row() {
    let mut viewport = Viewport::default();
    viewport.resize(4);
    viewport.scroll_down(100, 10);
    assert_eq!(viewport.window(10), 6..10, "clamped to the last screenful");

    viewport.scroll_up(2);
    assert_eq!(viewport.window(10), 4..8);

    viewport.clamp(5);
    assert_eq!(
        viewport.window(5),
        1..5,
        "fewer rows: the window pulls back"
    );
}

#[test]
fn follow_keeps_the_selected_row_inside_the_window_with_a_margin() {
    let mut viewport = Viewport::default();
    viewport.resize(10);

    viewport.follow(9, 20);
    assert!(
        viewport.window(20).contains(&9) && viewport.offset() >= 1,
        "moving into the bottom margin advances the window: {viewport:?}"
    );

    viewport.follow(0, 20);
    assert_eq!(viewport.offset(), 0);
}

#[test]
fn a_single_row_viewport_shows_exactly_the_selected_row() {
    let mut viewport = Viewport::default();
    viewport.resize(1);
    for row in 0..5 {
        viewport.follow(row, 10);
        assert_eq!(viewport.window(10), row..row + 1);
    }
}

#[test]
fn adjacent_items_skip_headers_and_stop_at_the_ends() {
    let item = |row: &Option<u8>| *row;
    assert_eq!(adjacent_item(&ROWS, 1, Direction::Down, item), Some(2));
    assert_eq!(
        adjacent_item(&ROWS, 2, Direction::Down, item),
        Some(3),
        "past the header"
    );
    assert_eq!(adjacent_item(&ROWS, 4, Direction::Down, item), None);
    assert_eq!(adjacent_item(&ROWS, 4, Direction::Up, item), Some(2));
    assert_eq!(adjacent_item(&ROWS, 1, Direction::Up, item), None);
    assert_eq!(
        adjacent_item(&ROWS, 5, Direction::Down, item),
        None,
        "from the last row"
    );
}
