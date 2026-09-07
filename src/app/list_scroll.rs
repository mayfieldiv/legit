//! Cursor-following scroll math shared by every keyboard-navigated list (the
//! Open PR List, the ticket queue). Pure functions over display-row indices,
//! so each list keeps its own cursor semantics and only borrows the window
//! arithmetic.

/// The scroll offset that keeps `selected_row` on-screen with a ~10% margin
/// above and below, starting from the current `offset`. Margin =
/// `viewport_height / 10`, floor 1, so the selection never parks on the very
/// top/bottom row when more rows are available in that direction — capped at
/// half the rows on each side, because in a tiny viewport the two margins
/// would otherwise overlap and become jointly unsatisfiable (at height 1 a
/// floor-1 margin demands a row above AND below the only visible line). The
/// result is also clamped so it never sits past the last screenful.
pub fn follow_selection(
    offset: usize,
    selected_row: usize,
    rows: usize,
    viewport_height: usize,
) -> usize {
    if viewport_height == 0 || rows == 0 {
        return offset;
    }
    let margin = (viewport_height / 10)
        .max(1)
        .min(viewport_height.saturating_sub(1) / 2);
    // The bottom constraint is a lower bound on the offset, the top constraint
    // an upper bound; with the capped margin they can't conflict, so one pass
    // settles both.
    let min_offset = (selected_row + margin + 1).saturating_sub(viewport_height);
    let max_for_top = selected_row.saturating_sub(margin);
    let followed = if offset < min_offset {
        min_offset
    } else if offset > max_for_top {
        max_for_top
    } else {
        offset
    };
    clamp(followed, rows, viewport_height)
}

/// `offset` clamped so the window never extends past the last row.
pub fn clamp(offset: usize, rows: usize, viewport_height: usize) -> usize {
    offset.min(rows.saturating_sub(viewport_height))
}
