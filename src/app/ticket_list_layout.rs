//! Canonical geometry of the ticket surface: the rows its chrome takes around
//! the queue and how the main region splits into the effort rail and the
//! queue. The single source of truth shared by `view::ticket_list` (which lays
//! the frame out) and `Model::sync_viewport` (which sizes the queue viewport),
//! so rendering and scroll math can't disagree — the `list_layout` analogue.

/// The top `legit — Tickets` line.
const APP_HEADER_ROWS: usize = 1;
/// The queue's column header row.
const TABLE_HEADER_ROWS: usize = 1;
/// The status bar pinned to the bottom.
const STATUS_ROWS: usize = 1;

/// Columns the effort rail takes, from the prototype's winning layout.
pub const RAIL_WIDTH: u16 = 38;
/// Width of the `│` rule between the rail and the queue.
pub const DIVIDER_WIDTH: u16 = 1;

/// Total chrome rows around the selectable queue rows — what `sync_viewport`
/// subtracts from the terminal height.
pub fn chrome_rows() -> usize {
    APP_HEADER_ROWS + TABLE_HEADER_ROWS + STATUS_ROWS
}
