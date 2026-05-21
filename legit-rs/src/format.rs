//! Pure display formatters. Take inputs explicitly (no `Utc::now()` or other
//! ambient state) so they're trivially testable.

use chrono::{DateTime, Utc};

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

/// Truncate `s` to at most `max` display chars, appending `…` when shortened.
pub fn truncate(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return s.to_owned();
    }
    let head: String = chars.iter().take(max.saturating_sub(1)).collect();
    format!("{head}…")
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::{format_age, format_size, truncate};

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
}
