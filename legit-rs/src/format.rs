//! Pure display formatters. Take inputs explicitly (no `Utc::now()` or other
//! ambient state) so they're trivially testable.

use chrono::{DateTime, Utc};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Format a past instant as a compact age relative to `now`. Mirrors the TS
/// `formatAge` in `src/lib/format.ts`: "now", "Nm", "Nh", "Nd", "Nmo",
/// "NyNmo" / "Ny".
pub fn format_age(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (now - then).num_seconds().max(0);
    if seconds < 60 {
        return "now".to_owned();
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{hours}h");
    }
    let days = hours / 24;
    if days < 30 {
        return format!("{days}d");
    }
    let months = days / 30;
    if months < 12 {
        return format!("{months}mo");
    }
    let years = months / 12;
    let rem = months % 12;
    if rem > 0 {
        format!("{years}y{rem}mo")
    } else {
        format!("{years}y")
    }
}

/// Format additions/deletions as `+A/-D`.
pub fn format_size(additions: u64, deletions: u64) -> String {
    format!("+{additions}/-{deletions}")
}

/// Truncate `s` to at most `max` terminal columns, appending `…` when
/// shortened. Width is measured in display columns (via `unicode-width`), not
/// `char` count: CJK ideographs and emoji occupy two columns, so a char-count
/// truncation would overflow the column it's sized for.
pub fn truncate(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if s.width() <= max {
        return s.to_owned();
    }
    // Reserve one column for the ellipsis, then take chars until the next one
    // would spill past the budget. A wide char straddling the boundary is
    // dropped whole rather than clipped to half a glyph.
    let budget = max - 1;
    let mut width = 0;
    let mut head = String::new();
    for ch in s.chars() {
        let w = ch.width().unwrap_or(0);
        if width + w > budget {
            break;
        }
        width += w;
        head.push(ch);
    }
    format!("{head}…")
}

/// Right-pad `s` with spaces to at least `width` terminal columns. Like
/// `format!("{s:<width$}")` but measures display columns instead of `char`
/// count, so columns stay aligned when a cell contains wide glyphs.
pub fn pad_to_width(s: &str, width: usize) -> String {
    let used = s.width();
    if used >= width {
        return s.to_owned();
    }
    format!("{s}{}", " ".repeat(width - used))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use unicode_width::UnicodeWidthStr;

    use super::{format_age, format_size, pad_to_width, truncate};

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 5, 20, 12, 0, 0).unwrap()
    }

    #[test]
    fn format_age_under_minute_is_now() {
        let then = now() - chrono::Duration::seconds(45);
        assert_eq!(format_age(then, now()), "now");
    }

    #[test]
    fn format_age_returns_compact_units() {
        assert_eq!(
            format_age(now() - chrono::Duration::minutes(15), now()),
            "15m"
        );
        assert_eq!(format_age(now() - chrono::Duration::hours(3), now()), "3h");
        assert_eq!(format_age(now() - chrono::Duration::hours(48), now()), "2d");
        assert_eq!(format_age(now() - chrono::Duration::days(45), now()), "1mo");
    }

    #[test]
    fn format_size_renders_additions_and_deletions() {
        assert_eq!(format_size(5, 3), "+5/-3");
        assert_eq!(format_size(0, 0), "+0/-0");
    }

    #[test]
    fn truncate_leaves_short_strings_alone() {
        assert_eq!(truncate("hi", 10), "hi");
    }

    #[test]
    fn truncate_appends_ellipsis_for_long_strings() {
        assert_eq!(truncate("abcdefghij", 5), "abcd…");
    }

    #[test]
    fn truncate_measures_wide_chars_by_display_width() {
        // Each CJK ideograph is two columns wide. At max=5 the budget before
        // the ellipsis is 4 columns, so only two ideographs (4 cols) fit.
        let result = truncate("一二三四五", 5);
        assert_eq!(result, "一二…");
        assert!(result.width() <= 5, "must fit the column: {result:?}");
    }

    #[test]
    fn pad_to_width_counts_display_columns() {
        // Two ideographs already fill four columns; padding to 6 adds two
        // spaces, not "6 - char_count".
        assert_eq!(pad_to_width("一二", 6), "一二  ");
        assert_eq!(pad_to_width("ab", 5), "ab   ");
        assert_eq!(pad_to_width("already wide", 4), "already wide");
    }
}
