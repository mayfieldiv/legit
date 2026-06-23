//! Pure colour maths for the view layer. Kept dependency-free and
//! side-effect-free so the colour values a frame paints can be asserted
//! directly in unit tests. Today it is just `parse_hex`, the primitive the
//! curated `palette` is defined in terms of; it is also where any later colour
//! maths would live (per-repo hue hashing, contrast flips for label chips) if
//! and when that work lands — none is in flight yet.

use ratatui::style::Color;

/// Parse a `#rrggbb` (or bare `rrggbb`) hex string into a truecolor [`Color`].
///
/// Case-insensitive. Returns `None` for anything that isn't exactly six ASCII
/// hex digits — an empty, short, over-long, or non-hex value — so a malformed
/// colour (e.g. a label colour GitHub left blank) can fall back to a default
/// rather than panic.
///
/// `const` so the curated `palette` can be a compile-time constant: a malformed
/// palette literal then fails the build rather than panicking on first use. That
/// is why the parse is a hand-rolled byte match — `u8::from_str_radix` is not
/// yet `const`.
pub const fn parse_hex(s: &str) -> Option<Color> {
    let bytes = s.as_bytes();
    // Accept one optional leading '#'. Work on raw bytes throughout: a non-ASCII
    // input (e.g. a 6-byte multibyte string) has no hex-digit bytes, so it is
    // rejected below without a UTF-8 boundary panic.
    let offset = if !bytes.is_empty() && bytes[0] == b'#' {
        1
    } else {
        0
    };
    if bytes.len() != 6 + offset {
        return None;
    }
    let r = match hex_byte(bytes[offset], bytes[offset + 1]) {
        Some(v) => v,
        None => return None,
    };
    let g = match hex_byte(bytes[offset + 2], bytes[offset + 3]) {
        Some(v) => v,
        None => return None,
    };
    let b = match hex_byte(bytes[offset + 4], bytes[offset + 5]) {
        Some(v) => v,
        None => return None,
    };
    Some(Color::Rgb(r, g, b))
}

/// Combine two hex-digit bytes (high then low nibble) into one byte, or `None`
/// if either is not a hex digit.
const fn hex_byte(hi: u8, lo: u8) -> Option<u8> {
    match (hex_nibble(hi), hex_nibble(lo)) {
        (Some(hi), Some(lo)) => Some((hi << 4) | lo),
        _ => None,
    }
}

/// One ASCII hex digit (`0-9`, `a-f`, `A-F`) to its 0–15 value, or `None`.
const fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
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
