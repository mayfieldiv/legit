//! Domain types for the per-PR enrichment layer (review status, checks,
//! reviews, review threads, issue comments) and the wayfinder ticket
//! transport (issues, sub-issue and dependency summaries). The PR-side field
//! sets mirror the TS `src/lib/types.ts` so downstream consumers (blocker
//! engine, summary panel, detail view) stay in lockstep with the reference
//! implementation. Strings are kept permissive (e.g. `mergeable`, `state`,
//! `conclusion`) rather than enums so a value GitHub adds later doesn't fail
//! parsing — same posture as `PR`.

use chrono::{DateTime, Utc};

use crate::ticket::{Claim, TicketType};

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

// ── wayfinder ticket transport types ─────────────────────────────────────────

/// Lifecycle state for a GitHub issue. REST reports `open`/`closed`, GraphQL
/// `OPEN`/`CLOSED`; [`IssueState::parse`] accepts both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueState {
    Open,
    Closed,
}

impl IssueState {
    /// Parse either transport's casing. An absent or unrecognised value
    /// defaults to `Open` — the safe direction both ways it's used: an open
    /// Ticket stays visible, and an open blocker keeps its dependent off the
    /// Frontier. Mirrors `parse_pr_state`'s `_ => Open` fallback.
    pub fn parse(state: Option<&str>) -> Self {
        match state {
            Some(s) if s.eq_ignore_ascii_case("closed") => IssueState::Closed,
            _ => IssueState::Open,
        }
    }
}

/// The transport-to-domain state mapping both the GraphQL map read and the
/// REST single-ticket refresh normalize through.
impl From<IssueState> for crate::ticket::TicketState {
    fn from(state: IssueState) -> Self {
        match state {
            IssueState::Open => Self::Open,
            IssueState::Closed => Self::Closed,
        }
    }
}

/// A Map issue's sub-issue progress counters, as GitHub reports them on every
/// REST issue payload (`sub_issues_summary`). The GraphQL map read doesn't
/// deserialize its equivalent — an Effort's counts derive from the tickets
/// themselves, which the summary can only lag.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
pub struct SubIssuesSummary {
    #[serde(default)]
    pub total: u64,
    #[serde(default)]
    pub completed: u64,
    #[serde(default)]
    pub percent_completed: u64,
}

/// An issue's dependency counters, from the REST payload's
/// `issue_dependencies_summary` (the GraphQL map read doesn't deserialize its
/// equivalent). `blocked_by`/`blocking` count OPEN counterparts only; the
/// `total_*` pair counts open and closed.
///
/// Never derive blocked-ness from these: the summary is eventually consistent
/// (~10s after a close), so a read right after a blocker closes still counts
/// it. Blocked-ness comes from the `blockedBy` *list* filtered on each node's
/// state (spec §4.2); these ride along as display-only data.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
pub struct DependenciesSummary {
    #[serde(default)]
    pub blocked_by: u64,
    #[serde(default)]
    pub blocking: u64,
    #[serde(default)]
    pub total_blocked_by: u64,
    #[serde(default)]
    pub total_blocking: u64,
}

/// A GitHub issue as the ticket surface consumes it — the REST single-issue
/// refresh's parsed output (the `PR` analog for issues). The whole-map GraphQL
/// read normalizes straight into `ticket::Effort` instead; this type serves
/// the per-ticket paths (drill-in body, single-ticket refresh).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub state: IssueState,
    pub url: String,
    /// The issue's markdown body; empty when the author left it blank.
    pub body: String,
    /// Label names only — the ticket surface derives Type from them and
    /// renders no label chips.
    pub labels: Vec<String>,
    /// Assignee logins; the first one is the Claim.
    pub assignees: Vec<String>,
    pub sub_issues_summary: SubIssuesSummary,
    pub dependencies_summary: DependenciesSummary,
    /// API URL of the parent issue, when this issue is a sub-issue. On every
    /// payload, which is why `GET …/parent` (404 when parentless) is never
    /// called — every 404 stays a genuine error, including the
    /// missing-Issues-scope case, which surfaces as 404. Kept as the raw wire
    /// string: no consumer navigates by it yet; one that does should parse it
    /// into an `EffortKey` rather than compare URLs.
    pub parent_issue_url: Option<String>,
}

/// The Claim a GitHub assignee list carries: the first assignee (the model
/// holds one claimant). Shared by the whole-map read and the single-ticket
/// refresh so the rule can't drift between them.
pub(crate) fn claim_from_assignees(assignees: impl IntoIterator<Item = String>) -> Option<Claim> {
    assignees.into_iter().next().map(Claim::By)
}

/// The Type a GitHub label list carries: the first `wayfinder:<type>` label,
/// prefix stripped. A ticket without one gets the empty Type (shown verbatim,
/// Mode Either); other labels (triage vocabulary) never masquerade as a Type.
/// Shared by the whole-map read and the single-ticket refresh.
pub(crate) fn ticket_type_from_labels<'a>(labels: impl IntoIterator<Item = &'a str>) -> TicketType {
    labels
        .into_iter()
        .find_map(|name| name.strip_prefix("wayfinder:").map(str::to_owned))
        .map(TicketType)
        .unwrap_or_else(|| TicketType(String::new()))
}

/// Whether a commenter is a bot. Mirrors the TS rule: a GraphQL `Bot` typename
/// (or REST `user.type == "Bot"`), a `[bot]` login suffix, or a configured
/// `botLogins` entry. `type_name` carries whichever the source provides.
pub(crate) fn is_bot(login: &str, type_name: Option<&str>, bot_logins: &[String]) -> bool {
    type_name == Some("Bot") || login.ends_with("[bot]") || bot_logins.iter().any(|b| b == login)
}
