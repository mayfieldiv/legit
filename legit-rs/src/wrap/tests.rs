use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use super::wrap_lines;

fn texts(lines: &[Line<'_>]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn wraps_at_word_boundaries_and_drops_the_break_space() {
    let lines = wrap_lines(vec![Line::from("aaa bbb ccc")], 7);
    assert_eq!(texts(&lines), vec!["aaa bbb", "ccc"]);
}

#[test]
fn short_lines_pass_through_untouched() {
    let lines = wrap_lines(vec![Line::from("short"), Line::from("")], 10);
    assert_eq!(texts(&lines), vec!["short", ""]);
}

#[test]
fn zero_width_passes_everything_through() {
    let lines = wrap_lines(vec![Line::from("anything at all goes here")], 0);
    assert_eq!(texts(&lines), vec!["anything at all goes here"]);
}

#[test]
fn preserves_span_styles_across_the_break() {
    let red = Style::default().fg(Color::Red);
    let blue = Style::default().fg(Color::Blue);
    let line = Line::from(vec![
        Span::styled("red text ", red),
        Span::styled("blue text", blue),
    ]);

    let lines = wrap_lines(vec![line], 9);

    assert_eq!(texts(&lines), vec!["red text", "blue text"]);
    assert!(
        lines[0].spans.iter().all(|s| s.style == red),
        "first row must keep the red style: {:?}",
        lines[0]
    );
    assert!(
        lines[1].spans.iter().all(|s| s.style == blue),
        "second row must keep the blue style: {:?}",
        lines[1]
    );
}

#[test]
fn hard_splits_words_wider_than_the_row() {
    let lines = wrap_lines(vec![Line::from("abcdefghij")], 4);
    assert_eq!(texts(&lines), vec!["abcd", "efgh", "ij"]);
}

#[test]
fn long_word_after_short_word_starts_its_own_row() {
    let lines = wrap_lines(vec![Line::from("ok abcdefgh")], 5);
    assert_eq!(texts(&lines), vec!["ok", "abcde", "fgh"]);
}

#[test]
fn leading_indent_stays_on_the_first_row() {
    let lines = wrap_lines(vec![Line::from("  • bullet text that wraps")], 15);
    assert_eq!(texts(&lines), vec!["  • bullet text", "that wraps"]);
}

#[test]
fn wide_characters_count_their_display_width() {
    // Each CJK char is 2 columns wide, so only two fit per 5-column row.
    let lines = wrap_lines(vec![Line::from("ねこねこねこ")], 5);
    assert_eq!(texts(&lines), vec!["ねこ", "ねこ", "ねこ"]);
}
