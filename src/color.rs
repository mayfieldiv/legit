//! Pure colour maths for the view layer. Kept dependency-free and
//! side-effect-free so the colour values a frame paints can be asserted
//! directly in unit tests. It holds `parse_hex` (the primitive the curated
//! `palette` is defined in terms of) and the per-repo hue hashing behind the
//! Repo Color — `hash_hue` and `repo_color`. Contrast flips for label chips
//! would live here too if and when that work lands.

use ratatui::style::Color;

use crate::format::format_repo_short;

/// The curated Repo Color ramp: a fixed, ordered set of dark-tuned truecolor
/// values a repo name hashes into. The analogue of GHUI's per-repo colour map
/// in `src/ui/colors.ts`, but a hash over a ramp rather than a hand-keyed
/// table, so any repo lands on a stable hue without per-repo configuration.
///
/// Tuned for a dark background (ADR 0005): medium-saturation hues spread around
/// the wheel so distinct common repo names land on visibly distinct colours.
/// This is colour maths, not a semantic `Palette` role — a Repo Color is a
/// cosmetic identity cue, distinct from the curated semantic roles.
const REPO_RAMP: [Color; 16] = [
    Color::Rgb(0x3d, 0xa3, 0xb0), // cyan (darker than the accent role, kept disjoint from it)
    Color::Rgb(0x98, 0xc3, 0x79), // green
    Color::Rgb(0xc6, 0x78, 0xdd), // magenta
    Color::Rgb(0xe5, 0xc0, 0x7b), // amber
    Color::Rgb(0x61, 0xaf, 0xef), // blue
    Color::Rgb(0xe0, 0x6c, 0x75), // red
    Color::Rgb(0xd1, 0x9a, 0x66), // orange
    Color::Rgb(0x5c, 0xb0, 0x8a), // teal
    Color::Rgb(0xb5, 0x88, 0xf7), // violet
    Color::Rgb(0xea, 0x9a, 0xb8), // pink
    Color::Rgb(0x7e, 0xc8, 0xd3), // sky
    Color::Rgb(0xc8, 0xb0, 0x6b), // gold
    Color::Rgb(0x6c, 0xc6, 0x9a), // mint
    Color::Rgb(0xa3, 0xbe, 0x8c), // sage
    Color::Rgb(0xd0, 0x87, 0x70), // terracotta
    Color::Rgb(0x88, 0x9b, 0xe6), // periwinkle
];

/// Map a name to a stable [`Color`] drawn from the curated [`REPO_RAMP`].
///
/// Deterministic: the same seed always yields the same hue, on every call and
/// across sessions (the hash is a fixed FNV-1a over the seed bytes, not a
/// process-seeded `Hasher`). Distinct common seeds spread across the ramp so
/// typical inputs don't obviously collide.
pub fn hash_hue(seed: &str) -> Color {
    // FNV-1a (32-bit): a small, well-distributed, deterministic hash with no
    // process-random seed, so the ramp index is reproducible across sessions.
    let mut hash: u32 = 0x811c_9dc5;
    for byte in seed.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    REPO_RAMP[(hash as usize) % REPO_RAMP.len()]
}

/// The stable Repo Color for a repo `slug`. Reduces an `owner/repo` slug to its
/// repo short name (so `owner/repo` and the bare `repo` resolve to the same
/// colour), then defers to [`hash_hue`]. The analogue of GHUI's `repoColor` /
/// `shortRepoName` in `src/ui/pullRequests.ts`.
pub fn repo_color(slug: &str) -> Color {
    hash_hue(format_repo_short(slug))
}

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
    use crate::palette::DARK;

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

    #[test]
    fn hash_hue_is_stable_for_a_given_seed() {
        // Same seed -> same hue, every call (and, being a fixed hash, across
        // sessions). Asserted on the returned colour, not the hashing internals.
        assert_eq!(hash_hue("web"), hash_hue("web"));
        assert_eq!(hash_hue("api"), hash_hue("api"));
    }

    #[test]
    fn hash_hue_resolves_to_a_curated_ramp_colour() {
        // Every hue comes from the curated ramp — a truecolor value, never an
        // ANSI name or the terminal default.
        assert!(REPO_RAMP.contains(&hash_hue("web")));
        assert!(matches!(hash_hue("anything"), Color::Rgb(_, _, _)));
    }

    #[test]
    fn distinct_common_seeds_do_not_collide() {
        // A handful of typical repo short names should land on distinct hues.
        let seeds = ["web", "api", "cli", "core", "docs", "infra"];
        for (i, a) in seeds.iter().enumerate() {
            for b in &seeds[i + 1..] {
                assert_ne!(
                    hash_hue(a),
                    hash_hue(b),
                    "distinct seeds {a:?} and {b:?} collided on the ramp"
                );
            }
        }
    }

    #[test]
    fn repo_color_is_stable_across_calls() {
        assert_eq!(repo_color("acme/web"), repo_color("acme/web"));
    }

    #[test]
    fn repo_color_reduces_owner_repo_to_the_short_name() {
        // `owner/repo` and the bare `repo` short name resolve to the same hue:
        // the owner prefix is stripped before hashing.
        assert_eq!(repo_color("acme/web"), repo_color("web"));
        assert_eq!(repo_color("other-owner/web"), repo_color("web"));
    }

    #[test]
    fn repo_color_distinguishes_different_repos() {
        assert_ne!(repo_color("acme/web"), repo_color("acme/api"));
    }

    #[test]
    fn ramp_is_disjoint_from_the_accent_role() {
        // A Repo Color must never coincide with the `accent` role: the accent
        // paints the "All" scope's tab/header, so a concrete repo whose ramp
        // entry equalled it would be indistinguishable from the All scope. Keep
        // the ramp disjoint from the accent so the All-vs-repo cue is reliable.
        assert!(
            !REPO_RAMP.contains(&DARK.accent),
            "REPO_RAMP must not contain the accent role"
        );
    }
}
