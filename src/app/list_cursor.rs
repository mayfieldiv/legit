//! The cursor and viewport machinery shared by every keyboard-navigated list
//! (the Open PR List, the ticket queue): how the selection relates to user
//! intent, the scroll window that keeps it on-screen, and stepping between
//! selectable display rows past headers. Each list keeps its own selection
//! identity (a PR index, a Ticket key) and its own row layout; this module
//! owns only what is identical between them, over display-row indices.

use std::ops::Range;

/// How the selection cursor relates to user intent — one value instead of
/// parallel booleans, so "viewport detached from a selection the user never
/// made" is unrepresentable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SelectionMode {
    /// The user hasn't navigated yet (or a tab switch/regroup reset to the
    /// top): `relayout` keeps the selection on the top display row. Rows
    /// stream in and re-sort, so the first-arrived row is rarely the top one
    /// — without this the startup cursor would park on an arbitrary mid-list
    /// row.
    #[default]
    FollowTop,
    /// The user picked a row (j/k, click): the selection sticks to that item
    /// through re-sorts, and the viewport follows it.
    Pinned,
    /// Wheel scrolling moved the viewport away from the pinned selection;
    /// background relayouts preserve the viewport instead of snapping back,
    /// until the user explicitly moves/selects again.
    Detached,
}

/// The scroll window over a list's display rows: the first visible row and
/// how many rows are on screen. Headers count toward both like any other
/// row. A zero height (before the first resize) shows every row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Viewport {
    offset: usize,
    height: usize,
}

impl Viewport {
    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// Set the row count on screen. The caller re-anchors afterwards with
    /// `follow` or `clamp`, whichever its selection mode wants.
    pub fn resize(&mut self, height: usize) {
        self.height = height;
    }

    pub fn scroll_to_top(&mut self) {
        self.offset = 0;
    }

    /// The display-row range on screen, given `rows` rows in total.
    pub fn window(&self, rows: usize) -> Range<usize> {
        let start = self.offset.min(rows);
        let end = if self.height == 0 {
            rows
        } else {
            (start + self.height).min(rows)
        };
        start..end
    }

    /// Move the window so `selected_row` stays on-screen with a ~10% margin
    /// above and below. Margin = `height / 10`, floor 1, so the selection
    /// never parks on the very top/bottom row when more rows are available in
    /// that direction — capped at half the rows on each side, because in a
    /// tiny viewport the two margins would otherwise overlap and become
    /// jointly unsatisfiable (at height 1 a floor-1 margin demands a row
    /// above AND below the only visible line). Never sits past the last
    /// screenful.
    pub fn follow(&mut self, selected_row: usize, rows: usize) {
        if self.height == 0 || rows == 0 {
            return;
        }
        let margin = (self.height / 10)
            .max(1)
            .min(self.height.saturating_sub(1) / 2);
        // The bottom constraint is a lower bound on the offset, the top
        // constraint an upper bound; the capped margin keeps the lower bound
        // at or below the upper one, so one clamp settles both.
        let min_offset = (selected_row + margin + 1).saturating_sub(self.height);
        let max_for_top = selected_row.saturating_sub(margin);
        self.offset = self.offset.clamp(min_offset, max_for_top);
        self.clamp(rows);
    }

    /// Pull the window back so it never extends past the last row.
    pub fn clamp(&mut self, rows: usize) {
        self.offset = self.offset.min(rows.saturating_sub(self.height));
    }

    /// Move the window `by` rows toward the end, stopping at the last
    /// screenful. Wheel input: the selection is untouched.
    pub fn scroll_down(&mut self, by: usize, rows: usize) {
        self.offset = self.offset.saturating_add(by);
        self.clamp(rows);
    }

    /// Move the window `by` rows toward the start.
    pub fn scroll_up(&mut self, by: usize) {
        self.offset = self.offset.saturating_sub(by);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

/// The nearest selectable row from `current` in `direction`: `item` maps a
/// display row to its selection identity, or `None` for a header (skipped).
/// `None` when no selectable row lies in that direction (already at the
/// first/last item).
pub fn adjacent_item<R, T>(
    rows: &[R],
    current: usize,
    direction: Direction,
    item: impl Fn(&R) -> Option<T>,
) -> Option<T> {
    match direction {
        Direction::Down => rows[current + 1..].iter().find_map(item),
        Direction::Up => rows[..current].iter().rev().find_map(item),
    }
}

#[cfg(test)]
mod tests;
