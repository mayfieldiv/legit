use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use super::{Column, Columns, FillReserves, Table};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Col {
    A,
    B,
    C,
    D,
    Fill,
}

fn text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

/// The IDs `row` hands to its callback, in order.
fn callback_order(table: &Table<Col>) -> Vec<Col> {
    let mut order = Vec::new();
    table.row(None, |id, _| {
        order.push(id);
        Vec::new()
    });
    order
}

/// A required one-column `A`, a Fill reserving 5, and one optional `B` of
/// width 3 at rank 0 — the smallest declaration with an admission decision.
fn one_optional(row_width: usize) -> Table<Col> {
    Columns::new()
        .column(Column::fixed(Col::A, "a", 1))
        .fill(Col::Fill, "fill", FillReserves::new(5, 5))
        .column(Column::fixed(Col::B, "b", 3).optional(0))
        .fit(row_width)
}

#[test]
fn an_optional_column_is_admitted_at_exact_equality_and_costs_one_gap() {
    // Required: A 1 + gap 1 = 2; reserve 5 → budget = width - 7; B costs 3 + 1.
    let admitted = one_optional(11);
    assert_eq!(admitted.width(Col::B), Some(3));
    assert_eq!(admitted.width(Col::Fill), Some(5), "fill keeps its reserve");

    let rejected = one_optional(10);
    assert_eq!(rejected.width(Col::B), None);
    assert_eq!(
        rejected.width(Col::Fill),
        Some(8),
        "an absent column leaves neither its width nor its gap behind"
    );
}

#[test]
fn a_failed_admission_spends_nothing_and_a_cheaper_later_column_still_fits() {
    let table = Columns::new()
        .column(Column::fixed(Col::A, "a", 1))
        .fill(Col::Fill, "fill", FillReserves::new(5, 5))
        .column(Column::fixed(Col::B, "b", 5).optional(0))
        .column(Column::fixed(Col::C, "c", 2).optional(1))
        .fit(11);

    // Budget 4: B costs 6 and fails; C costs 3 and fits, leaving one over the
    // reserve for the fill.
    assert_eq!(table.width(Col::B), None);
    assert_eq!(table.width(Col::C), Some(2));
    assert_eq!(table.width(Col::Fill), Some(6));
}

#[test]
fn admission_follows_rank_not_declaration_order() {
    let table = Columns::new()
        .column(Column::fixed(Col::A, "a", 1))
        .fill(Col::Fill, "fill", FillReserves::new(5, 5))
        .column(Column::fixed(Col::B, "b", 3).optional(1))
        .column(Column::fixed(Col::C, "c", 3).optional(0))
        .fit(11);

    assert_eq!(table.width(Col::C), Some(3), "rank 0 is tried first");
    assert_eq!(table.width(Col::B), None);
}

#[test]
fn equal_ranks_break_ties_in_declaration_order() {
    let table = Columns::new()
        .column(Column::fixed(Col::A, "a", 1))
        .fill(Col::Fill, "fill", FillReserves::new(5, 5))
        .column(Column::fixed(Col::B, "b", 3).optional(0))
        .column(Column::fixed(Col::C, "c", 3).optional(0))
        .fit(11);

    assert_eq!(table.width(Col::B), Some(3));
    assert_eq!(table.width(Col::C), None);
}

#[test]
fn admitted_columns_render_in_declaration_order_regardless_of_rank() {
    let table = Columns::new()
        .column(Column::fixed(Col::A, "a", 1))
        .fill(Col::Fill, "fill", FillReserves::new(5, 5))
        .column(Column::fixed(Col::B, "b", 3).optional(1))
        .column(Column::fixed(Col::C, "c", 3).optional(0))
        .fit(15);

    assert_eq!(
        callback_order(&table),
        vec![Col::A, Col::Fill, Col::B, Col::C],
        "C was admitted before B but is painted after it"
    );
}

#[test]
fn the_callback_runs_once_per_present_column_and_never_for_hidden_ones() {
    let table = one_optional(10);

    assert_eq!(callback_order(&table), vec![Col::A, Col::Fill]);
}

#[test]
fn a_disabled_column_if_entry_is_absent_without_a_gap() {
    let with = Columns::new()
        .column(Column::fixed(Col::A, "a", 1))
        .column_if(true, Column::fixed(Col::B, "b", 2))
        .fill(Col::Fill, "fill", FillReserves::new(1, 1))
        .fit(10);
    let without = Columns::new()
        .column(Column::fixed(Col::A, "a", 1))
        .column_if(false, Column::fixed(Col::B, "b", 2))
        .fill(Col::Fill, "fill", FillReserves::new(1, 1))
        .fit(10);

    assert_eq!(with.width(Col::B), Some(2));
    assert_eq!(with.width(Col::Fill), Some(5));
    assert_eq!(without.width(Col::B), None);
    assert_eq!(without.width(Col::Fill), Some(8));
    assert_eq!(callback_order(&without), vec![Col::A, Col::Fill]);
}

fn header_and_row(table: &Table<Col>) -> (String, String) {
    let header = text(&table.header(Style::default()));
    let row = text(&table.row(None, |id, _| {
        let content = match id {
            Col::A => "aa",
            Col::B => "bb",
            Col::C => "cc",
            Col::D => "dd",
            Col::Fill => "ff",
        };
        vec![Span::raw(content)]
    }));
    (header, row)
}

#[test]
fn the_fill_can_lead_the_row() {
    let table = Columns::new()
        .fill(Col::Fill, "FILL", FillReserves::new(1, 1))
        .column(Column::fixed(Col::A, "A", 2))
        .column(Column::fixed(Col::B, "B", 2))
        .fit(12);

    assert_eq!(callback_order(&table), vec![Col::Fill, Col::A, Col::B]);
    let (header, row) = header_and_row(&table);
    assert_eq!(header, "FILL   A  B ");
    assert_eq!(row, "ff     aa bb");
}

#[test]
fn the_fill_can_sit_between_columns() {
    let table = Columns::new()
        .column(Column::fixed(Col::A, "A", 2))
        .fill(Col::Fill, "FILL", FillReserves::new(1, 1))
        .column(Column::fixed(Col::B, "B", 2))
        .fit(12);

    let (header, row) = header_and_row(&table);
    assert_eq!(header, "A  FILL   B ");
    assert_eq!(row, "aa ff     bb");
    assert_eq!(
        header.find('F'),
        row.find('f'),
        "header and data start together"
    );
    assert_eq!(header.find('B'), row.find('b'));
}

#[test]
fn the_fill_can_trail_the_row() {
    let table = Columns::new()
        .column(Column::fixed(Col::A, "A", 2))
        .column(Column::fixed(Col::B, "B", 2))
        .fill(Col::Fill, "FILL", FillReserves::new(1, 1))
        .fit(12);

    assert_eq!(callback_order(&table), vec![Col::A, Col::B, Col::Fill]);
    let (header, row) = header_and_row(&table);
    assert_eq!(header, "A  B  FILL  ");
    assert_eq!(row, "aa bb ff    ");
}

#[test]
#[should_panic(expected = "column ID twice")]
fn duplicate_required_ids_are_rejected() {
    Columns::new()
        .column(Column::fixed(Col::A, "a", 1))
        .column(Column::fixed(Col::A, "again", 1))
        .fill(Col::Fill, "fill", FillReserves::new(1, 1))
        .fit(20);
}

#[test]
#[should_panic(expected = "column ID twice")]
fn an_unadmitted_optional_still_counts_toward_uniqueness() {
    Columns::new()
        .column(Column::fixed(Col::A, "a", 1))
        .fill(Col::Fill, "fill", FillReserves::new(1, 1))
        .column(Column::fixed(Col::A, "again", 40).optional(0))
        .fit(0);
}

#[test]
#[should_panic(expected = "column ID twice")]
fn a_column_sharing_the_fill_id_is_rejected() {
    Columns::new()
        .column(Column::fixed(Col::Fill, "a", 1))
        .fill(Col::Fill, "fill", FillReserves::new(1, 1))
        .fit(20);
}

#[test]
#[should_panic(expected = "positive width")]
fn a_zero_fixed_width_is_rejected() {
    Column::fixed(Col::A, "a", 0);
}

#[test]
#[should_panic(expected = "at least one column")]
fn a_zero_admission_reserve_is_rejected() {
    FillReserves::new(0, 5);
}

#[test]
#[should_panic(expected = "must not be below")]
fn a_growth_reserve_below_the_admission_reserve_is_rejected() {
    FillReserves::new(6, 5);
}

#[test]
fn row_widths_zero_and_one_fit_with_the_fill_at_one_column() {
    for width in [0, 1] {
        let table = one_optional(width);
        assert_eq!(table.width(Col::A), Some(1), "row width {width}");
        assert_eq!(table.width(Col::Fill), Some(1), "row width {width}");
        assert_eq!(table.width(Col::B), None, "row width {width}");
    }
}

#[test]
fn required_overflow_keeps_the_fill_at_one_column_and_the_line_wider_than_the_row() {
    let table = Columns::new()
        .column(Column::fixed(Col::A, "a", 10))
        .column(Column::fixed(Col::B, "b", 10))
        .fill(Col::Fill, "fill", FillReserves::new(1, 1))
        .fit(5);

    assert_eq!(table.width(Col::Fill), Some(1));
    assert_eq!(
        table.header(Style::default()).width(),
        10 + 1 + 10 + 1 + 1,
        "required columns, their gaps, and a one-column fill; the widget clips"
    );
}

#[test]
fn multispan_cells_truncate_the_straddling_span_and_keep_each_style() {
    let red = Style::default().fg(Color::Red);
    let blue = Style::default().fg(Color::Blue);
    let table = Columns::new()
        .column(Column::fixed(Col::A, "a", 4))
        .fill(Col::Fill, "fill", FillReserves::new(1, 1))
        .fit(8);

    let line = table.row(None, |id, _| match id {
        Col::A => vec![Span::styled("ab", red), Span::styled("CDEF", blue)],
        _ => vec![Span::raw("x")],
    });

    assert_eq!(text(&line), "abC… x  ");
    assert_eq!(line.spans[0].style, red);
    assert_eq!(line.spans[1].content, "C…");
    assert_eq!(
        line.spans[1].style, blue,
        "the ellipsis inherits the shortened span's style"
    );
}

#[test]
fn spans_past_the_cell_budget_are_dropped_without_an_ellipsis() {
    let table = Columns::new()
        .column(Column::fixed(Col::A, "a", 3))
        .fill(Col::Fill, "fill", FillReserves::new(1, 1))
        .fit(5);

    let line = table.row(None, |id, _| match id {
        Col::A => vec![Span::raw("abc"), Span::raw("DEF")],
        _ => Vec::new(),
    });

    assert_eq!(text(&line), "abc  ");
}

#[test]
fn short_cells_are_padded_with_unstyled_spaces() {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let table = Columns::new()
        .column(Column::fixed(Col::A, "a", 4))
        .fill(Col::Fill, "fill", FillReserves::new(1, 1))
        .fit(8);

    let line = table.row(None, |id, _| match id {
        Col::A => vec![Span::styled("ab", bold)],
        _ => vec![Span::raw("f")],
    });

    assert_eq!(text(&line), "ab   f  ");
    assert_eq!(line.spans[1].content, "  ", "cell padding");
    assert_eq!(line.spans[1].style, Style::default());
    assert_eq!(line.spans[2].content, " ", "the inter-cell gap");
}

#[test]
fn a_selected_fill_paints_the_line_background_and_leaves_span_foregrounds() {
    let green = Style::default().fg(Color::Green);
    let table = Columns::new()
        .column(Column::fixed(Col::A, "a", 2))
        .fill(Col::Fill, "fill", FillReserves::new(1, 1))
        .fit(6);

    let plain = table.row(None, |_, _| vec![Span::styled("x", green)]);
    let selected = table.row(Some(Color::Cyan), |_, _| vec![Span::styled("x", green)]);

    assert_eq!(plain.style, Style::default());
    assert_eq!(selected.style, Style::default().bg(Color::Cyan));
    assert!(selected.spans.iter().all(|span| span.style.bg.is_none()));
    assert_eq!(selected.spans[0].style, green);
}

#[test]
fn wide_glyphs_truncate_by_display_width_and_pad_the_leftover_column() {
    let table = Columns::new()
        .column(Column::fixed(Col::A, "a", 3))
        .column(Column::fixed(Col::B, "b", 3))
        .fill(Col::Fill, "fill", FillReserves::new(1, 1))
        .fit(9);

    let line = table.row(None, |id, _| match id {
        Col::A => vec![Span::raw("一二")],
        Col::B => vec![Span::raw("一")],
        _ => vec![Span::raw("f")],
    });

    // A: 一 (2) + … (1) = 3. B: 一 (2) + one pad column. Fill: f.
    assert_eq!(text(&line), "一… 一  f");
    assert_eq!(line.width(), 9);
}

#[test]
fn the_header_labels_every_present_column_in_the_given_style() {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let table = Columns::new()
        .column(Column::fixed(Col::A, "Alpha", 6))
        .fill(Col::Fill, "Title", FillReserves::new(8, 8))
        .column(Column::fixed(Col::D, "Delta", 5).optional(0))
        .fit(14);

    let header = table.header(bold);

    assert_eq!(text(&header), "Alpha  Title  ");
    assert!(
        header
            .spans
            .iter()
            .filter(|span| !span.content.trim().is_empty())
            .all(|span| span.style == bold)
    );
    assert!(
        !text(&header).contains("Delta"),
        "an unadmitted column has no label"
    );
}

#[test]
fn width_reports_none_for_ids_absent_from_the_declaration() {
    let table = one_optional(20);

    assert_eq!(table.width(Col::D), None);
}
