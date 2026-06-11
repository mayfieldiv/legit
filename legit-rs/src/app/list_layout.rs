//! Canonical geometry of the list view: which rows and columns the Open PR
//! List occupies next to its chrome (tab bar, filter chip, status bar) and
//! the summary panel. The single source of truth shared by `view::view`
//! (which splits the frame into exactly these regions), `Model::chrome_rows`
//! / `sync_viewport` (which size the list viewport), and `update`'s mouse
//! hit-testing (which maps a click back to a visible row) — so rendering and
//! hit-testing can't disagree. Mirrors the TS `AppShell`, which computes the
//! same widths in one place. The detail view's analogue is `detail_layout`.

use super::model::Model;

/// Below this terminal width the summary panel is hidden entirely — the list
/// takes the whole row.
const MIN_WIDTH_FOR_PANEL: u16 = 80;
/// At this terminal width and above the panel widens from 36 to 50 columns.
const WIDE_WIDTH: u16 = 140;
/// Panel width in the narrow band (80-139 columns).
const NARROW_PANEL_WIDTH: u16 = 36;
/// Panel width at >=140 columns.
const WIDE_PANEL_WIDTH: u16 = 50;

/// Width of the `│` rule between the list and the summary panel.
pub const DIVIDER_WIDTH: u16 = 1;

/// Rows of the status bar pinned to the bottom of every list-view frame.
const STATUS_ROWS: u16 = 1;

/// The summary panel's width for a given terminal width, or `None` when the
/// terminal is too narrow to show it (the list then takes the whole row).
pub fn panel_width(total_cols: u16) -> Option<u16> {
    if total_cols < MIN_WIDTH_FOR_PANEL {
        None
    } else if total_cols < WIDE_WIDTH {
        Some(NARROW_PANEL_WIDTH)
    } else {
        Some(WIDE_PANEL_WIDTH)
    }
}

/// The list's width for a given terminal width: whatever the summary panel
/// and its divider don't take.
pub fn list_width(total_cols: u16) -> u16 {
    panel_width(total_cols).map_or(total_cols, |panel| {
        total_cols.saturating_sub(panel + DIVIDER_WIDTH)
    })
}

/// Rows of chrome above the list: the tab bar, plus the filter chip while it
/// is visible. The list's first visible row renders at exactly this row.
pub fn rows_above_list(filter_visible: bool) -> u16 {
    1 + u16::from(filter_visible)
}

/// Total chrome rows around the list (above plus the status bar) — what
/// `sync_viewport` subtracts from the terminal height to size the viewport.
pub fn chrome_rows(filter_visible: bool) -> usize {
    usize::from(rows_above_list(filter_visible) + STATUS_ROWS)
}

/// The list visible-row index under a click at (`column`, `row`), or `None`
/// when the click lands outside the list region — on the chrome rows, the
/// divider, or the summary panel.
pub fn visible_row_at(model: &Model, column: u16, row: u16) -> Option<usize> {
    let top = rows_above_list(model.list.filter().is_visible());
    let status_row = model.terminal_height.saturating_sub(STATUS_ROWS);
    if row < top || row >= status_row || column >= list_width(model.terminal_width) {
        return None;
    }
    Some(usize::from(row - top))
}

#[cfg(test)]
mod tests {
    use super::{chrome_rows, list_width, panel_width, rows_above_list};

    #[test]
    fn panel_hidden_below_80_columns() {
        assert_eq!(panel_width(79), None);
        assert_eq!(panel_width(0), None);
    }

    #[test]
    fn panel_is_36_in_the_narrow_band() {
        assert_eq!(panel_width(80), Some(36));
        assert_eq!(panel_width(139), Some(36));
    }

    #[test]
    fn panel_is_50_at_wide_widths() {
        assert_eq!(panel_width(140), Some(50));
        assert_eq!(panel_width(200), Some(50));
    }

    #[test]
    fn list_takes_whatever_the_panel_and_divider_leave() {
        assert_eq!(list_width(79), 79, "no panel below 80 columns");
        assert_eq!(list_width(116), 116 - 36 - 1);
        assert_eq!(list_width(140), 140 - 50 - 1);
    }

    #[test]
    fn filter_chip_adds_a_chrome_row() {
        assert_eq!(rows_above_list(false), 1);
        assert_eq!(rows_above_list(true), 2);
        assert_eq!(chrome_rows(false), 2);
        assert_eq!(chrome_rows(true), 3);
    }
}
