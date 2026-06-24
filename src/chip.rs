//! Label Chips: the pure layout and colour resolution behind rendering a PR's
//! labels as filled colour badges in the summary panel and detail header.
//!
//! A chip is self-contained — it carries its own background (the label's GitHub
//! colour, or a stable hashed fallback when GitHub left it blank) and a
//! contrast-flipped foreground (`color::contrast_text`), so chips need no
//! palette role of their own (ADR 0005). This module owns the colour resolution
//! (`label_color`), the width-wrapping packer (`chip_rows`, ported from GHUI's
//! `labelChipRows`), and the render helper that turns one packed row into styled
//! spans (`chip_spans`). Chips are purely cosmetic: they drive no sort, filter,
//! or Smart-status.

use ratatui::{
    style::{Color, Style},
    text::Span,
};
use unicode_width::UnicodeWidthStr;

use crate::color::{contrast_text, hash_hue, parse_hex};
use crate::github::rest::Label;
use crate::palette::Palette;

/// The space a chip occupies on a row: the label name's display width plus one
/// padding column on each side. Measures terminal display columns (not Unicode
/// scalar count) so wide glyphs — emoji, CJK — reserve the room ratatui actually
/// paints, keeping `chip_rows` (and the header band it sizes) in step with the
/// rendered chips. Mirrors GHUI's `label.name.length + 2`.
fn chip_width(label: &Label) -> usize {
    label.name.width() + 2
}

/// The colour a Label Chip paints as its background: the label's GitHub colour
/// when present and parseable, else a stable hashed fallback derived from its
/// name so a colourless label is still distinct and consistent across sessions.
/// The analogue of GHUI's `labelColor`.
pub fn label_color(label: &Label) -> Color {
    label
        .color
        .as_deref()
        .and_then(parse_hex)
        .unwrap_or_else(|| hash_hue(&label.name))
}

/// Pack `labels` into rows that each fit `width` columns, keeping label order.
///
/// A row breaks to a new one when the next chip — counting its padding and the
/// one-column gap from the previous chip — would overflow `width`; a chip is
/// never dropped, so at a very narrow width each chip simply takes its own row.
/// Ported from GHUI's `labelChipRows`.
///
/// Tolerates `width` 0 — callers need not clamp it. `width` is only read in the
/// overflow comparison, which fires only once a row is non-empty, by which
/// point the next chip already overflows any width below the smallest chip
/// (`>= 5` columns); so widths 0 and 1 partition identically and neither panics.
pub fn chip_rows(labels: &[Label], width: usize) -> Vec<Vec<&Label>> {
    let mut rows: Vec<Vec<&Label>> = Vec::new();
    let mut current: Vec<&Label> = Vec::new();
    let mut current_width = 0usize;

    for label in labels {
        let label_width = chip_width(label);
        // The gap only counts once a chip already sits on the current row.
        let gap = usize::from(!current.is_empty());
        let next_width = current_width + label_width + gap;
        if !current.is_empty() && next_width > width {
            rows.push(std::mem::take(&mut current));
            current.push(label);
            current_width = label_width;
        } else {
            current.push(label);
            current_width = next_width;
        }
    }
    if !current.is_empty() {
        rows.push(current);
    }
    rows
}

/// Build the styled spans for one packed row of chips: each chip is its name
/// padded by a space on each side, painted with `label_color` as the background
/// and `contrast_text` as the foreground, separated by a muted single-column
/// gap. The analogue of GHUI's `LabelChips`.
pub fn chip_spans(row: &[&Label], palette: &Palette) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(row.len() * 2);
    for (index, label) in row.iter().enumerate() {
        if index > 0 {
            // The inter-chip gap reuses the muted role rather than a hardcoded
            // colour (it only ever paints a space, so the colour is moot, but it
            // keeps every span palette-routed per ADR 0005).
            spans.push(Span::styled(" ", Style::default().fg(palette.muted)));
        }
        let bg = label_color(label);
        spans.push(Span::styled(
            format!(" {} ", label.name),
            Style::default().fg(contrast_text(bg)).bg(bg),
        ));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    fn label(name: &str, color: Option<&str>) -> Label {
        Label {
            name: name.to_owned(),
            color: color.map(str::to_owned),
        }
    }

    #[test]
    fn label_color_uses_the_github_colour_when_present() {
        assert_eq!(
            label_color(&label("bug", Some("d73a4a"))),
            Color::Rgb(0xd7, 0x3a, 0x4a)
        );
    }

    #[test]
    fn label_color_falls_back_to_a_stable_hash_when_blank() {
        // No GitHub colour -> the name-hashed fallback, stable across calls.
        let l = label("needs-triage", None);
        assert_eq!(label_color(&l), hash_hue("needs-triage"));
        assert_eq!(label_color(&l), label_color(&l));
    }

    #[test]
    fn label_color_falls_back_when_the_colour_is_unparseable() {
        // A malformed hex (not six hex digits) is treated like a blank colour.
        let l = label("wontfix", Some("nothex"));
        assert_eq!(label_color(&l), hash_hue("wontfix"));
    }

    #[test]
    fn chip_rows_keeps_a_single_row_when_everything_fits() {
        let labels = vec![label("a", None), label("bb", None)];
        // " a " (3) + gap (1) + " bb " (4) = 8 columns; width 8 is an exact fit.
        let rows = chip_rows(&labels, 8);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), 2);
    }

    #[test]
    fn chip_rows_breaks_to_a_new_row_on_overflow() {
        let labels = vec![label("a", None), label("bb", None)];
        // One column short of the 8-column exact fit forces the second chip down.
        let rows = chip_rows(&labels, 7);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0].name, "a");
        assert_eq!(rows[1][0].name, "bb");
    }

    #[test]
    fn chip_rows_degrades_to_one_chip_per_row_at_narrow_widths() {
        let labels = vec![label("alpha", None), label("beta", None), label("c", None)];
        // Width 1 cannot fit any chip, but no label is ever dropped: each takes
        // its own row.
        let rows = chip_rows(&labels, 1);
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.len() == 1));
        assert_eq!(rows[0][0].name, "alpha");
        assert_eq!(rows[1][0].name, "beta");
        assert_eq!(rows[2][0].name, "c");
    }

    #[test]
    fn chip_rows_never_drops_a_label() {
        let labels = vec![
            label("enhancement", None),
            label("good first issue", None),
            label("help wanted", None),
        ];
        let total: usize = chip_rows(&labels, 20).iter().map(|r| r.len()).sum();
        assert_eq!(total, labels.len());
    }

    #[test]
    fn chip_rows_is_empty_for_no_labels() {
        assert!(chip_rows(&[], 40).is_empty());
    }

    #[test]
    fn chip_rows_measures_display_width_for_wide_glyphs() {
        // A CJK glyph is two display columns. Two "中" chips (display width 2 ->
        // chip width 4 each) overflow width 8 once the inter-chip gap is counted,
        // so they wrap. A scalar-count measure (chip width 3) would pack them on
        // one row and disagree with what ratatui paints.
        let labels = vec![label("中", None), label("中", None)];
        let rows = chip_rows(&labels, 8);
        assert_eq!(rows.len(), 2, "wide-glyph chips must wrap by display width");
    }
}
