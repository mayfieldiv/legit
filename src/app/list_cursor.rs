//! The selection cursor and scroll viewport shared by every keyboard-navigated
//! list (the Open PR List, the ticket queue): what is selected, how that
//! relates to user intent, and the window that keeps it on-screen. Each list
//! keeps its own display rows and its own selection identity (a PR index, a
//! Ticket key) and hands the rows in per call; the cursor owns only the state
//! and the rules that are identical between them.

use std::ops::Range;

/// A display row: an item with a selection identity, or a header (`None`).
pub trait SelectableRow {
    type Id: Clone + Eq;
    fn id(&self) -> Option<&Self::Id>;
}

/// How the selection cursor relates to user intent — one value instead of
/// parallel booleans, so "viewport detached from a selection the user never
/// made" is unrepresentable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum SelectionMode {
    /// The user hasn't navigated yet (or a tab switch/regroup reset to the
    /// top): `re_anchor` keeps the selection on the top item. Rows stream in
    /// and re-sort, so the first-arrived row is rarely the top one — without
    /// this the startup cursor would park on an arbitrary mid-list row.
    #[default]
    FollowTop,
    /// The user picked an item (j/k, click): the selection sticks to it
    /// through re-sorts, and the viewport follows it.
    Pinned,
    /// Wheel scrolling moved the viewport away from the pinned selection;
    /// background re-anchors preserve the viewport instead of snapping back,
    /// until the user explicitly moves/selects again.
    Detached,
}

/// The selection cursor and scroll viewport of one list, over its display
/// rows (headers count toward the window like any other row). Every rebuild
/// of the rows must be followed by `re_anchor`; after it, the selection is
/// `None` exactly when no row is selectable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListCursor<Id> {
    selected: Option<Id>,
    mode: SelectionMode,
    viewport: Viewport,
}

/// Hand-written so the identity needn't be `Default`: an empty cursor selects
/// nothing.
impl<Id> Default for ListCursor<Id> {
    fn default() -> Self {
        Self {
            selected: None,
            mode: SelectionMode::default(),
            viewport: Viewport::default(),
        }
    }
}

impl<Id: Clone + Eq> ListCursor<Id> {
    pub fn selected(&self) -> Option<&Id> {
        self.selected.as_ref()
    }

    /// The first display row on screen.
    pub fn offset(&self) -> usize {
        self.viewport.offset
    }

    /// Rows on screen; zero before the first resize, which shows every row.
    pub fn height(&self) -> usize {
        self.viewport.height
    }

    /// The display rows inside the window, each flagged when it is the
    /// selected item. Headers are never flagged.
    pub fn visible_rows<'r, R: SelectableRow<Id = Id>>(
        &self,
        rows: &'r [R],
    ) -> impl Iterator<Item = (&'r R, bool)> {
        let selected = self.selected.as_ref();
        rows[self.viewport.window(rows.len())]
            .iter()
            .map(move |row| (row, row.id().is_some_and(|id| Some(id) == selected)))
    }

    /// Step the selection to the adjacent item in `direction`, skipping
    /// headers and clamping at the ends. Pins the cursor: the user has chosen.
    pub fn step<R: SelectableRow<Id = Id>>(&mut self, rows: &[R], direction: Direction) {
        self.mode = SelectionMode::Pinned;
        if let Some(current) = self.selected_row(rows)
            && let Some(next) = adjacent_item(rows, current, direction)
        {
            self.selected = Some(next.clone());
        }
        self.settle(rows);
    }

    /// Re-anchor after the rows were rebuilt: an unpinned cursor follows the
    /// top item; a pinned or detached one keeps its item while it is still in
    /// the rows and snaps to the top otherwise — a detached viewport aimed at
    /// a vanished item re-pins to the snapped-to row rather than staying aimed
    /// at nothing. Then the viewport settles.
    pub fn re_anchor<R: SelectableRow<Id = Id>>(&mut self, rows: &[R]) {
        let keep = self.mode != SelectionMode::FollowTop && self.selected_row(rows).is_some();
        if !keep {
            let top = first_item(rows).cloned();
            if self.selected != top {
                self.selected = top;
                if self.mode == SelectionMode::Detached {
                    self.mode = SelectionMode::Pinned;
                }
            }
        }
        self.settle(rows);
    }

    /// Back to the top item with the window at the top and the cursor
    /// unpinned — what a tab switch or regroup wants, so the selection
    /// follows the new top rather than chasing the previous item.
    pub fn reset_to_top<R: SelectableRow<Id = Id>>(&mut self, rows: &[R]) {
        self.selected = first_item(rows).cloned();
        self.mode = SelectionMode::FollowTop;
        self.viewport.offset = 0;
        self.settle(rows);
    }

    pub fn resize<R: SelectableRow<Id = Id>>(&mut self, rows: &[R], height: usize) {
        self.viewport.height = height;
        self.settle(rows);
    }

    /// Wheel input: move the window `by` rows toward the end without moving
    /// the selection, which may then sit off-screen. Detaches the viewport —
    /// wheel input is engagement, and leaving `FollowTop` would let a
    /// background re-anchor yank the still-default selection (and the window
    /// with it) back to a re-sorted top row mid-browse. A wheel event over an
    /// empty or unsized list is a no-op, not engagement: detaching then would
    /// pin the default selection to whichever item happens to arrive first.
    pub fn scroll_down<R: SelectableRow<Id = Id>>(&mut self, rows: &[R], by: usize) {
        if self.viewport.height == 0 || rows.is_empty() {
            return;
        }
        self.viewport.offset = self.viewport.offset.saturating_add(by);
        self.viewport.clamp(rows.len());
        self.mode = SelectionMode::Detached;
    }

    /// Wheel input toward the start; see `scroll_down`.
    pub fn scroll_up<R: SelectableRow<Id = Id>>(&mut self, rows: &[R], by: usize) {
        if self.viewport.height == 0 || rows.is_empty() {
            return;
        }
        self.viewport.offset = self.viewport.offset.saturating_sub(by);
        self.mode = SelectionMode::Detached;
    }

    /// Select the item at `visible_row` within the window; `false` for a
    /// header or a row past the end. Unlike keyboard movement this leaves the
    /// window alone: the clicked row is already on screen.
    pub fn select_visible_row<R: SelectableRow<Id = Id>>(
        &mut self,
        rows: &[R],
        visible_row: usize,
    ) -> bool {
        let Some(id) = rows
            .get(self.viewport.offset.saturating_add(visible_row))
            .and_then(SelectableRow::id)
        else {
            return false;
        };
        self.selected = Some(id.clone());
        self.mode = SelectionMode::Pinned;
        true
    }

    fn selected_row<R: SelectableRow<Id = Id>>(&self, rows: &[R]) -> Option<usize> {
        let selected = self.selected.as_ref()?;
        rows.iter().position(|row| row.id() == Some(selected))
    }

    /// Restore the window's invariant: following the selection, or — with the
    /// viewport detached or nothing selected — pulled back to the last
    /// screenful.
    fn settle<R: SelectableRow<Id = Id>>(&mut self, rows: &[R]) {
        match self.selected_row(rows) {
            Some(row) if self.mode != SelectionMode::Detached => {
                self.viewport.follow(row, rows.len());
            }
            _ => self.viewport.clamp(rows.len()),
        }
    }
}

fn first_item<R: SelectableRow>(rows: &[R]) -> Option<&R::Id> {
    rows.iter().find_map(SelectableRow::id)
}

/// The scroll window over a list's display rows: the first visible row and
/// how many rows are on screen. A zero height (before the first resize)
/// shows every row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Viewport {
    offset: usize,
    height: usize,
}

impl Viewport {
    /// The display-row range on screen, given `rows` rows in total.
    fn window(&self, rows: usize) -> Range<usize> {
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
    fn follow(&mut self, selected_row: usize, rows: usize) {
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
    fn clamp(&mut self, rows: usize) {
        self.offset = self.offset.min(rows.saturating_sub(self.height));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

/// The nearest item from `current` in `direction`, skipping headers; `None`
/// when no item lies that way (already at the first/last item).
fn adjacent_item<R: SelectableRow>(
    rows: &[R],
    current: usize,
    direction: Direction,
) -> Option<&R::Id> {
    match direction {
        Direction::Down => rows[current + 1..].iter().find_map(SelectableRow::id),
        Direction::Up => rows[..current].iter().rev().find_map(SelectableRow::id),
    }
}

#[cfg(test)]
mod tests;
