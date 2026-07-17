//! Domain types for the per-PR enrichment layer (review status, checks,
//! reviews, review threads, issue comments). Field sets mirror the TS
//! `src/lib/types.ts` so downstream consumers (blocker engine, summary panel,
//! detail view) stay in lockstep with the reference implementation. Strings are
//! kept permissive (e.g. `mergeable`, `state`, `conclusion`) rather than enums
//! so a value GitHub adds later doesn't fail parsing — same posture as `PR`.

use chrono::{DateTime, Utc};

/// Lifecycle state for a pull request. Mirrors the TS `PRState` discriminated
/// type so the rest of the app can compare against the same values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PRState {
    Open,
    Merged,
    Closed,
}

/// Enrichment fetched per-PR via the batched GraphQL review-status query. These
/// are the fields the REST list endpoint omits; they overwrite the `PR`
/// defaults once they arrive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewStatus {
    pub additions: u64,
    pub deletions: u64,
    pub review_decision: String,
    pub mergeable: String,
    /// The PR's Lifecycle State as of this fetch. The REST list endpoint only
    /// yields `OPEN`; this per-PR query is what detects a `MERGED`/`CLOSED`
    /// transition since the list was fetched (CONTEXT.md "Lifecycle State"), so
    /// the row can stop showing a merged PR's permanent `UNKNOWN` mergeable.
    pub state: PRState,
    /// The PR's last-activity time as of this fetch. Keeps the list's
    /// activity sort and Updated column fresh on a single-PR refresh (`r`),
    /// which never re-runs the REST listing that otherwise supplies it.
    /// Optional under the same permissive-parse posture as the other fields;
    /// an absent value leaves the PR's clock untouched.
    pub updated_at: Option<DateTime<Utc>>,
    pub last_commit_date: Option<DateTime<Utc>>,
    pub head_commit_sha: Option<String>,
}

/// A single CI check run for a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRun {
    pub name: String,
    /// The name of the workflow this run belongs to, when known — the
    /// disambiguator GitHub's checks tab shows as `workflow / job`. Resolved by
    /// mapping the run's check suite to its Actions workflow run; `None` for a
    /// non-Actions check or when the workflow lookup didn't cover it.
    pub workflow_name: Option<String>,
    pub status: String,
    pub conclusion: Option<String>,
    /// When the run began, as reported by the check-runs endpoint. Parse-only:
    /// dropped if the payload omits it (e.g. a queued run, or a commit status
    /// surfaced outside the check-runs endpoint).
    pub started_at: Option<DateTime<Utc>>,
    /// When the run finished. Absent until the run completes.
    pub completed_at: Option<DateTime<Utc>>,
}

impl CheckRun {
    /// The run's Check Duration: wall-clock `completed_at − started_at`, derived
    /// only when BOTH endpoints are present. Keeping the "do we have both"
    /// guard here means callers can't accidentally show a duration computed
    /// from a single timestamp. A negative span (clock skew in the payload) is
    /// treated as no duration rather than a bogus value.
    pub fn duration(&self) -> Option<chrono::Duration> {
        let (started, completed) = (self.started_at?, self.completed_at?);
        let span = completed - started;
        (span >= chrono::Duration::zero()).then_some(span)
    }
}

/// A submitted review, reduced to the latest decision per user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    pub user: String,
    pub state: String,
}

/// One comment inside a review thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewComment {
    pub id: String,
    pub author: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub url: String,
    pub is_bot: bool,
}

/// An inline review-comment thread on a file/line, with its ordered comments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullReviewThread {
    pub id: String,
    pub is_resolved: bool,
    pub path: String,
    pub line: Option<u64>,
    pub comments: Vec<ReviewComment>,
}

/// A top-level PR conversation comment (not tied to a file/line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueComment {
    pub id: u64,
    pub author: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub url: String,
    pub is_bot: bool,
}

/// Whether a commenter is a bot. Mirrors the TS rule: a GraphQL `Bot` typename
/// (or REST `user.type == "Bot"`), a `[bot]` login suffix, or a configured
/// `botLogins` entry. `type_name` carries whichever the source provides.
pub(crate) fn is_bot(login: &str, type_name: Option<&str>, bot_logins: &[String]) -> bool {
    type_name == Some("Bot") || login.ends_with("[bot]") || bot_logins.iter().any(|b| b == login)
}
