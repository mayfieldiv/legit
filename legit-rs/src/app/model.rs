use std::collections::HashMap;

use crate::{
    blocker::{BlockerOptions, BlockerResult, compute_blocker},
    config::LegitConfig,
    git_remote::RepoInfo,
    github::limiter::NetworkStats,
    github::rest::PrKey,
    github::types::{CheckRun, FullReviewThread, IssueComment, Review},
    secret::Secret,
};

use super::{cmd::Cmd, pr_list::PrList};

/// Per-PR enrichment landed by the GraphQL/REST fan-out. Keyed by `PrKey`
/// (slug + number — numbers collide across repos), except `checks` which is
/// keyed by head commit SHA (checks belong to a commit and are shared across
/// PRs that point at it). Written here in M3; the blocker engine, summary
/// panel, and detail view consume these in later milestones.
#[derive(Clone, Debug, Default)]
pub struct Enrichment {
    pub review_threads: HashMap<PrKey, Vec<FullReviewThread>>,
    pub reviews: HashMap<PrKey, Vec<Review>>,
    pub issue_comments: HashMap<PrKey, Vec<IssueComment>>,
    pub checks: HashMap<String, Vec<CheckRun>>,
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
    /// halts the list instead).
    pub config_loaded: bool,
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
    /// Transient status message + a generation counter. A scheduled clear only
    /// fires if its token still matches `status_gen`, so a newer message is
    /// never wiped by an older message's timer.
    pub status: Option<StatusMessage>,
    pub status_gen: u64,
    pub network_stats: NetworkStats,
    pub enrichment: Enrichment,
    /// Per-PR Smart-status, derived from `enrichment` + the current user and
    /// cached so the list view and grouping read it without recomputing on
    /// every frame. Keyed by `PrKey`; recomputed by `refresh_blockers`
    /// whenever a PR arrives or its enrichment lands. A PR absent from the map
    /// hasn't been derived yet (it groups under "Loading details…").
    pub blockers: HashMap<PrKey, BlockerResult>,
}

impl Model {
    pub fn new() -> (Self, Vec<Cmd>) {
        (
            Self {
                should_quit: false,
                config: LegitConfig::default(),
                config_loaded: false,
                auth_token: None,
                repo: RepoDetection::Pending,
                list: PrList::new(),
                active_tab: 0,
                terminal_height: 0,
                status: None,
                status_gen: 0,
                network_stats: NetworkStats::default(),
                enrichment: Enrichment::default(),
                blockers: HashMap::new(),
            },
            vec![Cmd::LoadConfig, Cmd::ResolveAuthToken, Cmd::DetectRepo],
        )
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
    /// `ConfigLoadFailed` halts the list before that, so it is unreachable.
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
    /// bar and status bar, plus the filter chip while it's visible. The single
    /// source of truth shared by `sync_viewport` (which subtracts it from the
    /// terminal height) and `view::view` (which lays out exactly this many
    /// fixed rows around the list).
    pub fn chrome_rows(&self) -> usize {
        2 + usize::from(self.list.filter().is_visible())
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
        let checks = pr
            .head_commit_sha
            .as_ref()
            .and_then(|sha| self.enrichment.checks.get(sha))
            .map(Vec::as_slice)
            .unwrap_or(&[]);
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
