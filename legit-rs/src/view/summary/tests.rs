use chrono::{DateTime, TimeZone, Utc};
use ratatui::{Terminal, backend::TestBackend};

use super::panel_width;
use crate::{app::model::Model, view};

fn fixed_now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 20, 12, 0, 0).unwrap()
}

/// Render the whole frame at `width`x`height` and return the panel's columns
/// (everything right of the list/summary split), excluding the tab bar and
/// status bar rows. The panel width matches `panel_width(width)`.
fn panel_rows(model: &Model, width: u16, height: u16) -> Vec<String> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| view::view(model, frame, fixed_now()))
        .expect("draw");
    let buf = terminal.backend().buffer().clone();
    let panel_w = panel_width(width).expect("panel should be visible at this width");
    let split_x = width - panel_w;
    (1..height - 1)
        .map(|y| {
            (split_x..width)
                .map(|x| buf[(x, y)].symbol().to_owned())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn no_pr_selected_renders_placeholder() {
    let (model, _) = Model::new();

    let rows = panel_rows(&model, 80, 6);

    assert!(
        rows[0].trim_start().starts_with("No PR selected"),
        "{rows:?}"
    );
}

#[test]
fn panel_is_hidden_below_eighty_columns() {
    assert_eq!(panel_width(79), None);
    assert_eq!(panel_width(0), None);
}

#[test]
fn panel_is_thirty_six_columns_in_the_narrow_band() {
    assert_eq!(panel_width(80), Some(36));
    assert_eq!(panel_width(139), Some(36));
}

#[test]
fn panel_is_fifty_columns_at_one_forty_and_above() {
    assert_eq!(panel_width(140), Some(50));
    assert_eq!(panel_width(200), Some(50));
}
