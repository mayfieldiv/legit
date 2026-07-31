//! Canonical geometry of the list view: which rows and columns the Open PR
//! List occupies next to its chrome (app header, tab bar, filter chip, table
//! header, status bar) and the summary panel. The single source of truth shared
//! by `view::view` (which splits the frame into exactly these regions),
//! `Model::chrome_rows` / `sync_viewport` (which size the list viewport), and
//! `update`'s mouse hit-testing (which maps a click back to a visible row) — so
//! rendering and hit-testing can't disagree. Mirrors the TS `AppShell`, which
//! computes the same widths in one place. The detail view's analogue is
//! `detail_layout`.

use super::model::Model;

/// Below this terminal width the summary panel is hidden entirely — the list
/// takes the whole row.
const MIN_WIDTH_FOR_PANEL: u16 = 80;
/// At this terminal width and above the panel widens from 40 to 60 columns.
const WIDE_WIDTH: u16 = 140;
/// Panel width in the narrow band (80-139 columns).
const NARROW_PANEL_WIDTH: u16 = 40;
/// Panel width at >=140 columns.
const WIDE_PANEL_WIDTH: u16 = 60;

/// Width of the `│` rule between the list and the summary panel.
pub const DIVIDER_WIDTH: u16 = 1;

/// Rows of the status bar pinned to the bottom of every list-view frame.
const STATUS_ROWS: u16 = 1;
/// The top `legit — scope — N open PRs` line.
const APP_HEADER_ROWS: u16 = 1;
/// The Repo Tab row.
const TAB_ROWS: u16 = 1;
/// The column header row rendered above visible PR rows.
const TABLE_HEADER_ROWS: u16 = 1;

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

/// Rows of chrome above the selectable list rows: app header, tab bar, the
/// filter chip while it is visible, plus the table column header. The list's
/// first selectable visible row renders at exactly this row.
pub fn rows_above_list(filter_visible: bool) -> u16 {
    APP_HEADER_ROWS + TAB_ROWS + u16::from(filter_visible) + TABLE_HEADER_ROWS
}

/// Total chrome rows around the selectable list rows (above plus the status
/// bar) — what `sync_viewport` subtracts from the terminal height to size the
/// viewport.
pub fn chrome_rows(filter_visible: bool) -> usize {
    usize::from(rows_above_list(filter_visible) + STATUS_ROWS)
}

/// Visible rows in the summary panel. Unlike the Open PR List viewport this
/// includes the row parallel to the list's table header: the summary starts
/// immediately below the app header / tabs / optional filter chip and ends
/// above the status bar.
pub fn summary_viewport_rows(model: &Model) -> usize {
    let rows_outside_panel =
        APP_HEADER_ROWS + TAB_ROWS + u16::from(model.list.filter().is_visible()) + STATUS_ROWS;
    usize::from(model.terminal_height.saturating_sub(rows_outside_panel))
}

/// Whether a terminal cell lies inside the visible summary-panel viewport.
/// Uses the same width and chrome constants as the renderer and list-row
/// hit-testing, so wheel routing cannot drift from the painted panel.
pub fn summary_contains(model: &Model, column: u16, row: u16) -> bool {
    let Some(panel_width) = panel_width(model.terminal_width) else {
        return false;
    };
    let panel_left = model.terminal_width.saturating_sub(panel_width);
    let panel_top = APP_HEADER_ROWS + TAB_ROWS + u16::from(model.list.filter().is_visible());
    let status_row = model.terminal_height.saturating_sub(STATUS_ROWS);

    column >= panel_left && column < model.terminal_width && row >= panel_top && row < status_row
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
    fn panel_is_40_in_the_narrow_band() {
        assert_eq!(panel_width(80), Some(40));
        assert_eq!(panel_width(139), Some(40));
    }

    #[test]
    fn panel_is_60_at_wide_widths() {
        assert_eq!(panel_width(140), Some(60));
        assert_eq!(panel_width(200), Some(60));
    }

    #[test]
    fn list_takes_whatever_the_panel_and_divider_leave() {
        assert_eq!(list_width(79), 79, "no panel below 80 columns");
        assert_eq!(list_width(116), 116 - 40 - 1);
        assert_eq!(list_width(140), 140 - 60 - 1);
    }

    #[test]
    fn filter_chip_adds_a_chrome_row() {
        assert_eq!(rows_above_list(false), 3);
        assert_eq!(rows_above_list(true), 4);
        assert_eq!(chrome_rows(false), 4);
        assert_eq!(chrome_rows(true), 5);
    }
}
