use ratatui::crossterm::event::Event;

use crate::{
    config::LegitConfig,
    git_remote::RepoInfo,
    github::limiter::NetworkStats,
    github::rest::PR,
    github::types::{CheckRun, FullReviewThread, IssueComment, Review, ReviewStatus},
    secret::Secret,
};

#[derive(Debug)]
pub enum Msg {
    TerminalEvent(Event),
    ConfigLoaded(LegitConfig),
    AuthTokenResolved(Secret<String>),
    RepoDetected(RepoInfo),
    PrArrived(PR),
    PrListLoaded,
    NetworkStatsChanged(NetworkStats),
    // ── enrichment arrivals ──
    ReviewStatusArrived {
        pr_number: u64,
        status: ReviewStatus,
    },
    ThreadsArrived {
        pr_number: u64,
        threads: Vec<FullReviewThread>,
    },
    ReviewsArrived {
        pr_number: u64,
        reviews: Vec<Review>,
    },
    ChecksArrived {
        head_sha: String,
        checks: Vec<CheckRun>,
    },
    IssueCommentsArrived {
        pr_number: u64,
        comments: Vec<IssueComment>,
    },
    // ── enrichment failures (per area; none crash the TUI) ──
    ReviewStatusFailed {
        context: &'static str,
        error: String,
    },
    ThreadsFailed {
        context: &'static str,
        error: String,
    },
    ReviewsFailed {
        context: &'static str,
        error: String,
    },
    ChecksFailed {
        context: &'static str,
        error: String,
    },
    IssueCommentsFailed {
        context: &'static str,
        error: String,
    },
    /// A scheduled status-message clear fired; honored only if `token` still
    /// matches the model's current status generation.
    StatusCleared {
        token: u64,
    },
    PrListFailed {
        context: &'static str,
        error: String,
    },
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
