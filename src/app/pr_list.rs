//! Open PR List Module: the pooled PRs of every Tracked Repo, plus the user's
//! selection cursor, scroll viewport, per-repo fetch phases, and how the list
//! is grouped. The active Repo Tab and filter narrow the pool to a visible
//! subset at `relayout` time — there is no per-tab cache. Concentrates the
//! invariants that used to be spread across `Model` and `update.rs`.
//!
//! Grouping turns the flat PR vec into a `Vec<DisplayRow>` (headers + PR rows).
//! Selection tracks a *PR index* (so it survives regrouping), while scrolling
//! works over display rows (headers included). `j`/`k` step PR-to-PR, skipping
//! header rows; keyboard navigation keeps the selected PR's row on-screen,
//! while wheel scrolling can move the viewport independently of selection.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};
use std::fmt;

use crate::app::grouping::{DisplayRow, Grouping, display_rows};
use crate::app::list_cursor::{Direction, ListCursor, SelectableRow};
use crate::blocker::Tier;
use crate::github::rest::{PR, PrKey};
use crate::repo_slug::RepoSlug;

/// The substring filter over the Open PR List. `/` opens editing; Enter locks
/// the text in; Esc clears. `Applied("")` is unrepresentable — submitting an
/// empty filter returns to `Inactive` — so "filter active" is exactly
/// "non-empty text".
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    Inactive,
    /// `/` pressed; every keystroke edits the text and re-filters live, and
    /// the editor consumes all keys except Esc/Enter.
    Editing(String),
    /// Enter pressed; the text keeps narrowing the list while normal-mode
    /// keys work again.
    Applied(String),
}

impl Filter {
    /// The text currently narrowing the list (`""` when inactive).
    pub fn text(&self) -> &str {
        match self {
            Filter::Inactive => "",
            Filter::Editing(text) | Filter::Applied(text) => text,
        }
    }

    pub fn is_editing(&self) -> bool {
        matches!(self, Filter::Editing(_))
    }

    /// Whether the filter chip row is on screen (editing or applied).
    pub fn is_visible(&self) -> bool {
        !matches!(self, Filter::Inactive)
    }
}

/// The filter text, classified once per relayout (not per PR) into the shape
/// it matches by:
///
/// - GitHub PR URL — `https://github.com/owner/repo/pull/N` with optional
///   scheme, `www.`, and trailing segments (`/changes`, `/files`, …). Matches
///   that exact `owner/repo` + number.
/// - Worktree path — any string containing `/` whose leaf is `{N}-{branch}`
///   (legit's worktree directory naming). Matches by PR number. Requiring a
///   separator keeps a title search like `1-click` on the substring path.
/// - Otherwise, a case-insensitive substring over title, author, and number;
///   the number also matches with a leading `#` (`#42`).
///
/// Both paste shapes fall back to `Substring` while incomplete, so ordinary
/// matching still applies as the user types.
#[derive(Debug, PartialEq, Eq)]
enum FilterQuery {
    /// Empty filter: everything matches.
    All,
    PrUrl {
        slug: String,
        number: u64,
    },
    WorktreePath(u64),
    Substring(String),
}

impl FilterQuery {
    fn parse(text: &str) -> Self {
        let needle = text.trim().to_lowercase();
        if needle.is_empty() {
            return Self::All;
        }
        if let Some((slug, number)) = parse_github_pr_url_filter(&needle) {
            return Self::PrUrl { slug, number };
        }
        if let Some(number) = parse_worktree_path_filter(&needle) {
            return Self::WorktreePath(number);
        }
        Self::Substring(needle)
    }

    fn matches(&self, pr: &PR) -> bool {
        match self {
            Self::All => true,
            Self::PrUrl { slug, number } => pr.repo_slug == slug.as_str() && pr.number == *number,
            Self::WorktreePath(number) => pr.number == *number,
            Self::Substring(needle) => {
                let number_needle = needle.strip_prefix('#').unwrap_or(needle);
                pr.title.to_lowercase().contains(needle)
                    || pr.author.to_lowercase().contains(needle)
                    || (!number_needle.is_empty() && pr.number.to_string().contains(number_needle))
            }
        }
    }
}

/// Parse a pasted GitHub PR URL into `(owner/repo, number)`. `needle` is
/// already lowercased. Returns `None` for incomplete or non-URL text.
fn parse_github_pr_url_filter(needle: &str) -> Option<(String, u64)> {
    let rest = needle
        .strip_prefix("https://")
        .or_else(|| needle.strip_prefix("http://"))
        .unwrap_or(needle);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let rest = rest.strip_prefix("github.com/")?;
    let mut parts = rest.split('/');
    let owner = parts.next().filter(|s| !s.is_empty())?;
    let repo = parts.next().filter(|s| !s.is_empty())?;
    if parts.next() != Some("pull") {
        return None;
    }
    // Trailing query/fragment on the number segment (unusual, but cheap).
    let number = parts.next()?.split(['#', '?']).next()?.parse().ok()?;
    Some((format!("{owner}/{repo}"), number))
}

/// Parse a pasted worktree path into the PR number named by its leaf.
fn parse_worktree_path_filter(needle: &str) -> Option<u64> {
    if !needle.contains('/') {
        return None;
    }
    let leaf = needle.trim_end_matches('/').rsplit('/').next()?;
    crate::worktree::parse_worktree_leaf(leaf)
}

/// Most recent GitHub activity first. Creation time keeps same-second updates
/// chronological; identity makes the order total and independent of arrival.
fn compare_recent_activity(a: &PR, b: &PR) -> Ordering {
    b.updated_at
        .cmp(&a.updated_at)
        .then_with(|| b.created_at.cmp(&a.created_at))
        .then_with(|| a.repo_slug.cmp(&b.repo_slug))
        .then_with(|| a.number.cmp(&b.number))
}

impl SelectableRow for DisplayRow {
    type Id = usize;
    fn id(&self) -> Option<&usize> {
        match self {
            DisplayRow::Pr(index) => Some(index),
            DisplayRow::Header(_) => None,
        }
    }
}

/// Lifecycle of one Tracked Repo's open-PR fetch. A repo with no entry in
/// `PrList::phases` hasn't had a fetch dispatched yet. At most one variant
/// holds per repo, so the view never has to ask "are we loading AND failed?".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Fetch in flight. The set holds the PR numbers streamed since this repo's
    /// `begin_fetch`, used to reconcile membership when the listing settles:
    /// `finish_listing` prunes pooled PRs whose number didn't reappear
    /// (closed/merged since). Numbers, not full keys — the slug is already this
    /// phase's map key. Living inside `Loading` ties the seen-set's lifetime to
    /// the in-flight listing: completing or failing the fetch replaces the
    /// variant and drops it, so a settled repo can never carry a stale set.
    Loading(HashSet<u64>),
    /// Fetch completed (rows may still be 0).
    Loaded,
    /// Fetch returned an error; the message is what the status bar surfaces.
    Failed(String),
}

#[derive(Clone, Default)]
pub struct PrList {
    prs: Vec<PR>,
    /// Per-Tracked-Repo fetch lifecycle, keyed by slug. BTreeMap so `failure`
    /// reports deterministically (alphabetical) when several repos fail. The
    /// `Loading` phase also carries the cycle's seen-set for membership
    /// reconciliation (see `Phase`).
    phases: BTreeMap<RepoSlug, Phase>,
    /// The substring filter narrowing the visible set (with the active tab).
    filter: Filter,
    grouping: Grouping,
    /// Flattened display layout (headers + PR rows). Rebuilt by `relayout`
    /// whenever the PRs, their tiers, or the grouping change.
    rows: Vec<DisplayRow>,
    /// The selection (a PR index) and scroll viewport over `rows`.
    cursor: ListCursor<usize>,
}

impl fmt::Debug for PrList {
    /// Renders the list as `{ phases, prs: <len>, ... }` — the full PR vec is
    /// noisy in `tracing` output and rarely informative compared to its length.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrList")
            .field("phases", &self.phases)
            .field("prs", &self.prs.len())
            .field("grouping", &self.grouping)
            .field("cursor", &self.cursor)
            .finish()
    }
}

impl PrList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn begin_fetch(&mut self, repo_slug: &RepoSlug) {
        // A fresh `Loading` phase starts with an empty seen-set; this cycle's
        // arrivals populate it, defining which PRs the repo still has when
        // `finish_listing` reconciles. Replacing any prior phase drops a
        // previous cycle's set.
        self.phases
            .insert(repo_slug.to_owned(), Phase::Loading(HashSet::new()));
    }

    pub fn complete_fetch(&mut self, repo_slug: &RepoSlug) {
        self.phases.insert(repo_slug.clone(), Phase::Loaded);
    }

    pub fn fail_fetch(&mut self, repo_slug: &RepoSlug, message: String) {
        self.phases
            .insert(repo_slug.to_owned(), Phase::Failed(message));
    }

    pub fn push(&mut self, pr: PR) {
        self.prs.push(pr);
    }

    /// Pool a PR streamed from a listing, deduping by key. A genuinely-new PR
    /// is appended; an already-pooled PR adopts the fresh listing object with
    /// its enrichment grafted back on (see `PR::adopt_listing`). Returns
    /// whether the pool changed — false when the re-streamed copy matched the
    /// pooled one exactly, so the caller can skip relaying out. The initial
    /// listing has no pooled PRs so every arrival is new; a re-list (`R`)
    /// re-streams the pooled ones, which this reconciles so they neither
    /// duplicate nor lose their enrichment.
    pub fn merge_listed(&mut self, pr: PR) -> bool {
        // Record the PR as present in this fetch cycle so `finish_listing` keeps
        // it, whether or not it was already pooled. Arrivals only occur while
        // the repo's listing is in flight, so its phase is always `Loading`.
        if let Some(Phase::Loading(seen)) = self.phases.get_mut(&pr.repo_slug) {
            seen.insert(pr.number);
        }
        if let Some(existing) = self.pr_mut(&pr.key()) {
            return existing.adopt_listing(pr);
        }
        self.push(pr);
        true
    }

    /// Settle `repo_slug`'s listing: drop pooled PRs whose number didn't arrive
    /// in this fetch cycle (closed/merged since) and mark the repo `Loaded`
    /// (which drops the seen-set with the `Loading` phase). Returns whether any
    /// PR was pruned, so the caller can `relayout` the now-stale rows. The
    /// initial listing sees every pooled PR, so it prunes nothing; an `R`-driven
    /// re-list prunes what's gone.
    pub fn finish_listing(&mut self, repo_slug: &RepoSlug) -> bool {
        // Take the seen-set out of the `Loading` phase before pruning; the
        // `complete_fetch` below replaces the phase anyway. Owning it releases
        // the borrow on `self.phases` so we can mutate `self.prs`.
        let seen = match self.phases.get_mut(repo_slug) {
            Some(Phase::Loading(seen)) => std::mem::take(seen),
            _ => HashSet::new(),
        };
        let before = self.prs.len();
        self.prs
            .retain(|pr| pr.repo_slug != *repo_slug || seen.contains(&pr.number));
        self.complete_fetch(repo_slug);
        self.prs.len() != before
    }

    /// Rebuild the display layout from the current PRs under the active
    /// grouping, showing only the PRs in `scope` (a Repo Tab's slug, or `None`
    /// for the All tab) and ordering each group by most recent GitHub activity.
    /// `tier_of(pr)` returns the Smart-status tier for a PR, or `None` when its
    /// enrichment hasn't been derived yet; the repo-grouping key is read
    /// straight off each PR's `repo_slug`. The cursor then re-anchors (see
    /// `ListCursor::re_anchor`). Called by `update` after PRs arrive,
    /// enrichment lands, or the grouping/scope changes.
    pub fn relayout(&mut self, scope: Option<&RepoSlug>, tier_of: impl Fn(&PR) -> Option<Tier>) {
        let query = FilterQuery::parse(self.filter.text());
        let mut visible: Vec<usize> = (0..self.prs.len())
            .filter(|&i| scope.is_none_or(|slug| self.prs[i].repo_slug == *slug))
            .filter(|&i| query.matches(&self.prs[i]))
            .collect();
        visible.sort_by(|&a, &b| compare_recent_activity(&self.prs[a], &self.prs[b]));
        // `display_rows` keys on PR index; adapt the &PR closure (and the slug
        // we own) into index closures. Build into a local so the index closures
        // can borrow `self.prs` while we hold `&mut self`, then store the rows.
        let prs = &self.prs;
        let rows = display_rows(
            &visible,
            self.grouping,
            |i| tier_of(&prs[i]),
            |i| prs[i].repo_slug.clone(),
        );
        self.rows = rows;
        self.cursor.re_anchor(&self.rows);
    }

    /// Whether the current layout shows no PR rows at all — the placeholder
    /// state. Distinct from `prs.is_empty()`: a Repo Tab or filter can hide
    /// every pooled PR.
    pub fn visible_is_empty(&self) -> bool {
        !self.rows.iter().any(|r| matches!(r, DisplayRow::Pr(_)))
    }

    /// Whether `scope` (a Repo Tab's slug, or `None` for the All tab) admits any
    /// pooled PR, ignoring the filter. Stateless — recomputed from `prs` rather
    /// than cached — so it can't drift from the current PRs. Read by refresh to
    /// tell a genuinely-empty repo (re-list it) from one whose PRs a filter just
    /// hid (leave them be).
    pub fn any_in_scope(&self, scope: Option<&RepoSlug>) -> bool {
        self.prs
            .iter()
            .any(|pr| scope.is_none_or(|s| pr.repo_slug == *s))
    }

    /// True when the filter (not the tab) is why the list is empty: `scope`
    /// admitted PRs but the filter text matched none. Drives the
    /// "No matching PRs" placeholder.
    pub fn filter_hid_everything(&self, scope: Option<&RepoSlug>) -> bool {
        self.visible_is_empty() && self.any_in_scope(scope) && !self.filter.text().is_empty()
    }

    pub fn filter(&self) -> &Filter {
        &self.filter
    }

    /// `/` pressed: enter filter editing, resuming an applied filter's text so
    /// `/` acts as "edit the current filter" rather than starting over.
    pub fn filter_open(&mut self) {
        self.filter = Filter::Editing(self.filter.text().to_owned());
    }

    pub fn filter_push(&mut self, c: char) {
        if let Filter::Editing(text) = &mut self.filter {
            text.push(c);
        }
    }

    pub fn filter_backspace(&mut self) {
        if let Filter::Editing(text) = &mut self.filter {
            text.pop();
        }
    }

    /// Enter pressed: lock the filter in (back to normal-mode keys, matches
    /// still narrowed). Submitting empty text deactivates instead — an empty
    /// `Applied` would be an invisible no-op chip.
    pub fn filter_submit(&mut self) {
        let text = self.filter.text().to_owned();
        self.filter = if text.is_empty() {
            Filter::Inactive
        } else {
            Filter::Applied(text)
        };
    }

    /// Esc pressed (editing or applied): drop the filter entirely.
    pub fn filter_clear(&mut self) {
        self.filter = Filter::Inactive;
    }

    /// Absolute PR indices currently in the display layout (post scope), in
    /// display order. The view sizes its columns from these so an off-tab PR
    /// can't widen this tab's columns.
    pub fn visible_pr_indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.rows.iter().filter_map(|row| match row {
            DisplayRow::Pr(i) => Some(*i),
            DisplayRow::Header(_) => None,
        })
    }

    /// PR numbers of the display rows, in display order — the assertion
    /// ordering tests make. Test-only sugar over `visible_pr_indices`.
    #[cfg(test)]
    pub fn pr_numbers_in_display_order(&self) -> Vec<u64> {
        self.visible_pr_indices()
            .map(|i| self.prs[i].number)
            .collect()
    }

    pub fn grouping(&self) -> Grouping {
        self.grouping
    }

    /// Advance to the next grouping mode and reset the selection to the top.
    /// The caller must `relayout` after to rebuild the rows; the new grouping
    /// is in effect immediately.
    pub fn cycle_grouping(&mut self) {
        self.grouping = self.grouping.next();
        self.cursor.reset_to_top(&self.rows);
    }

    /// Reset the selection to the first visible PR and scroll to the top. Used
    /// on tab switches, where the spec resets to the top of the new tab rather
    /// than chasing the previously selected PR.
    pub fn select_first_visible(&mut self) {
        self.cursor.reset_to_top(&self.rows);
    }

    pub fn move_down(&mut self) {
        self.cursor.step(&self.rows, Direction::Down);
    }

    pub fn move_up(&mut self) {
        self.cursor.step(&self.rows, Direction::Up);
    }

    pub fn resize(&mut self, viewport_height: usize) {
        self.cursor.resize(&self.rows, viewport_height);
    }

    pub fn scroll_down(&mut self, rows: usize) {
        self.cursor.scroll_down(&self.rows, rows);
    }

    pub fn scroll_up(&mut self, rows: usize) {
        self.cursor.scroll_up(&self.rows, rows);
    }

    pub fn select_visible_row(&mut self, visible_row: usize) -> bool {
        self.cursor.select_visible_row(&self.rows, visible_row)
    }

    /// Index of the first display row inside the scroll window. Exposed for
    /// tests and future debug overlays; the view itself prefers `visible_rows`.
    #[allow(dead_code)]
    pub fn scroll_offset(&self) -> usize {
        self.cursor.offset()
    }

    /// Number of rows currently allotted to the list (terminal height minus
    /// the status bar). Set via `resize`. Exposed for tests/inspection.
    #[allow(dead_code)]
    pub fn viewport_height(&self) -> usize {
        self.cursor.height()
    }

    pub fn prs(&self) -> &[PR] {
        &self.prs
    }

    /// The selected PR, or `None` while the visible list is empty. The summary
    /// panel reads this to know which PR to render.
    pub fn selected_pr(&self) -> Option<&PR> {
        self.cursor.selected().map(|&index| &self.prs[index])
    }

    /// Immutable access to a PR by key. Used by the detail view to look up the
    /// current PR for `r` (refresh). `None` if no PR with that key is in the list.
    pub fn pr(&self, key: &PrKey) -> Option<&PR> {
        self.prs
            .iter()
            .find(|pr| pr.repo_slug == key.repo_slug && pr.number == key.number)
    }

    /// Mutable access to a streamed PR by key, for enrichment that overwrites
    /// list fields in place (mergeable, review decision, size, head SHA). `None`
    /// if no PR with that key is in the list.
    pub fn pr_mut(&mut self, key: &PrKey) -> Option<&mut PR> {
        self.prs
            .iter_mut()
            .find(|pr| pr.repo_slug == key.repo_slug && pr.number == key.number)
    }

    /// Iterate the display rows currently inside the scroll viewport. Each item
    /// is the row plus whether it is the selected PR (so the view can highlight
    /// it). Headers are never marked selected.
    pub fn visible_rows(&self) -> impl Iterator<Item = (&DisplayRow, bool)> {
        self.cursor.visible_rows(&self.rows)
    }

    /// Fetch phase for one Tracked Repo, or `None` when no fetch has been
    /// dispatched for it yet. Test-only; the view asks the scope-aware
    /// `is_loading` instead.
    #[cfg(test)]
    pub fn phase_of(&self, repo_slug: &RepoSlug) -> Option<&Phase> {
        self.phases.get(repo_slug)
    }

    /// Whether `repo_slug` should have an open-PR listing dispatched: it has
    /// never been fetched, or its last listing *failed* (so it retries). False
    /// while a listing is in flight or has already loaded — re-dispatching then
    /// would re-stream and duplicate the pooled PRs. The config-reload gate
    /// (`R`) uses this to fetch only newly tracked or previously-failed repos.
    pub fn needs_listing(&self, repo_slug: &RepoSlug) -> bool {
        match self.phases.get(repo_slug) {
            None | Some(Phase::Failed(_)) => true,
            Some(Phase::Loading(_) | Phase::Loaded) => false,
        }
    }

    /// Whether a listing is still in flight for `scope`: a specific repo slug,
    /// or `None` meaning "any Tracked Repo" (the All tab).
    pub fn is_loading(&self, scope: Option<&RepoSlug>) -> bool {
        match scope {
            Some(slug) => matches!(self.phases.get(slug), Some(Phase::Loading(_))),
            None => self.phases.values().any(|p| matches!(p, Phase::Loading(_))),
        }
    }

    /// The per-repo failure the status bar surfaces: the first `Failed` phase in
    /// slug order. `None` when no listing has failed. App-level fatals (a
    /// malformed config) live on `Model::fatal` and take precedence in the view.
    pub fn failure(&self) -> Option<&str> {
        self.phases.values().find_map(|phase| match phase {
            Phase::Failed(message) => Some(message.as_str()),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests;
