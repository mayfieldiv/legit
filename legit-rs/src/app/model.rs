use std::collections::HashMap;

use crate::{
    blocker::{BlockerOptions, BlockerResult, Tier, compute_blocker},
    config::LegitConfig,
    git_remote::RepoInfo,
    github::limiter::NetworkStats,
    github::types::{CheckRun, FullReviewThread, IssueComment, Review},
    secret::Secret,
};

use super::{cmd::Cmd, pr_list::PrList};

/// Per-PR enrichment landed by the GraphQL/REST fan-out. Keyed by PR number,
/// except `checks` which is keyed by head commit SHA (checks belong to a commit
/// and are shared across PRs that point at it). Written here in M3; the blocker
/// engine, summary panel, and detail view consume these in later milestones.
#[derive(Clone, Debug, Default)]
pub struct Enrichment {
    pub review_threads: HashMap<u64, Vec<FullReviewThread>>,
    pub reviews: HashMap<u64, Vec<Review>>,
    pub issue_comments: HashMap<u64, Vec<IssueComment>>,
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

#[derive(Clone, Debug)]
pub struct Model {
    pub should_quit: bool,
    pub config: LegitConfig,
    pub auth_token: Option<Secret<String>>,
    pub repo: Option<RepoInfo>,
    pub list: PrList,
    /// Transient status message + a generation counter. A scheduled clear only
    /// fires if its token still matches `status_gen`, so a newer message is
    /// never wiped by an older message's timer.
    pub status: Option<StatusMessage>,
    pub status_gen: u64,
    pub network_stats: NetworkStats,
    pub enrichment: Enrichment,
    /// Per-PR Smart-status, derived from `enrichment` + the current user and
    /// cached so the list view and grouping read it without recomputing on
    /// every frame. Keyed by PR number; recomputed by `refresh_blockers`
    /// whenever a PR arrives or its enrichment lands. A PR absent from the map
    /// hasn't been derived yet (it groups under "Loading details…").
    pub blockers: HashMap<u64, BlockerResult>,
}

impl Model {
    pub fn new() -> (Self, Vec<Cmd>) {
        (
            Self {
                should_quit: false,
                config: LegitConfig::default(),
                auth_token: None,
                repo: None,
                list: PrList::new(),
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

    /// `owner/repo` slug for the detected repo, or empty when none is detected
    /// yet. Used as the repo-grouping label (single-repo today).
    pub fn repo_slug(&self) -> String {
        match &self.repo {
            Some(repo) => format!("{}/{}", repo.owner, repo.repo),
            None => String::new(),
        }
    }

    /// Smart-status tier for the PR at `index` in the list, or `None` when its
    /// blocker hasn't been derived yet (enrichment still pending).
    fn tier_of(&self, index: usize) -> Option<Tier> {
        let pr = self.list.prs().get(index)?;
        self.blockers.get(&pr.number).map(|b| b.tier)
    }

    /// Recompute the cached blocker result for one PR from whatever enrichment
    /// has arrived. A PR is only classified once both its threads and reviews
    /// are present (matching the TS `loading` gate); until then it stays absent
    /// from the cache and groups under "Loading details…".
    fn refresh_blocker(&mut self, pr_number: u64) {
        let Some(pr) = self.list.prs().iter().find(|pr| pr.number == pr_number) else {
            return;
        };
        let (Some(threads), Some(reviews)) = (
            self.enrichment.review_threads.get(&pr_number),
            self.enrichment.reviews.get(&pr_number),
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
        self.blockers.insert(pr_number, result);
    }

    /// Recompute every cached blocker result, then rebuild the list layout. Used
    /// after any change that can affect tiers (enrichment arrival, a PR's
    /// fields changing, a fresh stream). Keeps the cache and the rendered groups
    /// in lockstep.
    pub fn refresh_blockers(&mut self) {
        let numbers: Vec<u64> = self.list.prs().iter().map(|pr| pr.number).collect();
        for number in numbers {
            self.refresh_blocker(number);
        }
        self.relayout();
    }

    /// Rebuild the list's display layout from the current PRs, cached tiers, and
    /// grouping. Cheap; safe to call after selection/grouping changes too.
    pub fn relayout(&mut self) {
        // Snapshot the inputs `tier_of` needs so the closure doesn't borrow
        // `self` while `self.list` is mutably borrowed.
        let tiers: Vec<Option<Tier>> = (0..self.list.prs().len())
            .map(|i| self.tier_of(i))
            .collect();
        let repo_slug = self.repo_slug();
        self.list.relayout(|i| tiers[i], &repo_slug);
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
