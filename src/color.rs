//! Pure colour maths for the view layer. Kept dependency-free and
//! side-effect-free so the colour values a frame paints can be asserted
//! directly in unit tests. Today it is just `parse_hex`, the primitive the
//! curated `palette` is defined in terms of; the repo-colour and label-chip
//! work layer their own helpers (hue hashing, contrast flips) on top later.

use ratatui::style::Color;

/// Parse a `#rrggbb` (or bare `rrggbb`) hex string into a truecolor [`Color`].
///
/// Case-insensitive. Returns `None` for anything that isn't exactly six ASCII
/// hex digits — an empty, short, over-long, or non-hex value — so a malformed
/// colour (e.g. a label colour GitHub left blank) can fall back to a default
/// rather than panic.
pub fn parse_hex(s: &str) -> Option<Color> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    // `hex` is six ASCII bytes, so byte-indexing lands on char boundaries.
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_valid_six_digit_hex() {
        assert_eq!(parse_hex("#56b6c2"), Some(Color::Rgb(0x56, 0xb6, 0xc2)));
    }

    #[test]
    fn is_case_insensitive() {
        assert_eq!(parse_hex("#56B6C2"), Some(Color::Rgb(0x56, 0xb6, 0xc2)));
    }

    #[test]
    fn accepts_a_bare_hex_without_the_leading_hash() {
        assert_eq!(parse_hex("98c379"), Some(Color::Rgb(0x98, 0xc3, 0x79)));
    }

    #[test]
    fn rejects_a_short_hex() {
        // A three-digit shorthand is not expanded; it is simply invalid here.
        assert_eq!(parse_hex("#fff"), None);
        assert_eq!(parse_hex("#12345"), None);
    }

    #[test]
    fn rejects_an_over_long_hex() {
        // Eight-digit (rgba) and other longer strings are out of scope.
        assert_eq!(parse_hex("#1234567"), None);
        assert_eq!(parse_hex("#56b6c2ff"), None);
    }

    #[test]
    fn rejects_non_hex_and_empty_values() {
        assert_eq!(parse_hex("#zzzzzz"), None);
        assert_eq!(parse_hex("#12345g"), None);
        assert_eq!(parse_hex(""), None);
        assert_eq!(parse_hex("#"), None);
        // "ééé" is three 2-byte chars: six bytes, so it passes the length check
        // and must be rejected by the ASCII guard rather than panicking on a
        // non-char-boundary byte index.
        assert_eq!(parse_hex("ééé"), None);
    }
}
