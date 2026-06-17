use ratatui::crossterm::event::Event;

use crate::{
    config::LegitConfig,
    file_category::FileChange,
    git_remote::RepoInfo,
    github::limiter::NetworkStats,
    github::rest::{PR, PrKey},
    github::types::{CheckRun, FullReviewThread, IssueComment, Review, ReviewStatus},
    secret::Secret,
    worktree::WorktreeEntry,
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
    /// A PR's changed files arrived (fetched on selection change). Carries the
    /// raw `FileChange`s; `update` categorises them against the config
    /// `file_rules` and stores the result for the summary panel's breakdown.
    FilesArrived {
        pr: PrKey,
        files: Vec<FileChange>,
    },
    /// A `Cmd::FetchFiles` request failed. Sent alongside the generic
    /// `CommandFailed` (which surfaces the error) so `update` can remove the
    /// PR's `Enrichment::files` entry, returning it from `Requested` to
    /// "never requested" — otherwise a transient error would permanently
    /// suppress refetching and leave the summary panel's file breakdown stuck
    /// on its loading placeholder.
    FilesFetchFailed {
        pr: PrKey,
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
    /// Open an arbitrary URL using the platform browser opener.
    OpenUrl(String),
    /// Open a PR's GitHub web URL from any view that has a PR value.
    OpenInBrowser(PR),
    /// Open a PR's Devin review deep link from any view that has a PR value.
    OpenInDevin(PR),
    /// The platform opener spawned successfully. The browser command itself is
    /// detached, so this acknowledges only successful dispatch.
    OpenUrlSucceeded {
        url: String,
    },
    /// The platform opener failed to spawn.
    OpenUrlFailed {
        url: String,
        error: String,
    },
    /// Parsed `git worktree list --porcelain` entries arrived for one Tracked
    /// Repo's configured source clone.
    WorktreesArrived {
        repo_slug: String,
        entries: Vec<WorktreeEntry>,
    },
    /// `git worktree add -d` + `gh pr checkout` completed.
    WorktreeCreated {
        pr: PrKey,
        path: String,
    },
    /// The terminal accepted the OSC 52 clipboard sequence.
    ClipboardCopied {
        text: String,
    },
    /// Writing the OSC 52 clipboard sequence failed.
    ClipboardCopyFailed {
        text: String,
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
    /// The detail fetch for a PR completed. Carries the PR's key (`pr`, matching
    /// the other enrichment arrivals) so `update` can check whether the view is
    /// still open for this PR, and the body (markdown) to display. The PR itself
    /// is sourced from the enriched list (`model.list.pr(pr)`) rather than
    /// stored here, so the detail view always reads the up-to-date
    /// mergeable/head_commit_sha/etc.
    PRDetailArrived {
        pr: PrKey,
        body: String,
    },
    /// `r` in the list view: refresh the selected PR, including its files. The
    /// network limiter promotes it via focus, so it leads. No-op when nothing
    /// is selected.
    RefreshSelected,
    /// `R` (Shift-r) in the list view: re-read the config (to pick up newly
    /// tracked repos) and refresh every visible PR, dispatched in smart-status
    /// tier order so the limiter's FIFO background lane drains `me-blocking`
    /// first.
    RefreshAll,
    /// One PR's `Cmd::RefreshPr` fan-out finished (success or failure — the
    /// individual fetch failures surface their own `CommandFailed`). Clears the
    /// PR's refresh indicator; once every in-flight refresh has drained, the
    /// run's "Refreshed N" summary posts.
    RefreshComplete {
        pr: PrKey,
    },
    /// A scheduled mergeable re-fetch is due: re-run review-status for one PR
    /// whose `OPEN` mergeable came back `UNKNOWN`. Carries only the identity;
    /// `update` rebuilds the request context. Mirrors the TS settled-index
    /// `UNKNOWN` retry.
    MergeableRetryDue {
        pr: PrKey,
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
