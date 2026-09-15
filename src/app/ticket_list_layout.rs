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

/// Width of the `│` rule between the rail and the queue.
pub const DIVIDER_WIDTH: u16 = 1;
/// Below this many queue columns the rail is dropped so the queue keeps a
/// usable width — the floor only.
// TODO(#133): the narrow-width collapse proper (spec §6.4).
const MIN_QUEUE_WIDTH: u16 = 40;

/// The rail's width for the main region's width, or `None` when the rail and
/// its divider would leave the queue under the floor (the queue then takes
/// the whole region) — the `panel_width` analogue.
pub fn rail_width(main_width: u16) -> Option<u16> {
    let rail = (main_width / 4).clamp(38, 72);
    (main_width >= rail + DIVIDER_WIDTH + MIN_QUEUE_WIDTH).then_some(rail)
}

/// Total chrome rows around the selectable queue rows — what `sync_viewport`
/// subtracts from the terminal height.
pub fn chrome_rows() -> usize {
    APP_HEADER_ROWS + TABLE_HEADER_ROWS + STATUS_ROWS
}

#[cfg(test)]
mod tests {
    use super::rail_width;

    #[test]
    fn the_rail_is_dropped_when_it_would_leave_the_queue_under_the_floor() {
        assert_eq!(rail_width(78), None, "38 + 1 + 40 columns is the minimum");
        assert_eq!(rail_width(79), Some(38));
        assert_eq!(rail_width(100), Some(38));
    }

    #[test]
    fn the_rail_takes_a_quarter_of_the_width_between_38_and_72() {
        assert_eq!(rail_width(200), Some(50));
        assert_eq!(rail_width(320), Some(72));
    }
}
