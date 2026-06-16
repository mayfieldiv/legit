use std::collections::{HashMap, HashSet};

use ratatui::text::Line;

use crate::{
    blocker::{BlockerOptions, BlockerResult, compute_blocker},
    config::LegitConfig,
    file_category::FileCategorization,
    git_remote::RepoInfo,
    github::limiter::NetworkStats,
    github::rest::{PR, PrKey},
    github::types::{CheckRun, FullReviewThread, IssueComment, Review},
    secret::Secret,
    worktree::{self, WorktreeEntry},
};

use super::{
    cmd::Cmd,
    detail_items::{DetailFilters, DetailFocus},
    pr_list::PrList,
};

/// Which top-level view is active. `List` is the default PR list; `Detail`
/// carries the whole detail-view state in one variant so illegal combinations
/// (a body or scroll offset with no open detail; a scroll offset that outlives
/// its PR) are unrepresentable — mirroring the TS `detail-state.ts` union.
#[derive(Clone, Debug, PartialEq)]
pub enum ViewMode {
    /// The open PR list with the summary panel.
    List,
    /// The PR detail view, holding the open PR's key, fetched body, and scroll
    /// offset together. Entering `Detail` constructs a fresh `DetailState`;
    /// leaving it is a single assignment back to `List`, so there is no
    /// hand-synchronised side state to clear.
    Detail(DetailState),
}

/// The complete state of an open detail view. Bundled into `ViewMode::Detail`
/// so the body and scroll offset can only exist while a detail view is open and
/// always belong to `key`'s PR.
#[derive(Clone, Debug, PartialEq)]
pub struct DetailState {
    /// Identity of the PR being shown (`repo_slug` + number). The `PR` itself is
    /// sourced from the enriched `model.list` via this key so mergeable,
    /// head_commit_sha, etc. are always current.
    pub key: PrKey,
    /// The fetched PR description, rendered to display lines exactly once on
    /// arrival (`Msg::PRDetailArrived`) rather than re-parsed every frame.
    /// `None` while `Cmd::FetchPRDetail` is in flight — the detail view shows a
    /// loading placeholder in that state. Holds only the description lines; the
    /// CI checks section is appended per-frame so late-arriving checks still
    /// show without a re-fetch.
    pub body: Option<Vec<Line<'static>>>,
    /// Vertical scroll offset for the body (lines scrolled past the top). Starts
    /// at zero on entry; `update` mutates it on PageUp/PageDown (and to keep the
    /// focused item visible) and clamps it so it can never sit past the last
    /// screenful. `usize` like every other line count, so the clamp math is
    /// exact however tall the content; the one narrowing to ratatui's `u16`
    /// happens at the render edge.
    pub scroll: usize,
    /// The focused Focus Sequence item, identity-keyed by comment URL (the
    /// same stable key `expanded` uses) with its last-resolved index.
    /// `j`/`k`/arrows move it; every update re-anchors it against the fresh
    /// sequence, so arrivals or filter toggles that insert/remove items above
    /// it move the index — never which card is focused. A vanished item falls
    /// back to its last position.
    pub focus: DetailFocus,
    /// What the last `normalize_detail` pass resolved the focus to: its
    /// identity plus the first line of its card in the measured layout.
    /// `None` until a pass has measured an arrived body. The change detector
    /// behind scroll-follows-focus: a pass whose resolved identity or card
    /// start differs scrolls the card back into view, so focus moves, filter
    /// toggles, and content arriving above the card all follow it — while raw
    /// PageUp/PageDown scrolling (which changes neither) never does. Enter
    /// clears it to force a follow after expanding the focused card in place.
    pub followed: Option<(DetailFocus, usize)>,
    /// Cards whose long bodies the user expanded with Enter, keyed by the
    /// comment's URL (unique per review/issue comment, and stable across
    /// filter toggles — unlike a focus index). Lives in `DetailState` so
    /// closing the view structurally resets every card to collapsed, mirroring
    /// the TS details-store being dropped per PR.
    pub expanded: HashSet<String>,
}

/// Per-PR enrichment landed by the GraphQL/REST fan-out. Keyed by `PrKey`
/// (slug + number — numbers collide across repos), except `checks` which is
/// keyed by (repo slug, head commit SHA): check runs are repo-scoped on
/// GitHub — a fork PR shares its head SHA with upstream but not its check
/// runs — while still being shared across same-repo PRs that point at the
/// same commit. Written here in M3; the blocker engine, summary panel, and
/// detail view consume these in later milestones.
#[derive(Clone, Debug, Default)]
pub struct Enrichment {
    pub review_threads: HashMap<PrKey, Vec<FullReviewThread>>,
    pub reviews: HashMap<PrKey, Vec<Review>>,
    pub issue_comments: HashMap<PrKey, Vec<IssueComment>>,
    pub checks: HashMap<(String, String), Vec<CheckRun>>,
    /// Comment bodies rendered to display lines once on arrival, keyed by the
    /// comment's URL (the same stable key `DetailState::expanded` uses).
    /// Private: written only by `store_threads`/`store_issue_comments`, so the
    /// cache stays adjacent to the raw maps and can't drift from them. Read by
    /// `detail_layout` via `rendered_body`, which would otherwise re-parse
    /// every comment's markdown each frame — the same rationale as caching the
    /// rendered description in `DetailState::body`. Entries for comments that
    /// vanish on a refresh linger (bounded by the session) and are overwritten
    /// whenever their URL re-arrives.
    rendered_bodies: HashMap<String, Vec<Line<'static>>>,
    /// Per-PR changed-files fetch state, fetched just-in-time on selection
    /// change. Keyed by `PrKey`; absent until the PR's files are first
    /// requested. This one map IS the files fetch's whole state machine
    /// (`absent` -> `Requested` -> `Loaded`), so a future Refresh invalidates a
    /// PR's files by removing its single entry here (no second collection to
    /// keep in sync). Consumed by the summary panel's File Category breakdown.
    pub files: HashMap<PrKey, FilesState>,
}

impl Enrichment {
    /// Store an arrived thread list, rendering each comment's markdown body to
    /// display lines exactly once. The one write path for `review_threads`, so
    /// the rendered cache always covers what the maps hold.
    pub fn store_threads(&mut self, pr: PrKey, threads: Vec<FullReviewThread>) {
        for comment in threads.iter().flat_map(|thread| &thread.comments) {
            self.rendered_bodies
                .insert(comment.url.clone(), crate::markdown::render(&comment.body));
        }
        self.review_threads.insert(pr, threads);
    }

    /// Store an arrived issue-comment list; same render-once contract as
    /// `store_threads`.
    pub fn store_issue_comments(&mut self, pr: PrKey, comments: Vec<IssueComment>) {
        for comment in &comments {
            self.rendered_bodies
                .insert(comment.url.clone(), crate::markdown::render(&comment.body));
        }
        self.issue_comments.insert(pr, comments);
    }

    /// The display lines for a comment's body, cloned from the render-once
    /// cache. Falls back to rendering fresh for a comment that bypassed the
    /// `store_*` writers — behaviourally identical, just uncached.
    pub fn rendered_body(&self, url: &str, body: &str) -> Vec<Line<'static>> {
        self.rendered_bodies
            .get(url)
            .cloned()
            .unwrap_or_else(|| crate::markdown::render(body))
    }

    /// The review threads fetched for `pr`, or `None` until `ThreadsArrived`.
    /// Callers that don't care about arrival (focus math) `unwrap_or(&[])`;
    /// the detail view distinguishes `None` (loading placeholder) from empty.
    pub fn threads_for(&self, pr: &PrKey) -> Option<&[FullReviewThread]> {
        self.review_threads.get(pr).map(Vec::as_slice)
    }

    /// The issue comments fetched for `pr`, or `None` until
    /// `IssueCommentsArrived`. Same `None`-vs-empty contract as `threads_for`.
    pub fn comments_for(&self, pr: &PrKey) -> Option<&[IssueComment]> {
        self.issue_comments.get(pr).map(Vec::as_slice)
    }

    /// The check runs fetched for `pr`'s head commit, or `None` until they
    /// arrive. The `checks` map is keyed by (repo slug, head SHA) — not `PrKey`
    /// — because check runs are repo-scoped on GitHub (a fork PR shares its
    /// head SHA with upstream but not its check runs). This is the single place
    /// that builds that key from a `PR`, so the blocker engine and the summary
    /// panel resolve checks identically. A PR with no head SHA yet (no commits,
    /// or review-status hasn't reported it) has no checks.
    pub fn checks_for(&self, pr: &PR) -> Option<&[CheckRun]> {
        let sha = pr.head_commit_sha.as_ref()?;
        self.checks
            .get(&(pr.repo_slug.clone(), sha.clone()))
            .map(Vec::as_slice)
    }
}

/// The three-state machine for one PR's changed-files fetch, collapsed into a
/// single map entry in `Enrichment::files`. An absent key is the third state
/// ("never requested"). Modelled as an enum over conflated parallel
/// collections for the same reason as `RepoDetection`: the states are mutually
/// exclusive, so one value that can only be in one of them at a time can't
/// drift out of sync.
#[derive(Clone, Debug)]
pub enum FilesState {
    /// `Cmd::FetchFiles` dispatched; the dedupe guard so scrolling back to a PR
    /// — or a flurry of `j` presses — never refetches. The summary panel renders
    /// its Loading placeholder, same as a never-requested PR. Cleared by
    /// `Msg::FilesFetchFailed` (removing the entry) so a transient error lets
    /// re-selecting the PR retry.
    Requested,
    /// Files arrived and were categorised against the config `file_rules`. The
    /// summary panel renders the File Category breakdown.
    Loaded(FileCategorization),
}

/// Severity of a transient status-bar message. Drives both styling and how long
/// the message lingers before auto-clearing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusKind {
    /// Persists until replaced. Produced by later milestones (e.g. in-flight
    /// operation notices); the view renders it today.
    #[allow(dead_code)]
    Info,
    /// Auto-clears after 4s. Produced by later milestones (e.g. worktree-created
    /// confirmations); the view renders it today.
    #[allow(dead_code)]
    Success,
    /// Auto-clears after 8s. Used now for command and enrichment failures.
    Error,
}

/// A transient message shown on the right of the status bar. `Success` clears
/// after 4s and `Error` after 8s (scheduled by `update`); `Info` persists until
/// replaced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusMessage {
    pub kind: StatusKind,
    pub text: String,
}

/// Outcome of CWD repo detection (`Cmd::DetectRepo`). An explicit tri-state so
/// the PR-fetch gate can tell "detection hasn't reported yet" apart from
/// "detection reported, but there's no repo here". Conflating the two as
/// `Option<RepoInfo>` (`None` = both) would wedge the app at an empty list
/// whenever detection fails (outside a git repo / no GitHub remote): the gate
/// would wait forever for a `Detected` that never comes, never fetching even
/// the configured Tracked Repos.
#[derive(Clone, Debug)]
pub enum RepoDetection {
    /// `Cmd::DetectRepo` is still in flight; the gate waits.
    Pending,
    /// Detection found a GitHub repo in the CWD — added to the tracked set.
    Detected(RepoInfo),
    /// Detection ran but found no repo (not a git repo / no GitHub remote).
    /// The gate proceeds on configured repos alone; nothing is added to the
    /// tracked set.
    Failed,
}

impl RepoDetection {
    /// True once detection has reported either way — the PR-fetch gate keys
    /// off this rather than `Detected` alone, so a failed detection still lets
    /// configured Tracked Repos fetch.
    pub fn is_settled(&self) -> bool {
        !matches!(self, RepoDetection::Pending)
    }

    /// The detected CWD repo, or `None` while pending or after a failure.
    pub fn repo(&self) -> Option<&RepoInfo> {
        match self {
            RepoDetection::Detected(repo) => Some(repo),
            RepoDetection::Pending | RepoDetection::Failed => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Model {
    pub should_quit: bool,
    pub config: LegitConfig,
    /// True once config load has settled successfully (`Cmd::LoadConfig` ->
    /// `Msg::ConfigLoaded`). The PR fetch waits on this so blockers are never
    /// derived with a default `user`/`bot_logins` that lost the startup race
    /// against enrichment. A malformed config never sets it (`ConfigLoadFailed`
    /// records a `fatal` error instead).
    pub config_loaded: bool,
    /// An app-level fatal error that blocks every fetch — today only a malformed
    /// config (`Msg::ConfigLoadFailed`). Distinct from a `PrList` per-repo
    /// failure: it is the whole app's prerequisite that failed, not one repo's
    /// listing. The status bar surfaces it ahead of any list failure.
    pub fatal: Option<String>,
    pub auth_token: Option<Secret<String>>,
    /// CWD repo detection state. The PR-fetch gate waits for this to settle
    /// (`Detected` or `Failed`), not for `Detected` specifically, so a failed
    /// detection doesn't permanently block configured Tracked Repos.
    pub repo: RepoDetection,
    pub list: PrList,
    /// Active Repo Tab index: 0 is the All tab, `i >= 1` is `tracked_repos()[i-1]`.
    /// Clamped at read time by `active_scope` (the tracked set only ever grows,
    /// and only until config + repo detection settle).
    pub active_tab: usize,
    /// Last reported terminal height, kept so `sync_viewport` can re-derive the
    /// list viewport when chrome rows (tab bar, filter chip) appear or vanish
    /// without a resize event.
    pub terminal_height: u16,
    /// Last reported terminal width, kept so `update` can measure the detail
    /// body via the same `detail_content` layout the view renders (the runtime
    /// seeds both dimensions with a synthetic initial Resize).
    pub terminal_width: u16,
    /// Transient status message + a generation counter. A scheduled clear only
    /// fires if its token still matches `status_gen`, so a newer message is
    /// never wiped by an older message's timer.
    pub status: Option<StatusMessage>,
    pub status_gen: u64,
    pub network_stats: NetworkStats,
    pub enrichment: Enrichment,
    /// Latest `git worktree list --porcelain` entries per Tracked Repo whose
    /// config has a source clone. Filled at startup and after worktree
    /// creation; summary/detail views derive per-PR matches from this cache.
    pub worktrees_by_repo: HashMap<String, Vec<WorktreeEntry>>,
    /// Per-PR Smart-status, derived from `enrichment` + the current user and
    /// cached so the list view and grouping read it without recomputing on
    /// every frame. Keyed by `PrKey`; recomputed by `refresh_blockers`
    /// whenever a PR arrives or its enrichment lands. A PR absent from the map
    /// hasn't been derived yet (it groups under "Loading details…").
    pub blockers: HashMap<PrKey, BlockerResult>,
    /// Which top-level view is active. Starts at `List`; transitions to
    /// `Detail(DetailState)` when Enter is pressed on a PR (the body and scroll
    /// offset live inside that variant). `Esc` in the detail view returns to
    /// `List`.
    pub view_mode: ViewMode,
    /// Detail-view filter: show resolved threads (`t` toggles; default false).
    /// Lives on the `Model`, not `DetailState`, so the preference survives
    /// closing and reopening detail views (mirrors the TS app-level ui-state).
    pub show_resolved: bool,
    /// Detail-view filter: show bot comments (`b` toggles; default true).
    /// Model-level for the same reason as `show_resolved`.
    pub show_bot_comments: bool,
}

impl Model {
    pub fn new() -> (Self, Vec<Cmd>) {
        (
            Self {
                should_quit: false,
                config: LegitConfig::default(),
                config_loaded: false,
                fatal: None,
                auth_token: None,
                repo: RepoDetection::Pending,
                list: PrList::new(),
                active_tab: 0,
                terminal_height: 0,
                terminal_width: 0,
                status: None,
                status_gen: 0,
                network_stats: NetworkStats::default(),
                enrichment: Enrichment::default(),
                worktrees_by_repo: HashMap::new(),
                blockers: HashMap::new(),
                view_mode: ViewMode::List,
                show_resolved: false,
                show_bot_comments: true,
            },
            vec![Cmd::LoadConfig, Cmd::ResolveAuthToken, Cmd::DetectRepo],
        )
    }

    /// The detail view's comment-visibility filters, bundled for the
    /// `detail_items` derivation shared by `update` and `detail_layout`.
    pub fn detail_filters(&self) -> DetailFilters {
        DetailFilters {
            show_resolved: self.show_resolved,
            show_bot_comments: self.show_bot_comments,
        }
    }

    /// The current user's login, from config (`~/.legit/config.json` `user`).
    /// Empty when unset — the blocker engine treats an empty current user as
    /// "no one is me", so nothing is ever me-blocking until it's configured.
    pub fn current_user(&self) -> &str {
        &self.config.user
    }

    /// Every Tracked Repo: the configured repos in config order, then the
    /// CWD-detected repo appended when it isn't already configured. Deduped
    /// case-insensitively comparing `.slug()` (GitHub slugs are
    /// case-insensitive); the first occurrence's casing wins, so fetches,
    /// `PR::repo_slug` stamps, and tab labels all share one canonical string per
    /// repo.
    ///
    /// This is the ONE site that turns config `repos` slugs into `RepoInfo`, so
    /// it is where the validated-at-load invariant is leaned on: a config slug
    /// that `RepoInfo::from_slug` can't parse is silently dropped, which only
    /// happens if a malformed slug slipped past `config::validate_repo_slug` —
    /// `ConfigLoadFailed` records a `fatal` error and blocks the fetch before
    /// that, so it is unreachable.
    pub fn tracked_repos(&self) -> Vec<RepoInfo> {
        let mut repos: Vec<RepoInfo> = Vec::new();
        let push_unique = |repo: RepoInfo, repos: &mut Vec<RepoInfo>| {
            let slug = repo.slug();
            if !repos.iter().any(|r| r.slug().eq_ignore_ascii_case(&slug)) {
                repos.push(repo);
            }
        };
        for repo in &self.config.repos {
            if let Some(info) = RepoInfo::from_slug(&repo.slug) {
                push_unique(info, &mut repos);
            }
        }
        if let Some(repo) = self.repo.repo() {
            push_unique(repo.clone(), &mut repos);
        }
        repos
    }

    /// The `RepoInfo` for a Tracked Repo slug, or `None` when no tracked repo
    /// matches (e.g. a PR whose `repo_slug` is no longer configured). The single
    /// place enrichment/check fan-out resolves a slug back to a `RepoInfo`, so
    /// the validated-at-load invariant is leaned on only in `tracked_repos`.
    pub fn tracked_repo(&self, slug: &str) -> Option<RepoInfo> {
        self.tracked_repos()
            .into_iter()
            .find(|repo| repo.slug() == slug)
    }

    /// The PR the user is focused on for fetch prioritisation: the open detail
    /// PR, else the selected list PR. The runtime pushes this to the network
    /// limiter so the focused PR's pending fetches are granted ahead of the
    /// background fan-out.
    pub fn focused_pr_key(&self) -> Option<PrKey> {
        match &self.view_mode {
            ViewMode::Detail(detail) => Some(detail.key.clone()),
            ViewMode::List => self.list.selected_pr().map(PR::key),
        }
    }

    /// The worktree currently attached to `pr`, matched by expected branch
    /// first and deterministic legit path second.
    pub fn worktree_for_pr(&self, pr: &PR) -> Option<&WorktreeEntry> {
        let repo = self.tracked_repo(&pr.repo_slug)?;
        let entries = self.worktrees_by_repo.get(&pr.repo_slug)?;
        let expected_branch = worktree::expected_branch_for_pr(pr, &repo.owner);
        let expected_path =
            worktree::resolve_worktree_path(&self.config, &pr.repo_slug, pr.number, &pr.head_ref)
                .ok()?;
        let expected_path = expected_path.to_string_lossy();
        worktree::match_worktree(entries, &expected_branch, &expected_path)
    }

    /// The repo slug the active tab narrows the list to, or `None` for the All
    /// tab. An out-of-range `active_tab` clamps to All rather than panicking.
    pub fn active_scope(&self) -> Option<String> {
        if self.active_tab == 0 {
            return None;
        }
        self.tracked_repos()
            .into_iter()
            .nth(self.active_tab - 1)
            .map(|repo| repo.slug())
    }

    /// Number of non-list "chrome" rows around the list: the always-present tab
    /// bar and status bar, plus the filter chip while it's visible. Defined by
    /// `list_layout` — the canonical list-view geometry — so the viewport
    /// `sync_viewport` sizes, the rows `view::view` lays out, and the rows
    /// mouse hit-testing maps can't disagree.
    pub fn chrome_rows(&self) -> usize {
        super::list_layout::chrome_rows(self.list.filter().is_visible())
    }

    /// Re-derive the list viewport from the terminal height minus the chrome
    /// rows (tab bar + status bar, plus the filter chip while visible). Called
    /// on terminal resize — and whenever a chrome row appears or vanishes
    /// without one (opening/closing the filter).
    pub fn sync_viewport(&mut self) {
        self.list
            .resize((self.terminal_height as usize).saturating_sub(self.chrome_rows()));
    }

    /// Recompute the cached blocker result for one PR from whatever enrichment
    /// has arrived. A PR is only classified once both its threads and reviews
    /// are present (matching the TS `loading` gate); until then it stays absent
    /// from the cache and groups under "Loading details…".
    fn refresh_blocker(&mut self, index: usize) {
        let Some(pr) = self.list.prs().get(index) else {
            return;
        };
        let key = pr.key();
        let (Some(threads), Some(reviews)) = (
            self.enrichment.review_threads.get(&key),
            self.enrichment.reviews.get(&key),
        ) else {
            return;
        };
        let checks = self.enrichment.checks_for(pr).unwrap_or(&[]);
        let result = compute_blocker(
            pr,
            self.current_user(),
            &BlockerOptions {
                checks,
                reviews,
                threads: Some(threads),
            },
        );
        self.blockers.insert(key, result);
    }

    /// Recompute every cached blocker result, then rebuild the list layout. Used
    /// after any change that can affect tiers (enrichment arrival, a PR's
    /// fields changing, a fresh stream). Keeps the cache and the rendered groups
    /// in lockstep.
    pub fn refresh_blockers(&mut self) {
        for index in 0..self.list.prs().len() {
            self.refresh_blocker(index);
        }
        self.relayout();
    }

    /// Rebuild the list's display layout from the current PRs, cached tiers,
    /// grouping, and active Repo Tab. Cheap; safe to call after
    /// selection/grouping/tab changes too.
    pub fn relayout(&mut self) {
        let scope = self.active_scope();
        // `blockers` is a field disjoint from `self.list`, so it can be borrowed
        // by the tier closure while `self.list` is borrowed mutably.
        let blockers = &self.blockers;
        self.list.relayout(scope.as_deref(), |pr| {
            blockers.get(&pr.key()).map(|b| b.tier)
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::{app::model::Model, secret::Secret};

    #[test]
    fn debug_redacts_auth_token() {
        let (mut model, _) = Model::new();
        model.auth_token = Some(Secret::new("secret-token".to_owned()));

        let debug = format!("{model:?}");

        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("secret-token"));
    }
}
