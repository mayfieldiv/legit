//! PR list grouping: turn the flat Open PR List into ordered groups with a
//! header per group, then flatten back into the display rows the list view
//! renders (a header row followed by its PR rows). Pure — given the PRs, their
//! Smart-status tiers, the repo slug, and the active mode, it produces the same
//! rows every time.
//!
//! Scope mirrors the slice of `src/lib/group-filter-engine.ts` that issue #46
//! needs: smart-status, repo, and none. Empty groups are suppressed; smart-
//! status groups render in tier order (Me blocking / Needs review / Waiting on
//! author), repo groups alphabetically.

use crate::blocker::Tier;

/// How the Open PR List is grouped. `g` cycles through these in order, wrapping
/// back to `SmartStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Grouping {
    /// Group by Smart-status tier (the default).
    #[default]
    SmartStatus,
    /// Group by repository slug (`owner/repo`).
    Repo,
    /// No grouping — one flat list with no headers.
    None,
}

impl Grouping {
    /// The next mode in the `g`-cycle: SmartStatus -> Repo -> None -> SmartStatus.
    pub fn next(self) -> Self {
        match self {
            Grouping::SmartStatus => Grouping::Repo,
            Grouping::Repo => Grouping::None,
            Grouping::None => Grouping::SmartStatus,
        }
    }
}

/// One row in the rendered list: either a group header or a PR. `Pr` carries the
/// absolute index into the underlying PR list so selection (a PR index) maps to
/// a display row and back without a second lookup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DisplayRow {
    Header(String),
    Pr(usize),
}

/// Build the display rows for the PRs at `visible` (absolute indices into the
/// underlying PR list, in display order) under `grouping`. Indices excluded by
/// the active Repo Tab or filter simply aren't passed in.
///
/// - `tier_of` returns the Smart-status tier for a PR by index, or `None` when
///   its enrichment hasn't been derived yet (those PRs collect under a trailing
///   "Loading details…" group, matching the TS engine).
/// - `slug_of` returns a PR's Tracked Repo slug, used as its repo-grouping key
///   (so the All tab groups under one header per repo).
///
/// PR order within a group preserves input order (the REST stream order). Empty
/// groups are never emitted.
pub fn display_rows(
    visible: &[usize],
    grouping: Grouping,
    tier_of: impl Fn(usize) -> Option<Tier>,
    slug_of: impl Fn(usize) -> String,
) -> Vec<DisplayRow> {
    match grouping {
        Grouping::None => visible.iter().copied().map(DisplayRow::Pr).collect(),
        // Each PR's slug is its group key. `parse_pr` always stamps a non-empty
        // `owner/repo`, so no header-key normalization is needed here.
        Grouping::Repo => grouped_rows(visible, slug_of),
        Grouping::SmartStatus => smart_status_rows(visible, tier_of),
    }
}

/// Smart-status grouping: tier-ordered groups, then a trailing "Loading details…"
/// group for PRs whose tier hasn't been derived yet.
fn smart_status_rows(
    visible: &[usize],
    tier_of: impl Fn(usize) -> Option<Tier>,
) -> Vec<DisplayRow> {
    // Collect indices per tier, preserving input order within each tier.
    let mut tiers: Vec<(Tier, Vec<usize>)> = Vec::new();
    let mut loading: Vec<usize> = Vec::new();
    for &i in visible {
        match tier_of(i) {
            Some(tier) => match tiers.iter_mut().find(|(t, _)| *t == tier) {
                Some((_, members)) => members.push(i),
                None => tiers.push((tier, vec![i])),
            },
            None => loading.push(i),
        }
    }
    tiers.sort_by_key(|(tier, _)| tier.order());

    let mut rows = Vec::with_capacity(visible.len() + tiers.len() + 1);
    for (tier, members) in tiers {
        rows.push(DisplayRow::Header(tier.label().to_owned()));
        rows.extend(members.into_iter().map(DisplayRow::Pr));
    }
    if !loading.is_empty() {
        rows.push(DisplayRow::Header("Loading details…".to_owned()));
        rows.extend(loading.into_iter().map(DisplayRow::Pr));
    }
    rows
}

/// Generic single-key grouping (used by repo): bucket indices by the key
/// `key_of(i)` produces, emit groups sorted alphabetically by key, headers
/// labelled with the key.
fn grouped_rows(visible: &[usize], key_of: impl Fn(usize) -> String) -> Vec<DisplayRow> {
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for &i in visible {
        let key = key_of(i);
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, members)) => members.push(i),
            None => groups.push((key, vec![i])),
        }
    }
    groups.sort_by(|(a, _), (b, _)| a.cmp(b));

    let mut rows = Vec::with_capacity(visible.len() + groups.len());
    for (key, members) in groups {
        rows.push(DisplayRow::Header(key));
        rows.extend(members.into_iter().map(DisplayRow::Pr));
    }
    rows
}

#[cfg(test)]
mod tests;
