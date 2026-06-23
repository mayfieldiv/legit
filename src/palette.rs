//! The curated, dark-first truecolor palette (ADR 0005). One `Palette` of
//! semantic roles that every view call site reads instead of naming a raw ANSI
//! colour, so the scheme lives in one place and a future terminal-derived theme
//! is a drop-in rather than a rewrite of the view layer.
//!
//! Dark-first by design: the roles are tuned for a dark background and light
//! terminals will look wrong until a future theme lands — a known item, not a
//! bug. The one exception is `text`, left as the terminal's own foreground
//! (`Color::Reset`) so body copy stays legible on whatever background the user
//! runs, while the accents and status hues are curated truecolor values.

use ratatui::style::Color;

use crate::blocker::Tier;
use crate::color::parse_hex;

/// The single curated palette instance, resolved at compile time. Every colour
/// in the app resolves through this: the view layer threads it from here, and the
/// shared formatting / markdown / detail-layout helpers (whose coloured output is
/// cached in the model or measured by `update`, so they can't take a per-frame
/// palette argument) read it directly. A future runtime-selected theme replaces
/// this single source — the additive change ADR 0005 anticipates.
pub const DARK: Palette = Palette::dark();

/// Resolve a palette hex literal at compile time. The literals below are valid
/// six-digit hex by construction, so a malformed one is a programming error —
/// and because this is `const`, it fails the build rather than panicking at
/// runtime.
const fn hex(literal: &str) -> Color {
    match parse_hex(literal) {
        Some(color) => color,
        None => panic!("palette colours are valid six-digit hex literals"),
    }
}

/// A curated set of semantic colour roles. Every field is the colour for one
/// role the view paints; views reference role names, never hex or `Color::`
/// literals. Several roles share a value today (e.g. `passing`/`approved`,
/// `failing`/`changes_requested`) but stay distinct so a future theme can tune
/// them independently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    /// Default body text. Left as the terminal foreground so copy stays legible
    /// on any background; the curated values are reserved for accents and status.
    pub text: Color,
    /// Secondary labels and dim placeholders (section labels, "Loading…", the
    /// idle network indicator).
    pub muted: Color,
    /// Structural rules and inline separators (the panel divider, " · " joins).
    pub separator: Color,
    /// App identity and interactive emphasis (the `legit` title, section
    /// headers, branch refs, the active tab, the filter chip).
    pub accent: Color,
    /// Hyperlinks (the PR's GitHub URL).
    pub link: Color,
    /// Inline code and code-block bodies in rendered markdown.
    pub code: Color,
    /// PR numbers — an identity cue distinct from `accent` so the two can diverge.
    pub count: Color,
    /// PR author names.
    pub author: Color,
    /// Attention text that is not itself a check or review status (the detail
    /// of a surfaced error).
    pub warning: Color,
    /// Hard error chrome (the `error:` status prefix, an error-kind status line).
    pub error: Color,

    /// Smart-status tier: I am blocking this PR.
    pub tier_me_blocking: Color,
    /// Smart-status tier: waiting on the author.
    pub tier_waiting_on_author: Color,
    /// Smart-status tier: needs review.
    pub tier_needs_review: Color,

    /// CI checks that passed.
    pub passing: Color,
    /// CI checks still running or queued.
    pub pending: Color,
    /// CI checks that failed.
    pub failing: Color,
    /// A review that approved.
    pub approved: Color,
    /// A review that requested changes.
    pub changes_requested: Color,
    /// A review that only commented.
    pub commented: Color,
    /// A draft PR marker.
    pub draft: Color,
    /// A merged PR's lifecycle state.
    pub merged: Color,
}

impl Palette {
    /// The single curated dark palette. Hues mirror the app's established
    /// semantics (cyan accent, green author/passing, magenta me-blocking, amber
    /// pending/draft, red failing) pinned to balanced dark-background truecolor
    /// values rather than the terminal's sixteen ANSI colours.
    pub const fn dark() -> Self {
        Self {
            text: Color::Reset,
            muted: hex("#7d8590"),
            separator: hex("#4b5263"),
            accent: hex("#56b6c2"),
            link: hex("#61afef"),
            code: hex("#7ec8d3"),
            count: hex("#56b6c2"),
            author: hex("#98c379"),
            warning: hex("#e5c07b"),
            error: hex("#e06c75"),

            tier_me_blocking: hex("#c678dd"),
            tier_waiting_on_author: hex("#e5c07b"),
            tier_needs_review: hex("#7d8590"),

            passing: hex("#98c379"),
            pending: hex("#e5c07b"),
            failing: hex("#e06c75"),
            approved: hex("#98c379"),
            changes_requested: hex("#e06c75"),
            commented: hex("#61afef"),
            draft: hex("#e5c07b"),
            merged: hex("#c678dd"),
        }
    }

    /// The colour for a Smart-status tier — the one tier-to-role mapping, shared
    /// by the list's action cell and the summary panel's Next Action line.
    pub fn tier(&self, tier: Tier) -> Color {
        match tier {
            Tier::MeBlocking => self.tier_me_blocking,
            Tier::WaitingOnAuthor => self.tier_waiting_on_author,
            Tier::NeedsReview => self.tier_needs_review,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_follows_the_terminal_foreground() {
        // Dark-first does not mean a hardcoded body-text colour: `text` stays the
        // terminal's own foreground so copy is legible on any background.
        assert_eq!(Palette::dark().text, Color::Reset);
    }

    #[test]
    fn tier_resolves_each_smart_status_tier_to_its_role() {
        let palette = Palette::dark();
        assert_eq!(palette.tier(Tier::MeBlocking), palette.tier_me_blocking);
        assert_eq!(
            palette.tier(Tier::WaitingOnAuthor),
            palette.tier_waiting_on_author
        );
        assert_eq!(palette.tier(Tier::NeedsReview), palette.tier_needs_review);
    }

    #[test]
    fn every_role_but_text_resolves_to_a_truecolor_value() {
        // The seam's contract: roles are curated truecolor, not ANSI names. Only
        // `text` is intentionally the terminal default.
        let p = Palette::dark();
        for (name, color) in [
            ("muted", p.muted),
            ("separator", p.separator),
            ("accent", p.accent),
            ("link", p.link),
            ("code", p.code),
            ("count", p.count),
            ("author", p.author),
            ("warning", p.warning),
            ("error", p.error),
            ("tier_me_blocking", p.tier_me_blocking),
            ("tier_waiting_on_author", p.tier_waiting_on_author),
            ("tier_needs_review", p.tier_needs_review),
            ("passing", p.passing),
            ("pending", p.pending),
            ("failing", p.failing),
            ("approved", p.approved),
            ("changes_requested", p.changes_requested),
            ("commented", p.commented),
            ("draft", p.draft),
            ("merged", p.merged),
        ] {
            assert!(
                matches!(color, Color::Rgb(_, _, _)),
                "role {name} should be a curated truecolor value, got {color:?}"
            );
        }
    }
}
