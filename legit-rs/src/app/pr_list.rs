//! Open PR List Module: PRs for the current Tracked Repo, plus the user's
//! selection cursor, scroll viewport, fetch phase, and how the list is grouped.
//! Concentrates the invariants that used to be spread across `Model` and
//! `update.rs`.
//!
//! Grouping turns the flat PR vec into a `Vec<DisplayRow>` (headers + PR rows).
//! Selection tracks a *PR index* (so it survives regrouping), while scrolling
//! works over display rows (headers included). `j`/`k` step PR-to-PR, skipping
//! header rows; the scroll viewport keeps the selected PR's row on-screen.

use std::fmt;

use crate::app::grouping::{DisplayRow, Grouping, display_rows};
use crate::blocker::Tier;
use crate::github::rest::PR;

/// Lifecycle of the open-PR fetch. At most one variant holds at a time, so the
/// view never has to ask "are we loading AND failed?".
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Phase {
    /// No fetch has been dispatched yet.
    #[default]
    Idle,
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
    phase: Phase,
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
    /// Renders the list as `{ phase, prs: <len>, ... }` — the full PR vec is
    /// noisy in `tracing` output and rarely informative compared to its length.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrList")
            .field("phase", &self.phase)
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

    pub fn begin_fetch(&mut self) {
        self.phase = Phase::Loading;
    }

    pub fn complete_fetch(&mut self) {
        self.phase = Phase::Loaded;
    }

    pub fn fail_fetch(&mut self, message: String) {
        self.phase = Phase::Failed(message);
    }

    pub fn push(&mut self, pr: PR) {
        self.prs.push(pr);
    }

    /// Rebuild the display layout from the current PRs under the active
    /// grouping. `tier_of(pr_index)` returns the Smart-status tier for a PR, or
    /// `None` when its enrichment hasn't been derived yet. `repo_slug` is the
    /// slug used for repo grouping. Re-clamps scroll so the selection stays
    /// on-screen. Called by `update` after PRs arrive, enrichment lands, or the
    /// grouping changes.
    pub fn relayout(&mut self, tier_of: impl Fn(usize) -> Option<Tier>, repo_slug: &str) {
        self.rows = display_rows(&self.prs, self.grouping, tier_of, repo_slug);
        if self.selected >= self.prs.len() {
            self.selected = self.prs.len().saturating_sub(1);
        }
        self.normalize_scroll();
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

    /// Mutable access to a streamed PR by number, for enrichment that overwrites
    /// list fields in place (mergeable, review decision, size, head SHA). `None`
    /// if no PR with that number is in the list.
    pub fn pr_mut(&mut self, number: u64) -> Option<&mut PR> {
        self.prs.iter_mut().find(|pr| pr.number == number)
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

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn failure(&self) -> Option<&str> {
        match &self.phase {
            Phase::Failed(message) => Some(message),
            _ => None,
        }
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
        list.relayout(|_| None, "owner/repo");
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
    fn new_list_starts_idle() {
        let list = PrList::new();
        assert!(matches!(list.phase(), super::Phase::Idle));
    }

    #[test]
    fn new_list_defaults_to_smart_status_grouping() {
        let list = PrList::new();
        assert_eq!(list.grouping(), Grouping::SmartStatus);
    }

    #[test]
    fn begin_fetch_transitions_to_loading() {
        let mut list = PrList::new();
        list.begin_fetch();
        assert!(matches!(list.phase(), super::Phase::Loading));
    }

    #[test]
    fn complete_fetch_transitions_to_loaded() {
        let mut list = PrList::new();
        list.begin_fetch();
        list.complete_fetch();
        assert!(matches!(list.phase(), super::Phase::Loaded));
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
        // Two tiers: me-blocking (index 0) and waiting-on-author (index 1).
        // Layout: [Header, Pr(0), Header, Pr(1)]. j must step Pr(0) -> Pr(1).
        let mut list = PrList::new();
        list.push(sample_pr(1));
        list.push(sample_pr(2));
        list.relayout(
            |i| {
                Some(if i == 0 {
                    Tier::MeBlocking
                } else {
                    Tier::WaitingOnAuthor
                })
            },
            "owner/repo",
        );

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
    fn fail_fetch_records_failure_message() {
        let mut list = PrList::new();
        list.begin_fetch();
        list.fail_fetch("network down".to_owned());

        assert!(matches!(list.phase(), super::Phase::Failed(_)));
        assert_eq!(list.failure(), Some("network down"));
    }
}
