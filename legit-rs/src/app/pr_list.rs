//! Open PR List Module: PRs for the current Tracked Repo, plus the user's
//! selection cursor, scroll viewport, and fetch phase. Concentrates the
//! invariants that used to be spread across `Model` and `update.rs`.

use std::fmt;

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
    selected: usize,
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

    pub fn move_down(&mut self) {
        if self.prs.is_empty() {
            return;
        }
        let last = self.prs.len() - 1;
        if self.selected < last {
            self.selected += 1;
        }
        self.normalize_scroll();
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
        self.normalize_scroll();
    }

    pub fn resize(&mut self, viewport_height: usize) {
        self.viewport_height = viewport_height;
        self.normalize_scroll();
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Index of the first row inside the scroll window. Exposed for tests and
    /// future debug overlays; the view itself prefers `visible_rows()`.
    #[allow(dead_code)]
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Number of rows currently allotted to the list (terminal height minus
    /// the status bar). Set via `resize()`. Exposed for tests/inspection.
    #[allow(dead_code)]
    pub fn viewport_height(&self) -> usize {
        self.viewport_height
    }

    /// Re-clamp `scroll_offset` so `selected` stays on-screen with a ~10%
    /// margin above and below. Margin = `viewport_height / 10`, floor 1, so
    /// the selection never parks on the very top/bottom row when more PRs are
    /// available in that direction.
    fn normalize_scroll(&mut self) {
        if self.viewport_height == 0 || self.prs.is_empty() {
            return;
        }
        let margin = (self.viewport_height / 10).max(1);
        let visible_top = self.scroll_offset;
        let visible_bottom = self.scroll_offset.saturating_add(self.viewport_height);

        if self.selected + margin >= visible_bottom {
            self.scroll_offset = self.selected + margin + 1 - self.viewport_height;
        }
        if self.selected < visible_top + margin {
            self.scroll_offset = self.selected.saturating_sub(margin);
        }

        let max_offset = self.prs.len().saturating_sub(self.viewport_height);
        if self.scroll_offset > max_offset {
            self.scroll_offset = max_offset;
        }
    }

    pub fn prs(&self) -> &[PR] {
        &self.prs
    }

    /// Iterate the rows currently inside the scroll viewport, yielding the
    /// absolute PR index alongside each `&PR`. The view uses the index to
    /// detect which row is the selected one (`index == selected()`).
    pub fn visible_rows(&self) -> impl Iterator<Item = (usize, &PR)> {
        let start = self.scroll_offset.min(self.prs.len());
        let end = if self.viewport_height == 0 {
            self.prs.len()
        } else {
            (start + self.viewport_height).min(self.prs.len())
        };
        (start..end).map(move |i| (i, &self.prs[i]))
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
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::PrList;
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
        let mut list = PrList::new();
        for n in 1..=3 {
            list.push(sample_pr(n));
        }

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
        let mut list = PrList::new();
        for n in 1..=3 {
            list.push(sample_pr(n));
        }
        list.move_down();
        list.move_down();
        assert_eq!(list.selected(), 2);

        list.move_up();
        list.move_up();
        list.move_up();
        assert_eq!(list.selected(), 0);
    }

    #[test]
    fn visible_rows_yields_window_starting_at_scroll_offset() {
        let mut list = PrList::new();
        for n in 1..=20 {
            list.push(sample_pr(n));
        }
        list.resize(5);
        // Scroll the window down a few rows.
        for _ in 0..10 {
            list.move_down();
        }
        let offset = list.scroll_offset();

        let rows: Vec<(usize, u64)> = list.visible_rows().map(|(i, pr)| (i, pr.number)).collect();

        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].0, offset);
        // PR numbers are 1-indexed; visible row at PR index N has PR number N+1.
        assert_eq!(rows[0].1, (offset as u64) + 1);
        assert_eq!(rows[4].1, (offset as u64) + 5);
    }

    #[test]
    fn visible_rows_caps_at_list_length_when_window_extends_past_end() {
        let mut list = PrList::new();
        for n in 1..=3 {
            list.push(sample_pr(n));
        }
        list.resize(10);

        let count = list.visible_rows().count();

        assert_eq!(
            count, 3,
            "viewport is larger than list; should yield all PRs"
        );
    }

    #[test]
    fn moving_below_bottom_margin_advances_scroll() {
        let mut list = PrList::new();
        for n in 1..=20 {
            list.push(sample_pr(n));
        }
        list.resize(10);

        // Push selection toward the bottom; ~10% margin means at viewport=10
        // we keep at least one row of lead, so selection can't sit on the
        // very last visible row.
        for _ in 0..9 {
            list.move_down();
        }

        assert!(
            list.scroll_offset() >= 1,
            "scroll should advance into the bottom margin, got {}",
            list.scroll_offset(),
        );
        assert!(list.selected() >= list.scroll_offset());
        assert!(list.selected() < list.scroll_offset() + 10);
    }

    #[test]
    fn shrinking_viewport_re_clamps_scroll_to_keep_selection_visible() {
        let mut list = PrList::new();
        for n in 1..=30 {
            list.push(sample_pr(n));
        }
        list.resize(20);
        for _ in 0..25 {
            list.move_down();
        }
        let prev_offset = list.scroll_offset();
        assert!(list.selected() < prev_offset + 20);

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
    fn fail_fetch_records_failure_message() {
        let mut list = PrList::new();
        list.begin_fetch();
        list.fail_fetch("network down".to_owned());

        assert!(matches!(list.phase(), super::Phase::Failed(_)));
        assert_eq!(list.failure(), Some("network down"));
    }
}
