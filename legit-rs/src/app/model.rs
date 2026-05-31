use std::collections::HashMap;

use crate::{
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
            },
            vec![Cmd::LoadConfig, Cmd::ResolveAuthToken, Cmd::DetectRepo],
        )
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
