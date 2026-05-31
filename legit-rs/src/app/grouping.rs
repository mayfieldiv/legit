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
use crate::github::rest::PR;

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
/// absolute index into the underlying `&[PR]` so selection (a PR index) maps to
/// a display row and back without a second lookup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DisplayRow {
    Header(String),
    Pr(usize),
}

/// Build the display rows for `prs` under `grouping`.
///
/// - `tier_of` returns the Smart-status tier for a PR by index, or `None` when
///   its enrichment hasn't been derived yet (those PRs collect under a trailing
///   "Loading details…" group, matching the TS engine).
/// - `repo_slug` is the slug shown for repo grouping; the Rust app is single-
///   repo today, so every PR shares it.
///
/// PR order within a group preserves input order (the REST stream order). Empty
/// groups are never emitted.
pub fn display_rows(
    prs: &[PR],
    grouping: Grouping,
    tier_of: impl Fn(usize) -> Option<Tier>,
    repo_slug: &str,
) -> Vec<DisplayRow> {
    match grouping {
        Grouping::None => (0..prs.len()).map(DisplayRow::Pr).collect(),
        Grouping::Repo => grouped_rows(prs, |i| Some(repo_slug_label(prs, i, repo_slug))),
        Grouping::SmartStatus => smart_status_rows(prs, tier_of),
    }
}

/// Smart-status grouping: tier-ordered groups, then a trailing "Loading details…"
/// group for PRs whose tier hasn't been derived yet.
fn smart_status_rows(prs: &[PR], tier_of: impl Fn(usize) -> Option<Tier>) -> Vec<DisplayRow> {
    // Collect indices per tier, preserving input order within each tier.
    let mut tiers: Vec<(Tier, Vec<usize>)> = Vec::new();
    let mut loading: Vec<usize> = Vec::new();
    for i in 0..prs.len() {
        match tier_of(i) {
            Some(tier) => match tiers.iter_mut().find(|(t, _)| *t == tier) {
                Some((_, members)) => members.push(i),
                None => tiers.push((tier, vec![i])),
            },
            None => loading.push(i),
        }
    }
    tiers.sort_by_key(|(tier, _)| tier.order());

    let mut rows = Vec::with_capacity(prs.len() + tiers.len() + 1);
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
/// labelled with the key. `None` from `key_of` drops the PR from grouping (not
/// used today; kept for symmetry with the smart-status loading bucket).
fn grouped_rows(prs: &[PR], key_of: impl Fn(usize) -> Option<String>) -> Vec<DisplayRow> {
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for i in 0..prs.len() {
        let Some(key) = key_of(i) else { continue };
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, members)) => members.push(i),
            None => groups.push((key, vec![i])),
        }
    }
    groups.sort_by(|(a, _), (b, _)| a.cmp(b));

    let mut rows = Vec::with_capacity(prs.len() + groups.len());
    for (key, members) in groups {
        rows.push(DisplayRow::Header(key));
        rows.extend(members.into_iter().map(DisplayRow::Pr));
    }
    rows
}

/// Repo-grouping label for a PR. Single-repo today, so `repo_slug` is the slug
/// for every PR; falls back to `"unknown"` (matching the TS engine) when the
/// app has no detected repo yet.
fn repo_slug_label(_prs: &[PR], _index: usize, repo_slug: &str) -> String {
    if repo_slug.is_empty() {
        "unknown".to_owned()
    } else {
        repo_slug.to_owned()
    }
}

#[cfg(test)]
mod tests;
