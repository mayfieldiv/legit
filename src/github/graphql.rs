//! Hand-written GitHub GraphQL transport (reqwest + serde). Covers the two
//! queries REST can't serve well: the batched per-repo review-status query and
//! the full review-thread query (with `isResolved` + bot detection). Mirrors
//! the GraphQL half of the TS `src/lib/github-transport.ts`.
//!
//! Parsing is split into pure functions (`parse_review_status`,
//! `parse_review_threads`) tested directly against fixture JSON — the same
//! posture as `github::rest::parse_pr`. The `GraphQlClient` owns only the HTTP;
//! concurrency limiting happens at the command layer.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;

use crate::{
    github::types::{FullReviewThread, IssueState, PRState, ReviewComment, ReviewStatus, is_bot},
    repo_slug::RepoSlug,
    secret::Secret,
    ticket::{
        Claim, Dependency, Effort, EffortKey, ExternalDependency, Ticket, TicketKey, TicketType,
    },
};

const GITHUB_GRAPHQL_URL: &str = "https://api.github.com/graphql";
/// GitHub caps aliased batches; the TS client uses 25 PRs per review-status call.
const REVIEW_STATUS_BATCH_SIZE: usize = 25;

// ── graphql-level errors ─────────────────────────────────────────────────────

/// One entry from a GraphQL `errors` array. GitHub returns these with HTTP 200,
/// so a 2xx status alone does not mean the query succeeded — they must be
/// inspected explicitly.
#[derive(Debug, Deserialize)]
struct GraphQlError {
    message: String,
}

/// Implemented by every top-level response envelope so `post` can surface
/// query-level `errors` generically instead of silently parsing `data: null`
/// as an empty (but "successful") result.
trait GraphQlErrors {
    fn errors(&self) -> &[GraphQlError];
}

/// Turn a decoded response into `Err` when it carries any GraphQL-level errors,
/// joining their messages; otherwise pass it through unchanged.
fn ensure_no_errors<T: GraphQlErrors>(response: T) -> Result<T> {
    if response.errors().is_empty() {
        return Ok(response);
    }
    let joined = response
        .errors()
        .iter()
        .map(|e| e.message.as_str())
        .collect::<Vec<_>>()
        .join("; ");
    anyhow::bail!("GitHub GraphQL returned errors: {joined}");
}

// ── review status batch ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ReviewStatusResponse {
    #[serde(default)]
    data: Option<ReviewStatusData>,
    #[serde(default)]
    errors: Vec<GraphQlError>,
}

impl GraphQlErrors for ReviewStatusResponse {
    fn errors(&self) -> &[GraphQlError] {
        &self.errors
    }
}

#[derive(Debug, Deserialize)]
struct ReviewStatusData {
    #[serde(default)]
    repository: Option<HashMap<String, Option<RawReviewStatusNode>>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawReviewStatusNode {
    number: u64,
    #[serde(default)]
    additions: u64,
    #[serde(default)]
    deletions: u64,
    #[serde(default)]
    review_decision: Option<String>,
    #[serde(default)]
    mergeable: Option<String>,
    /// GitHub's `PullRequestState` enum: `OPEN`, `CLOSED`, or `MERGED`. Unlike
    /// the REST list (which reports a merged PR as `closed` + `mergedAt`), the
    /// GraphQL enum is already split, so no `merged_at` cross-check is needed.
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    commits: Option<RawCommitConnection>,
}

#[derive(Debug, Deserialize)]
struct RawCommitConnection {
    #[serde(default)]
    nodes: Vec<RawCommitNode>,
}

#[derive(Debug, Deserialize)]
struct RawCommitNode {
    commit: RawCommit,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCommit {
    #[serde(default)]
    committed_date: Option<DateTime<Utc>>,
    #[serde(default)]
    oid: Option<String>,
}

/// Map GitHub's GraphQL `PullRequestState` enum to the domain `PRState`. An
/// absent or unrecognised value defaults to `Open` — the safe direction, since
/// it keeps the PR in the Open PR List rather than silently treating a glitch
/// as a merge. Mirrors `rest::parse_pr`'s `_ => Open` fallback.
fn parse_pr_state(state: Option<&str>) -> PRState {
    match state {
        Some("MERGED") => PRState::Merged,
        Some("CLOSED") => PRState::Closed,
        _ => PRState::Open,
    }
}

/// Parse a batched review-status response into `(pr_number, ReviewStatus)`
/// pairs. Null aliases (a PR number that resolved to nothing) are dropped; a
/// missing `commits` connection yields `None` date/sha. Order is not preserved
/// (consumers key by PR number).
fn parse_review_status(response: ReviewStatusResponse) -> Vec<(u64, ReviewStatus)> {
    let Some(repo) = response.data.and_then(|d| d.repository) else {
        return Vec::new();
    };
    repo.into_values()
        .flatten()
        .map(|node| {
            let commit = node
                .commits
                .and_then(|c| c.nodes.into_iter().next())
                .map(|n| n.commit);
            let (last_commit_date, head_commit_sha) = match commit {
                Some(c) => (c.committed_date, c.oid),
                None => (None, None),
            };
            (
                node.number,
                ReviewStatus {
                    additions: node.additions,
                    deletions: node.deletions,
                    review_decision: node.review_decision.unwrap_or_default(),
                    mergeable: node.mergeable.unwrap_or_else(|| "UNKNOWN".to_owned()),
                    state: parse_pr_state(node.state.as_deref()),
                    updated_at: node.updated_at,
                    last_commit_date,
                    head_commit_sha,
                },
            )
        })
        .collect()
}

// ── full review threads ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ThreadsResponse {
    #[serde(default)]
    data: Option<ThreadsData>,
    #[serde(default)]
    errors: Vec<GraphQlError>,
}

impl GraphQlErrors for ThreadsResponse {
    fn errors(&self) -> &[GraphQlError] {
        &self.errors
    }
}

#[derive(Debug, Deserialize)]
struct ThreadsData {
    #[serde(default)]
    repository: Option<ThreadsRepo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadsRepo {
    #[serde(default)]
    pull_request: Option<ThreadsPr>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadsPr {
    #[serde(default)]
    review_threads: Option<RawThreadConnection>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawThreadConnection {
    page_info: RawPageInfo,
    #[serde(default)]
    nodes: Vec<RawReviewThread>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPageInfo {
    #[serde(default)]
    has_next_page: bool,
    #[serde(default)]
    end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawReviewThread {
    id: String,
    #[serde(default)]
    is_resolved: bool,
    #[serde(default)]
    path: String,
    #[serde(default)]
    line: Option<u64>,
    comments: RawThreadComments,
}

#[derive(Debug, Deserialize)]
struct RawThreadComments {
    #[serde(default)]
    nodes: Vec<RawThreadComment>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawThreadComment {
    id: String,
    #[serde(default)]
    author: Option<RawAuthor>,
    #[serde(default)]
    body: String,
    created_at: DateTime<Utc>,
    #[serde(default)]
    url: String,
}

#[derive(Debug, Deserialize)]
struct RawAuthor {
    login: String,
    #[serde(rename = "__typename", default)]
    typename: Option<String>,
}

/// One page of review threads plus the cursor needed to fetch the next.
struct ThreadsPage {
    threads: Vec<FullReviewThread>,
    has_next_page: bool,
    end_cursor: Option<String>,
}

/// Parse one page of review threads, resolving bot status per comment. A null
/// author becomes `ghost` and is never a bot (matches the TS guard).
fn parse_review_threads(response: ThreadsResponse, bot_logins: &[String]) -> ThreadsPage {
    let connection = response
        .data
        .and_then(|d| d.repository)
        .and_then(|r| r.pull_request)
        .and_then(|p| p.review_threads);

    let Some(connection) = connection else {
        return ThreadsPage {
            threads: Vec::new(),
            has_next_page: false,
            end_cursor: None,
        };
    };

    let threads = connection
        .nodes
        .into_iter()
        .map(|thread| FullReviewThread {
            id: thread.id,
            is_resolved: thread.is_resolved,
            path: thread.path,
            line: thread.line,
            comments: thread
                .comments
                .nodes
                .into_iter()
                .map(|comment| parse_thread_comment(comment, bot_logins))
                .collect(),
        })
        .collect();

    ThreadsPage {
        threads,
        has_next_page: connection.page_info.has_next_page,
        end_cursor: connection.page_info.end_cursor,
    }
}

fn parse_thread_comment(comment: RawThreadComment, bot_logins: &[String]) -> ReviewComment {
    let (author, is_bot_author) = match comment.author {
        Some(author) => {
            let bot = is_bot(&author.login, author.typename.as_deref(), bot_logins);
            (author.login, bot)
        }
        None => ("ghost".to_owned(), false),
    };
    ReviewComment {
        id: comment.id,
        author,
        body: comment.body,
        created_at: comment.created_at,
        url: comment.url,
        is_bot: is_bot_author,
    }
}

// ── wayfinder map read ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct WayfinderMapResponse {
    #[serde(default)]
    data: Option<WayfinderMapData>,
    #[serde(default)]
    errors: Vec<GraphQlError>,
}

impl GraphQlErrors for WayfinderMapResponse {
    fn errors(&self) -> &[GraphQlError] {
        &self.errors
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WayfinderMapData {
    #[serde(default)]
    rate_limit: Option<RawRateLimit>,
    #[serde(default)]
    repository: Option<WayfinderMapRepo>,
}

#[derive(Debug, Deserialize)]
struct RawRateLimit {
    #[serde(default)]
    cost: u64,
    #[serde(default)]
    remaining: u64,
}

#[derive(Debug, Deserialize)]
struct WayfinderMapRepo {
    #[serde(default)]
    issues: Option<RawMapConnection>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMapConnection {
    page_info: RawPageInfo,
    #[serde(default)]
    nodes: Vec<RawMapNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMapNode {
    number: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    sub_issues: Option<RawTicketConnection>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTicketConnection {
    page_info: RawPageInfo,
    #[serde(default)]
    nodes: Vec<RawTicketNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTicketNode {
    number: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    assignees: Option<RawLoginConnection>,
    #[serde(default)]
    labels: Option<RawLabelConnection>,
    #[serde(default)]
    blocked_by: Option<RawBlockerConnection>,
}

#[derive(Debug, Deserialize)]
struct RawLoginConnection {
    #[serde(default)]
    nodes: Vec<RawLogin>,
}

#[derive(Debug, Deserialize)]
struct RawLogin {
    login: String,
}

#[derive(Debug, Deserialize)]
struct RawLabelConnection {
    #[serde(default)]
    nodes: Vec<RawLabelName>,
}

#[derive(Debug, Deserialize)]
struct RawLabelName {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawBlockerConnection {
    page_info: RawPageInfo,
    #[serde(default)]
    nodes: Vec<RawBlockerNode>,
}

#[derive(Debug, Deserialize)]
struct RawBlockerNode {
    number: u64,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    title: Option<String>,
    /// The blocker's home repo. Read from the payload, never assumed: a
    /// blocker in another repo is an External Dependency.
    #[serde(default)]
    repository: Option<RawRepoName>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRepoName {
    name_with_owner: String,
}

/// One Effort normalized out of the whole-map read, plus its §4.4 degradation
/// signal.
// TODO(#120): remove once the fetch layer consumes map reads.
#[allow(dead_code)]
#[derive(Debug)]
pub struct WayfinderMapRead {
    pub effort: Effort,
    /// The map has zero native sub-issues but its body carries a task list —
    /// the body-line fallback dialect, which v1 detects but never parses. The
    /// effort card renders a degradation notice instead of silently showing
    /// an empty Effort.
    pub fallback_dialect: bool,
}

/// Every open wayfinder map in one repo — the result of one map read.
// TODO(#120): remove once the fetch layer consumes map reads.
#[allow(dead_code)]
#[derive(Debug)]
pub struct WayfinderMapPage {
    pub maps: Vec<WayfinderMapRead>,
    /// The repo has more open maps than the query's `first:10` window. The
    /// query is fixed (spec §4.1), so the surplus is reported, not fetched.
    pub has_more_maps: bool,
}

/// Parse a whole-map response into normalized Efforts. Blocked-ness inputs
/// come from each ticket's `blockedBy` list (filtered on state at derivation
/// time) — never from the eventually-consistent `issueDependenciesSummary`
/// counters, which this parse doesn't even read.
fn parse_wayfinder_maps(
    response: WayfinderMapResponse,
    slug: &RepoSlug,
) -> Result<WayfinderMapPage> {
    let connection = response
        .data
        .and_then(|d| d.repository)
        .and_then(|r| r.issues);
    let Some(connection) = connection else {
        return Ok(WayfinderMapPage {
            maps: Vec::new(),
            has_more_maps: false,
        });
    };
    let maps = connection
        .nodes
        .into_iter()
        .map(|node| parse_map_node(node, slug))
        .collect::<Result<Vec<_>>>()?;
    Ok(WayfinderMapPage {
        maps,
        has_more_maps: connection.page_info.has_next_page,
    })
}

fn parse_map_node(node: RawMapNode, slug: &RepoSlug) -> Result<WayfinderMapRead> {
    // `first:100` is the hard sub-issue cap per parent, so a next page
    // "can't" exist; if it ever does, say so rather than silently showing a
    // partial Effort.
    if node
        .sub_issues
        .as_ref()
        .is_some_and(|c| c.page_info.has_next_page)
    {
        tracing::warn!(map = node.number, "sub-issue list truncated at 100");
    }
    let ticket_nodes = node.sub_issues.map(|c| c.nodes).unwrap_or_default();
    // Same-effort membership is "is a sub-issue of this map", not "lives in
    // this repo": a same-repo blocker outside the map stays External.
    let members: HashSet<u64> = ticket_nodes.iter().map(|t| t.number).collect();
    let tickets: Vec<Ticket> = ticket_nodes
        .into_iter()
        .map(|t| parse_ticket_node(t, slug, &members))
        .collect();
    let fallback_dialect = tickets.is_empty() && body_has_task_list(&node.body);
    let effort = Effort::new(
        EffortKey::GitHub {
            repo_slug: slug.clone(),
            map_number: node.number,
        },
        node.title,
        destination_from_map_body(&node.body),
        tickets,
    )
    .with_context(|| format!("normalizing map {slug}#{}", node.number))?;
    Ok(WayfinderMapRead {
        effort,
        fallback_dialect,
    })
}

fn parse_ticket_node(node: RawTicketNode, slug: &RepoSlug, members: &HashSet<u64>) -> Ticket {
    let claim = node
        .assignees
        .into_iter()
        .flat_map(|c| c.nodes)
        .next()
        .map(|assignee| Claim::By(assignee.login));
    // The Type is the `wayfinder:<type>` label, prefix stripped; a ticket
    // without one gets the empty Type (shown verbatim, Mode Either). Other
    // labels (triage vocabulary) never masquerade as a Type.
    let ty = node
        .labels
        .into_iter()
        .flat_map(|c| c.nodes)
        .find_map(|label| label.name.strip_prefix("wayfinder:").map(str::to_owned))
        .map(TicketType)
        .unwrap_or_else(|| TicketType(String::new()));
    let mut dependencies = Vec::new();
    if let Some(connection) = node.blocked_by {
        dependencies.extend(
            connection
                .nodes
                .into_iter()
                .map(|blocker| parse_blocker(blocker, slug, members)),
        );
        // `first:50` is GitHub's hard relation cap, so this "can't" be true —
        // but unseen blockers must never put a ticket on the Frontier, so a
        // truncated list degrades to an Unknown Dependency.
        if connection.page_info.has_next_page {
            dependencies.push(Dependency::Unknown {
                raw: "blockers beyond the first 50".to_owned(),
            });
        }
    }
    Ticket {
        key: TicketKey::GitHub {
            repo_slug: slug.clone(),
            number: node.number,
        },
        title: node.title,
        state: IssueState::parse(node.state.as_deref()).into(),
        claim,
        ty,
        dependencies,
    }
}

fn parse_blocker(node: RawBlockerNode, slug: &RepoSlug, members: &HashSet<u64>) -> Dependency {
    let repo = match node.repository {
        Some(repo) => match RepoSlug::parse(&repo.name_with_owner) {
            Ok(parsed) => parsed,
            Err(_) => {
                return Dependency::Unknown {
                    raw: format!("{}#{}", repo.name_with_owner, node.number),
                };
            }
        },
        None => {
            return Dependency::Unknown {
                raw: format!("#{}", node.number),
            };
        }
    };
    if repo == *slug && members.contains(&node.number) {
        // Key on the map's slug so the edge is byte-identical to its target
        // ticket's key (RepoSlug equality is case-insensitive anyway).
        Dependency::SameEffort(TicketKey::GitHub {
            repo_slug: slug.clone(),
            number: node.number,
        })
    } else {
        // Outside this Effort — another repo, or a same-repo issue that isn't
        // one of this map's sub-issues. The payload's state and title are
        // captured so the closed/open signal survives without another fetch.
        Dependency::External(ExternalDependency {
            key: TicketKey::GitHub {
                repo_slug: repo,
                number: node.number,
            },
            state: IssueState::parse(node.state.as_deref()).into(),
            title: node.title,
        })
    }
}

/// The first paragraph under the map body's `## Destination` heading — the
/// one-liner the effort card and ticket detail header show. Wrapped lines are
/// joined; `None` when the body has no such heading or the section is empty.
fn destination_from_map_body(body: &str) -> Option<String> {
    let mut lines = body.lines();
    lines.by_ref().find(|line| {
        line.trim()
            .strip_prefix("##")
            .is_some_and(|rest| rest.trim().eq_ignore_ascii_case("destination"))
    })?;
    let mut paragraph: Vec<&str> = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            break;
        }
        if trimmed.is_empty() {
            if paragraph.is_empty() {
                continue;
            }
            break;
        }
        paragraph.push(trimmed);
    }
    (!paragraph.is_empty()).then(|| paragraph.join(" "))
}

/// Whether a map body carries a GitHub task list — half of the §4.4 fallback
/// signal (with zero native sub-issues).
fn body_has_task_list(body: &str) -> bool {
    body.lines().any(|line| {
        let Some(rest) = line.trim_start().strip_prefix(['-', '*', '+']) else {
            return false;
        };
        let rest = rest.trim_start();
        rest.starts_with("[ ]") || rest.starts_with("[x]") || rest.starts_with("[X]")
    })
}

/// Whether a ticket body opens with the fallback dialect's dependency lines
/// (`Part of #n` / `Blocked by: #n`) — the other half of the §4.4 signal,
/// checked when a ticket body arrives (drill-in / single-ticket refresh).
/// Only leading lines count, stopping at the first `##` heading, so refs in
/// prose or code fences don't match; a bare "blocked by" with no issue ref
/// doesn't either. Detection only — the lines are never parsed into the
/// model, and where native data exists a stale `Part of` line is advisory.
// TODO(#120): remove once the per-ticket fetch checks arriving bodies.
#[allow(dead_code)]
pub fn has_fallback_dependency_lines(body: &str) -> bool {
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("##") {
            break;
        }
        let lower = trimmed.to_ascii_lowercase();
        let is_dependency_line = lower.starts_with("part of ") || lower.starts_with("blocked by:");
        if is_dependency_line && line_has_issue_ref(trimmed) {
            return true;
        }
    }
    false
}

/// An issue ref in any of the fallback dialect's accepted forms: `#123`,
/// `owner/repo#123`, or a github.com URL.
fn line_has_issue_ref(line: &str) -> bool {
    if line.contains("github.com/") {
        return true;
    }
    line.match_indices('#').any(|(i, _)| {
        line[i + 1..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
    })
}

// ── transport ────────────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct GraphQlRequest {
    query: String,
    variables: serde_json::Value,
}

/// reqwest-backed GraphQL client. Holds only the HTTP client + token; the
/// concurrency permit is acquired by the caller (command layer).
pub struct GraphQlClient {
    http: reqwest::Client,
    token: Secret<String>,
}

impl GraphQlClient {
    pub fn new(token: &Secret<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent("legit")
            .build()
            .context("failed to build reqwest client")?;
        Ok(Self {
            http,
            token: token.clone(),
        })
    }

    async fn post<T: serde::de::DeserializeOwned + GraphQlErrors>(
        &self,
        body: &GraphQlRequest,
    ) -> Result<T> {
        let response = self
            .http
            .post(GITHUB_GRAPHQL_URL)
            .bearer_auth(self.token.expose_secret())
            .json(body)
            .send()
            .await
            .context("graphql request failed")?;
        let status = response.status();
        if !status.is_success() {
            let detail = response.text().await.unwrap_or_default();
            anyhow::bail!("GitHub GraphQL error: {status}: {detail}");
        }
        let decoded: T = response.json().await.context("decoding graphql response")?;
        // GitHub reports query-level failures as HTTP 200 with an `errors` array
        // and null/partial `data`; surface them rather than parsing empty data
        // as a successful (but empty) result.
        ensure_no_errors(decoded)
    }

    /// Fetch review status for many PRs, batched per `REVIEW_STATUS_BATCH_SIZE`.
    #[tracing::instrument(name = "fetch_review_status", skip(self, pr_numbers))]
    pub async fn fetch_review_status(
        &self,
        slug: &RepoSlug,
        pr_numbers: &[u64],
    ) -> Result<Vec<(u64, ReviewStatus)>> {
        let mut out = Vec::new();
        for chunk in pr_numbers.chunks(REVIEW_STATUS_BATCH_SIZE) {
            let aliases = chunk
                .iter()
                .enumerate()
                .map(|(i, number)| {
                    format!(
                        "pr{i}: pullRequest(number: {number}) {{ number additions deletions \
                         reviewDecision mergeable state updatedAt commits(last: 1) {{ nodes {{ \
                         commit {{ committedDate oid }} }} }} }}"
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            let query = format!(
                "query($owner: String!, $repo: String!) {{ \
                 repository(owner: $owner, name: $repo) {{ {aliases} }} }}"
            );
            let body = GraphQlRequest {
                query,
                variables: json!({ "owner": slug.owner(), "repo": slug.name() }),
            };
            let response: ReviewStatusResponse = self.post(&body).await?;
            out.extend(parse_review_status(response));
        }
        Ok(out)
    }

    /// One whole-map read: every open issue labelled `label` (the wayfinder
    /// maps) in `slug`, with sub-issues and their blockers, normalized into
    /// Efforts. The N+1 collapse that puts this in GraphQL: one query,
    /// measured cost 10 of 5,000 points/hr, flat in map size (spec §4.1).
    // TODO(#120): remove once the fetch layer dispatches map reads.
    #[allow(dead_code)]
    #[tracing::instrument(name = "fetch_wayfinder_map", skip(self))]
    pub async fn fetch_wayfinder_map(
        &self,
        slug: &RepoSlug,
        label: &str,
    ) -> Result<WayfinderMapPage> {
        // Verbatim from spec §4.1. `first:100`/`first:50` are GitHub's hard
        // relation caps, so `hasNextPage` can't be true — `pageInfo` is
        // selected anyway, and the parse degrades if it ever lies.
        const QUERY: &str = "query($owner:String!, $repo:String!, $label:String!) {
            rateLimit { cost remaining resetAt }
            repository(owner:$owner, name:$repo) {
                issues(first:10, labels:[$label], states:[OPEN]) {
                    pageInfo { hasNextPage endCursor }
                    nodes {
                        number title state url body
                        subIssuesSummary { total completed percentCompleted }
                        subIssues(first:100) {
                            pageInfo { hasNextPage endCursor }
                            nodes {
                                number title state stateReason url
                                assignees(first:5) { nodes { login } }
                                labels(first:10) { nodes { name } }
                                issueDependenciesSummary { blockedBy blocking totalBlockedBy totalBlocking }
                                blockedBy(first:50) {
                                    pageInfo { hasNextPage endCursor }
                                    nodes { number state title repository { nameWithOwner } }
                                }
                            }
                        }
                    }
                }
            }
        }";
        let body = GraphQlRequest {
            query: QUERY.to_owned(),
            variables: json!({ "owner": slug.owner(), "repo": slug.name(), "label": label }),
        };
        let response: WayfinderMapResponse = self.post(&body).await?;
        if let Some(rate) = response.data.as_ref().and_then(|d| d.rate_limit.as_ref()) {
            tracing::debug!(
                cost = rate.cost,
                remaining = rate.remaining,
                "map read cost"
            );
        }
        let page = parse_wayfinder_maps(response, slug)?;
        if page.has_more_maps {
            // The fixed query reads one `first:10` window; a repo with more
            // open maps gets the surplus reported, not silently dropped.
            tracing::warn!(%slug, "more than 10 open wayfinder maps; reading the first 10");
        }
        Ok(page)
    }

    /// Fetch every review thread for a PR, following pagination.
    #[tracing::instrument(name = "fetch_review_threads", skip(self, bot_logins))]
    pub async fn fetch_review_threads(
        &self,
        slug: &RepoSlug,
        number: u64,
        bot_logins: &[String],
    ) -> Result<Vec<FullReviewThread>> {
        const QUERY: &str = "query($owner: String!, $repo: String!, $number: Int!, $after: String) \
             { repository(owner: $owner, name: $repo) { pullRequest(number: $number) { \
             reviewThreads(first: 100, after: $after) { pageInfo { hasNextPage endCursor } \
             nodes { id isResolved path line comments(first: 100) { nodes { id \
             author { login __typename } body createdAt url } } } } } } }";

        let mut threads = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let body = GraphQlRequest {
                query: QUERY.to_owned(),
                variables: json!({
                    "owner": slug.owner(),
                    "repo": slug.name(),
                    "number": number,
                    "after": after,
                }),
            };
            let response: ThreadsResponse = self.post(&body).await?;
            let page = parse_review_threads(response, bot_logins);
            threads.extend(page.threads);
            if !page.has_next_page || page.end_cursor.is_none() {
                break;
            }
            after = page.end_cursor;
        }
        Ok(threads)
    }
}

#[cfg(test)]
mod tests;
