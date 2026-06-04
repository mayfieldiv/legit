//! Open PR List Module: the pooled PRs of every Tracked Repo, plus the user's
//! selection cursor, scroll viewport, per-repo fetch phases, and how the list
//! is grouped. The active Repo Tab and filter narrow the pool to a visible
//! subset at `relayout` time — there is no per-tab cache. Concentrates the
//! invariants that used to be spread across `Model` and `update.rs`.
//!
//! Grouping turns the flat PR vec into a `Vec<DisplayRow>` (headers + PR rows).
//! Selection tracks a *PR index* (so it survives regrouping), while scrolling
//! works over display rows (headers included). `j`/`k` step PR-to-PR, skipping
//! header rows; the scroll viewport keeps the selected PR's row on-screen.

use std::collections::BTreeMap;
use std::fmt;

use crate::app::grouping::{DisplayRow, Grouping, display_rows};
use crate::blocker::Tier;
use crate::github::rest::{PR, PrKey};

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

/// Case-insensitive substring match over a PR's title and author. An empty
/// needle matches everything. The needle must already be lowercased (done
/// once per relayout, not per PR).
fn filter_matches(pr: &PR, lowercase_needle: &str) -> bool {
    if lowercase_needle.is_empty() {
        return true;
    }
    pr.title.to_lowercase().contains(lowercase_needle)
        || pr.author.to_lowercase().contains(lowercase_needle)
}

/// Lifecycle of one Tracked Repo's open-PR fetch. A repo with no entry in
/// `PrList::phases` hasn't had a fetch dispatched yet. At most one variant
/// holds per repo, so the view never has to ask "are we loading AND failed?".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Fetch in flight.
    Loading,
    /// Fetch completed (rows may still be 0).
    Loaded,
    /// Fetch returned an error; the message is what the status bar surfaces.
    Failed(String),
}

#[derive(Clone, Default)]
pub struct PrList {
    prs: Vec<PR>,
    /// Per-Tracked-Repo fetch lifecycle, keyed by slug. BTreeMap so `failure`
    /// reports deterministically (alphabetical) when several repos fail.
    phases: BTreeMap<String, Phase>,
    /// The substring filter narrowing the visible set (with the active tab).
    filter: Filter,
    grouping: Grouping,
    /// Flattened display layout (headers + PR rows). Rebuilt by `relayout`
    /// whenever the PRs, their tiers, or the grouping change.
    rows: Vec<DisplayRow>,
    /// Selection cursor as an index into `prs`. Headers are never selectable.
    selected: usize,
    /// First visible display row (headers count toward the offset).
    scroll_offset: usize,
    viewport_height: usize,
}

impl fmt::Debug for PrList {
    /// Renders the list as `{ phases, prs: <len>, ... }` — the full PR vec is
    /// noisy in `tracing` output and rarely informative compared to its length.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrList")
            .field("phases", &self.phases)
            .field("prs", &self.prs.len())
            .field("grouping", &self.grouping)
            .field("selected", &self.selected)
            .field("scroll_offset", &self.scroll_offset)
            .field("viewport_height", &self.viewport_height)
            .finish()
    }
}

impl PrList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn begin_fetch(&mut self, repo_slug: &str) {
        self.phases.insert(repo_slug.to_owned(), Phase::Loading);
    }

    pub fn complete_fetch(&mut self, repo_slug: &str) {
        self.phases.insert(repo_slug.to_owned(), Phase::Loaded);
    }

    pub fn fail_fetch(&mut self, repo_slug: &str, message: String) {
        self.phases
            .insert(repo_slug.to_owned(), Phase::Failed(message));
    }

    pub fn push(&mut self, pr: PR) {
        self.prs.push(pr);
    }

    /// Rebuild the display layout from the current PRs under the active
    /// grouping, showing only the PRs in `scope` (a Repo Tab's slug, or `None`
    /// for the All tab). `tier_of(pr)` returns the Smart-status tier for a PR,
    /// or `None` when its enrichment hasn't been derived yet; the repo-grouping
    /// key is read straight off each PR's `repo_slug`. Selection sticks to the
    /// same PR when it remains visible and snaps to the first visible PR
    /// otherwise; scroll re-clamps so the selection stays on-screen. Called by
    /// `update` after PRs arrive, enrichment lands, or the grouping/scope
    /// changes.
    pub fn relayout(&mut self, scope: Option<&str>, tier_of: impl Fn(&PR) -> Option<Tier>) {
        let scoped: Vec<usize> = (0..self.prs.len())
            .filter(|&i| scope.is_none_or(|slug| self.prs[i].repo_slug == slug))
            .collect();
        let needle = self.filter.text().to_lowercase();
        let visible: Vec<usize> = scoped
            .into_iter()
            .filter(|&i| filter_matches(&self.prs[i], &needle))
            .collect();
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
        if !visible.contains(&self.selected) {
            self.selected = visible.first().copied().unwrap_or(0);
        }
        self.normalize_scroll();
    }

    /// Whether the current layout shows no PR rows at all — the placeholder
    /// state. Distinct from `prs.is_empty()`: a Repo Tab or filter can hide
    /// every pooled PR.
    pub fn visible_is_empty(&self) -> bool {
        !self.rows.iter().any(|r| matches!(r, DisplayRow::Pr(_)))
    }

    /// Whether `scope` (a Repo Tab's slug, or `None` for the All tab) admits any
    /// pooled PR, ignoring the filter. Stateless — recomputed from `prs` rather
    /// than cached — so it can't drift from the current PRs.
    fn any_in_scope(&self, scope: Option<&str>) -> bool {
        self.prs
            .iter()
            .any(|pr| scope.is_none_or(|s| pr.repo_slug == s))
    }

    /// True when the filter (not the tab) is why the list is empty: `scope`
    /// admitted PRs but the filter text matched none. Drives the
    /// "No matching PRs" placeholder.
    pub fn filter_hid_everything(&self, scope: Option<&str>) -> bool {
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

    pub fn grouping(&self) -> Grouping {
        self.grouping
    }

    /// Advance to the next grouping mode and reset the selection to the first
    /// PR (its display row may have moved). The caller must `relayout` after to
    /// rebuild the rows; the new grouping is in effect immediately.
    pub fn cycle_grouping(&mut self) {
        self.grouping = self.grouping.next();
        self.selected = 0;
        self.scroll_offset = 0;
    }

    /// Reset the selection to the first visible PR and scroll to the top. Used
    /// on tab switches, where the spec resets to the top of the new tab rather
    /// than chasing the previously selected PR.
    pub fn select_first_visible(&mut self) {
        let first = self.visible_pr_indices().next().unwrap_or(0);
        self.selected = first;
        self.scroll_offset = 0;
        self.normalize_scroll();
    }

    /// Move the selection to the next PR row in display order, skipping headers.
    pub fn move_down(&mut self) {
        if let Some(next) = self.adjacent_pr(self.selected, Direction::Down) {
            self.selected = next;
        }
        self.normalize_scroll();
    }

    /// Move the selection to the previous PR row in display order.
    pub fn move_up(&mut self) {
        if let Some(prev) = self.adjacent_pr(self.selected, Direction::Up) {
            self.selected = prev;
        }
        self.normalize_scroll();
    }

    pub fn resize(&mut self, viewport_height: usize) {
        self.viewport_height = viewport_height;
        self.normalize_scroll();
    }

    /// PR index of the selection cursor. Read by tests and future features
    /// (e.g. opening the selected PR); the view itself reads the selected flag
    /// straight off `visible_rows`.
    #[allow(dead_code)]
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Index of the first display row inside the scroll window. Exposed for
    /// tests and future debug overlays; the view itself prefers `visible_rows`.
    #[allow(dead_code)]
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Number of rows currently allotted to the list (terminal height minus
    /// the status bar). Set via `resize`. Exposed for tests/inspection.
    #[allow(dead_code)]
    pub fn viewport_height(&self) -> usize {
        self.viewport_height
    }

    /// The display row index of the currently selected PR, or `None` when the
    /// list is empty. Used by `normalize_scroll` to keep the selection visible.
    fn selected_display_row(&self) -> Option<usize> {
        self.rows
            .iter()
            .position(|row| row == &DisplayRow::Pr(self.selected))
    }

    /// Re-clamp `scroll_offset` so the selected PR's display row stays on-screen
    /// with a ~10% margin above and below. Margin = `viewport_height / 10`,
    /// floor 1, so the selection never parks on the very top/bottom row when
    /// more rows are available in that direction. Operates over display rows, so
    /// headers count toward the window like any other row.
    fn normalize_scroll(&mut self) {
        if self.viewport_height == 0 || self.rows.is_empty() {
            return;
        }
        let Some(selected_row) = self.selected_display_row() else {
            return;
        };

        // Cap the margin at half the rows on each side. Without this, a tiny
        // viewport makes the top and bottom margins overlap and become jointly
        // unsatisfiable (e.g. at height 1, a floor-1 margin demands a row above
        // AND below the only visible line), and the selection ends up off-screen.
        let margin = (self.viewport_height / 10)
            .max(1)
            .min(self.viewport_height.saturating_sub(1) / 2);

        // Single-pass clamp against both constraints. The bottom constraint is
        // a lower bound on the offset, the top constraint an upper bound; with
        // the capped margin they can't conflict, so order doesn't matter.
        let min_offset = (selected_row + margin + 1).saturating_sub(self.viewport_height);
        let max_for_top = selected_row.saturating_sub(margin);
        if self.scroll_offset < min_offset {
            self.scroll_offset = min_offset;
        } else if self.scroll_offset > max_for_top {
            self.scroll_offset = max_for_top;
        }

        let max_offset = self.rows.len().saturating_sub(self.viewport_height);
        if self.scroll_offset > max_offset {
            self.scroll_offset = max_offset;
        }
    }

    pub fn prs(&self) -> &[PR] {
        &self.prs
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
        let start = self.scroll_offset.min(self.rows.len());
        let end = if self.viewport_height == 0 {
            self.rows.len()
        } else {
            (start + self.viewport_height).min(self.rows.len())
        };
        let selected = self.selected;
        self.rows[start..end]
            .iter()
            .map(move |row| (row, row == &DisplayRow::Pr(selected)))
    }

    /// Fetch phase for one Tracked Repo, or `None` when no fetch has been
    /// dispatched for it yet. Test-only; the view asks the scope-aware
    /// `is_loading` instead.
    #[cfg(test)]
    pub fn phase_of(&self, repo_slug: &str) -> Option<&Phase> {
        self.phases.get(repo_slug)
    }

    /// Whether a listing is still in flight for `scope`: a specific repo slug,
    /// or `None` meaning "any Tracked Repo" (the All tab).
    pub fn is_loading(&self, scope: Option<&str>) -> bool {
        match scope {
            Some(slug) => self.phases.get(slug) == Some(&Phase::Loading),
            None => self.phases.values().any(|p| *p == Phase::Loading),
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

    /// The PR index of the nearest selectable row in `direction` from `from`,
    /// scanning display rows so headers are skipped. `None` if there is no PR
    /// row in that direction (already at the first/last PR).
    fn adjacent_pr(&self, from: usize, direction: Direction) -> Option<usize> {
        let current_row = self.rows.iter().position(|r| r == &DisplayRow::Pr(from))?;
        let candidates: &mut dyn Iterator<Item = usize> = match direction {
            Direction::Down => &mut ((current_row + 1)..self.rows.len()),
            Direction::Up => &mut (0..current_row).rev(),
        };
        for row in candidates {
            if let DisplayRow::Pr(i) = self.rows[row] {
                return Some(i);
            }
        }
        None
    }
}

#[derive(Clone, Copy)]
enum Direction {
    Up,
    Down,
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::{DisplayRow, Grouping, PrList};
    use crate::blocker::Tier;
    use crate::github::rest::{PR, PRState};

    fn sample_pr(number: u64) -> PR {
        PR {
            number,
            repo_slug: "owner/repo".to_owned(),
            title: format!("PR #{number}"),
            author: "octocat".to_owned(),
            created_at: chrono::Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap(),
            updated_at: chrono::Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap(),
            additions: 0,
            deletions: 0,
            is_draft: false,
            labels: Vec::new(),
            requested_reviewers: Vec::new(),
            assignees: Vec::new(),
            review_decision: String::new(),
            mergeable: "UNKNOWN".to_owned(),
            last_commit_date: None,
            head_commit_sha: None,
            head_ref: format!("feature/{number}"),
            base_ref: "main".to_owned(),
            head_repository_owner: "mayfieldiv".to_owned(),
            state: PRState::Open,
        }
    }

    /// Build a list with `n` PRs, laid out flat (no grouping) so navigation and
    /// scroll tests exercise the row mechanics without headers in the way.
    fn flat_list(n: u64) -> PrList {
        let mut list = PrList::new();
        for i in 1..=n {
            list.push(sample_pr(i));
        }
        list.grouping = Grouping::None;
        list.relayout(None, |_| None);
        list
    }

    /// PR indices among the currently visible display rows.
    fn visible_pr_indices(list: &PrList) -> Vec<usize> {
        list.visible_rows()
            .filter_map(|(row, _)| match row {
                DisplayRow::Pr(i) => Some(*i),
                DisplayRow::Header(_) => None,
            })
            .collect()
    }

    #[test]
    fn pushed_pr_appears_in_the_list() {
        let mut list = PrList::new();

        list.push(sample_pr(42));

        assert_eq!(list.prs().len(), 1);
        assert_eq!(list.prs()[0].number, 42);
    }

    #[test]
    fn new_list_has_no_fetch_in_flight_and_no_failure() {
        let list = PrList::new();
        assert!(!list.is_loading(None));
        assert_eq!(list.failure(), None);
    }

    #[test]
    fn new_list_defaults_to_smart_status_grouping() {
        let list = PrList::new();
        assert_eq!(list.grouping(), Grouping::SmartStatus);
    }

    #[test]
    fn begin_fetch_marks_only_that_repo_loading() {
        let mut list = PrList::new();
        list.begin_fetch("acme/web");

        assert!(list.is_loading(None), "any-repo scope sees the fetch");
        assert!(list.is_loading(Some("acme/web")));
        assert!(
            !list.is_loading(Some("acme/api")),
            "an untouched repo is not loading"
        );
    }

    #[test]
    fn complete_fetch_clears_loading_for_that_repo_only() {
        let mut list = PrList::new();
        list.begin_fetch("acme/web");
        list.begin_fetch("acme/api");

        list.complete_fetch("acme/web");

        assert!(!list.is_loading(Some("acme/web")));
        assert!(list.is_loading(Some("acme/api")));
        assert!(list.is_loading(None), "another repo is still in flight");
        assert_eq!(list.phase_of("acme/web"), Some(&super::Phase::Loaded));
    }

    #[test]
    fn move_down_advances_selection_within_bounds() {
        let mut list = flat_list(3);

        list.move_down();
        assert_eq!(list.selected(), 1);
        list.move_down();
        list.move_down();
        list.move_down();
        // Last PR is index 2; further moves clamp.
        assert_eq!(list.selected(), 2);
    }

    #[test]
    fn move_up_retreats_selection_and_clamps_at_zero() {
        let mut list = flat_list(3);
        list.move_down();
        list.move_down();
        assert_eq!(list.selected(), 2);

        list.move_up();
        list.move_up();
        list.move_up();
        assert_eq!(list.selected(), 0);
    }

    #[test]
    fn navigation_skips_group_headers() {
        // Two tiers: me-blocking (PR #1) and waiting-on-author (PR #2).
        // Layout: [Header, Pr(0), Header, Pr(1)]. j must step Pr(0) -> Pr(1).
        let mut list = PrList::new();
        list.push(sample_pr(1));
        list.push(sample_pr(2));
        list.relayout(None, |pr| {
            Some(if pr.number == 1 {
                Tier::MeBlocking
            } else {
                Tier::WaitingOnAuthor
            })
        });

        assert_eq!(list.selected(), 0);
        list.move_down();
        assert_eq!(list.selected(), 1, "j steps over the second group's header");
        list.move_up();
        assert_eq!(list.selected(), 0, "k steps back over the header");
    }

    #[test]
    fn cycle_grouping_advances_mode_and_resets_selection() {
        let mut list = flat_list(3);
        list.move_down();
        list.move_down();
        assert_eq!(list.selected(), 2);

        // flat_list set grouping to None; cycling wraps None -> SmartStatus.
        list.cycle_grouping();
        assert_eq!(list.grouping(), Grouping::SmartStatus);
        assert_eq!(list.selected(), 0, "selection resets on regroup");
    }

    #[test]
    fn visible_rows_yields_window_starting_at_scroll_offset() {
        let mut list = flat_list(20);
        list.resize(5);
        for _ in 0..10 {
            list.move_down();
        }
        let offset = list.scroll_offset();

        let indices = visible_pr_indices(&list);

        assert_eq!(indices.len(), 5);
        // Flat layout: display row N is PR index N, so the first visible PR
        // index equals the scroll offset.
        assert_eq!(indices[0], offset);
        assert_eq!(indices[4], offset + 4);
    }

    #[test]
    fn visible_rows_caps_at_list_length_when_window_extends_past_end() {
        let mut list = flat_list(3);
        list.resize(10);

        let count = list.visible_rows().count();

        assert_eq!(
            count, 3,
            "viewport is larger than list; should yield all rows"
        );
    }

    #[test]
    fn moving_below_bottom_margin_advances_scroll() {
        let mut list = flat_list(20);
        list.resize(10);

        for _ in 0..9 {
            list.move_down();
        }

        assert!(
            list.scroll_offset() >= 1,
            "scroll should advance into the bottom margin, got {}",
            list.scroll_offset(),
        );
    }

    #[test]
    fn shrinking_viewport_re_clamps_scroll_to_keep_selection_visible() {
        let mut list = flat_list(30);
        list.resize(20);
        for _ in 0..25 {
            list.move_down();
        }
        let selected_row = list.selected(); // flat: row == index
        assert!(selected_row < list.scroll_offset() + 20);

        list.resize(5);

        assert!(
            list.selected() >= list.scroll_offset() && list.selected() < list.scroll_offset() + 5,
            "selection {} must stay within window {}..{} after shrink",
            list.selected(),
            list.scroll_offset(),
            list.scroll_offset() + 5,
        );
    }

    #[test]
    fn single_row_viewport_keeps_selection_visible() {
        let mut list = flat_list(10);
        list.resize(1);

        // At viewport_height = 1 the margin must collapse to 0, otherwise the
        // top and bottom margins are jointly unsatisfiable and the selected row
        // scrolls out of the single visible line.
        for _ in 0..5 {
            list.move_down();
        }

        // Flat layout: selected PR index == its display row.
        assert_eq!(
            list.scroll_offset(),
            list.selected(),
            "the only visible row must be the selected one",
        );
    }

    #[test]
    fn fail_fetch_records_failure_without_masking_other_repos() {
        let mut list = PrList::new();
        list.begin_fetch("acme/web");
        list.begin_fetch("acme/api");

        list.fail_fetch("acme/web", "network down".to_owned());

        assert_eq!(list.failure(), Some("network down"));
        assert!(
            list.is_loading(Some("acme/api")),
            "the other repo's fetch keeps going"
        );
        assert!(!list.is_loading(Some("acme/web")));
    }

    #[test]
    fn failure_reports_first_failed_repo_in_slug_order() {
        let mut list = PrList::new();
        list.fail_fetch("zeta/repo", "zeta down".to_owned());
        list.fail_fetch("acme/web", "acme down".to_owned());

        assert_eq!(
            list.failure(),
            Some("acme down"),
            "BTreeMap order makes the report deterministic"
        );
    }
}
