use ratatui::crossterm::event::Event;

use crate::{
    config::LegitConfig,
    git_remote::RepoInfo,
    github::limiter::NetworkStats,
    github::rest::{PR, PrKey},
    github::types::{CheckRun, FullReviewThread, IssueComment, Review, ReviewStatus},
    secret::Secret,
};

#[derive(Debug)]
pub enum Msg {
    TerminalEvent(Event),
    ConfigLoaded(LegitConfig),
    AuthTokenResolved(Secret<String>),
    /// CWD repo detection settled. `Some` carries the detected GitHub repo;
    /// `None` means detection ran but found none (not a git repo / no GitHub
    /// remote). Either outcome settles the PR-fetch gate so configured Tracked
    /// Repos still fetch when there's no CWD repo.
    RepoDetected(Option<RepoInfo>),
    PrArrived(PR),
    /// One Tracked Repo's open-PR listing finished streaming.
    PrListLoaded {
        repo_slug: String,
    },
    NetworkStatsChanged(NetworkStats),
    // ── enrichment arrivals (keyed by PrKey — numbers collide across repos) ──
    ReviewStatusArrived {
        pr: PrKey,
        status: ReviewStatus,
    },
    ThreadsArrived {
        pr: PrKey,
        threads: Vec<FullReviewThread>,
    },
    ReviewsArrived {
        pr: PrKey,
        reviews: Vec<Review>,
    },
    /// Check runs for one commit in one Tracked Repo. Carries `repo_slug`
    /// because check runs are repo-scoped: a fork PR shares its head SHA with
    /// upstream but not its check runs.
    ChecksArrived {
        repo_slug: String,
        head_sha: String,
        checks: Vec<CheckRun>,
    },
    IssueCommentsArrived {
        pr: PrKey,
        comments: Vec<IssueComment>,
    },
    /// A scheduled status-message clear fired; honored only if `token` still
    /// matches the model's current status generation.
    StatusCleared {
        token: u64,
    },
    /// One Tracked Repo's open-PR listing failed; routes to that repo's
    /// `Failed` phase so the view can surface it distinctly from transient
    /// command errors (and without masking other repos' results).
    PrListFailed {
        repo_slug: String,
        context: &'static str,
        error: String,
    },
    /// Config load failed validation (a malformed `~/.legit/config.json`).
    /// Config is a hard prerequisite for fetching PRs — it supplies the current
    /// user and bot logins that drive smart-status — so this halts the list with
    /// a persistent failure rather than fetching with wrong defaults. A *missing*
    /// config is not an error: it loads as defaults and routes to `ConfigLoaded`.
    ConfigLoadFailed {
        error: String,
    },
    /// Any other command (auth/repo bootstrap or best-effort per-PR enrichment)
    /// failed. All such failures are surfaced identically as a transient
    /// status-bar error, so they share one variant; `context` names the
    /// operation.
    CommandFailed {
        context: &'static str,
        error: String,
    },
    Quit,
}

#[cfg(test)]
mod tests {
    use crate::{app::msg::Msg, secret::Secret};

    #[test]
    fn debug_redacts_auth_token() {
        let msg = Msg::AuthTokenResolved(Secret::new("secret-token".to_owned()));

        let debug = format!("{msg:?}");

        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("secret-token"));
    }
}
