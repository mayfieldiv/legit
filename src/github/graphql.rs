//! Hand-written GitHub GraphQL transport (reqwest + serde). Covers the two
//! queries REST can't serve well: the batched per-repo review-status query and
//! the full review-thread query (with `isResolved` + bot detection). Mirrors
//! the GraphQL half of the TS `src/lib/github-transport.ts`.
//!
//! Parsing is split into pure functions (`parse_review_status`,
//! `parse_review_threads`) tested directly against fixture JSON — the same
//! posture as `github::rest::parse_pr`. The `GraphQlClient` owns only the HTTP;
//! concurrency limiting happens at the command layer.

use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;

use crate::{
    github::types::{FullReviewThread, PRState, ReviewComment, ReviewStatus, is_bot},
    secret::Secret,
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
        owner: &str,
        repo: &str,
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
                variables: json!({ "owner": owner, "repo": repo }),
            };
            let response: ReviewStatusResponse = self.post(&body).await?;
            out.extend(parse_review_status(response));
        }
        Ok(out)
    }

    /// Fetch every review thread for a PR, following pagination.
    #[tracing::instrument(name = "fetch_review_threads", skip(self, bot_logins))]
    pub async fn fetch_review_threads(
        &self,
        owner: &str,
        repo: &str,
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
                    "owner": owner,
                    "repo": repo,
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
mod tests {
    use super::{
        ReviewStatusResponse, ThreadsResponse, ensure_no_errors, parse_review_status,
        parse_review_threads,
    };
    use crate::github::types::PRState;

    #[test]
    fn parses_review_status_batch_with_latest_commit() {
        let raw = r#"{
            "data": { "repository": {
                "pr0": {
                    "number": 42,
                    "additions": 10,
                    "deletions": 3,
                    "reviewDecision": "APPROVED",
                    "mergeable": "MERGEABLE",
                    "state": "OPEN",
                    "updatedAt": "2026-05-11T09:00:00Z",
                    "commits": { "nodes": [ { "commit": {
                        "committedDate": "2026-05-10T12:00:00Z",
                        "oid": "deadbeef"
                    } } ] }
                }
            } }
        }"#;
        let response: ReviewStatusResponse = serde_json::from_str(raw).expect("deserialize");

        let parsed = parse_review_status(response);

        assert_eq!(parsed.len(), 1);
        let (number, status) = &parsed[0];
        assert_eq!(*number, 42);
        assert_eq!(status.additions, 10);
        assert_eq!(status.deletions, 3);
        assert_eq!(status.review_decision, "APPROVED");
        assert_eq!(status.mergeable, "MERGEABLE");
        assert_eq!(status.state, PRState::Open);
        assert_eq!(
            status.updated_at,
            Some(chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 5, 11, 9, 0, 0).unwrap())
        );
        assert_eq!(status.head_commit_sha.as_deref(), Some("deadbeef"));
        assert!(status.last_commit_date.is_some());
    }

    #[test]
    fn review_status_parses_merged_and_closed_lifecycle_state() {
        // The whole point of fetching `state`: a refresh detects the MERGED or
        // CLOSED transition the OPEN-only list endpoint can't, so the row can
        // relabel off a merged PR's permanent UNKNOWN mergeable.
        let raw = r#"{ "data": { "repository": {
            "pr0": { "number": 1, "mergeable": "UNKNOWN", "state": "MERGED", "commits": { "nodes": [] } },
            "pr1": { "number": 2, "mergeable": "UNKNOWN", "state": "CLOSED", "commits": { "nodes": [] } }
        } } }"#;
        let response: ReviewStatusResponse = serde_json::from_str(raw).expect("deserialize");

        let mut parsed = parse_review_status(response);
        parsed.sort_by_key(|(number, _)| *number);

        assert_eq!(parsed[0].1.state, PRState::Merged);
        assert_eq!(parsed[1].1.state, PRState::Closed);
    }

    #[test]
    fn review_status_defaults_missing_fields() {
        let raw = r#"{ "data": { "repository": {
            "pr0": { "number": 7, "commits": { "nodes": [] } }
        } } }"#;
        let response: ReviewStatusResponse = serde_json::from_str(raw).expect("deserialize");

        let parsed = parse_review_status(response);

        let (number, status) = &parsed[0];
        assert_eq!(*number, 7);
        assert_eq!(status.additions, 0);
        assert_eq!(status.review_decision, "");
        assert_eq!(status.mergeable, "UNKNOWN");
        // An absent `state` defaults to Open — the safe direction (keep the PR
        // listed rather than treat a glitch as a merge).
        assert_eq!(status.state, PRState::Open);
        assert_eq!(status.updated_at, None);
        assert_eq!(status.last_commit_date, None);
        assert_eq!(status.head_commit_sha, None);
    }

    #[test]
    fn review_status_drops_null_aliases() {
        let raw = r#"{ "data": { "repository": {
            "pr0": null,
            "pr1": { "number": 99, "mergeable": "CONFLICTING", "commits": { "nodes": [] } }
        } } }"#;
        let response: ReviewStatusResponse = serde_json::from_str(raw).expect("deserialize");

        let parsed = parse_review_status(response);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].0, 99);
        assert_eq!(parsed[0].1.mergeable, "CONFLICTING");
    }

    #[test]
    fn parses_review_threads_with_bot_detection() {
        let raw = r#"{ "data": { "repository": { "pullRequest": { "reviewThreads": {
            "pageInfo": { "hasNextPage": false, "endCursor": null },
            "nodes": [
                {
                    "id": "T1",
                    "isResolved": false,
                    "path": "src/main.rs",
                    "line": 12,
                    "comments": { "nodes": [
                        { "id": "C1", "author": { "login": "alice", "__typename": "User" },
                          "body": "please fix", "createdAt": "2026-05-10T12:00:00Z", "url": "u1" },
                        { "id": "C2", "author": { "login": "dependabot", "__typename": "Bot" },
                          "body": "bump", "createdAt": "2026-05-10T13:00:00Z", "url": "u2" },
                        { "id": "C3", "author": { "login": "renovate[bot]", "__typename": "User" },
                          "body": "update", "createdAt": "2026-05-10T14:00:00Z", "url": "u3" }
                    ] }
                }
            ]
        } } } } }"#;
        let response: ThreadsResponse = serde_json::from_str(raw).expect("deserialize");

        let page = parse_review_threads(response, &["custombot".to_owned()]);

        assert!(!page.has_next_page);
        assert_eq!(page.threads.len(), 1);
        let thread = &page.threads[0];
        assert_eq!(thread.id, "T1");
        assert!(!thread.is_resolved);
        assert_eq!(thread.path, "src/main.rs");
        assert_eq!(thread.line, Some(12));
        assert_eq!(thread.comments.len(), 3);
        assert!(!thread.comments[0].is_bot, "human reviewer is not a bot");
        assert!(thread.comments[1].is_bot, "Bot typename detected");
        assert!(thread.comments[2].is_bot, "[bot] login suffix detected");
    }

    #[test]
    fn review_threads_treats_config_bot_logins_as_bots() {
        let raw = r#"{ "data": { "repository": { "pullRequest": { "reviewThreads": {
            "pageInfo": { "hasNextPage": true, "endCursor": "cursor-1" },
            "nodes": [ { "id": "T1", "isResolved": true, "path": "x", "line": null,
                "comments": { "nodes": [
                    { "id": "C1", "author": { "login": "app/devin-ai-integration" },
                      "body": "done", "createdAt": "2026-05-10T12:00:00Z", "url": "u" }
                ] } } ]
        } } } } }"#;
        let response: ThreadsResponse = serde_json::from_str(raw).expect("deserialize");

        let page = parse_review_threads(response, &["app/devin-ai-integration".to_owned()]);

        assert!(page.has_next_page);
        assert_eq!(page.end_cursor.as_deref(), Some("cursor-1"));
        assert_eq!(page.threads[0].line, None);
        assert!(page.threads[0].comments[0].is_bot, "configured botLogin");
    }

    #[test]
    fn null_author_becomes_ghost_and_not_a_bot() {
        let raw = r#"{ "data": { "repository": { "pullRequest": { "reviewThreads": {
            "pageInfo": { "hasNextPage": false, "endCursor": null },
            "nodes": [ { "id": "T1", "isResolved": false, "path": "x", "line": 1,
                "comments": { "nodes": [
                    { "id": "C1", "author": null, "body": "ghosted",
                      "createdAt": "2026-05-10T12:00:00Z", "url": "u" }
                ] } } ]
        } } } } }"#;
        let response: ThreadsResponse = serde_json::from_str(raw).expect("deserialize");

        let page = parse_review_threads(response, &[]);

        assert_eq!(page.threads[0].comments[0].author, "ghost");
        assert!(!page.threads[0].comments[0].is_bot);
    }

    #[test]
    fn missing_repository_yields_empty_page() {
        let raw = r#"{ "data": { "repository": null } }"#;
        let response: ThreadsResponse = serde_json::from_str(raw).expect("deserialize");

        let page = parse_review_threads(response, &[]);

        assert!(page.threads.is_empty());
        assert!(!page.has_next_page);
    }

    #[test]
    fn graphql_errors_with_http_200_surface_as_err() {
        // GitHub returns query-level failures as HTTP 200 with `data: null` and a
        // populated `errors` array; this must not look like an empty success.
        let raw = r#"{ "data": null, "errors": [
            { "message": "Bad credentials" },
            { "message": "Something went wrong while executing your query." }
        ] }"#;
        let response: ReviewStatusResponse = serde_json::from_str(raw).expect("deserialize");

        let err = ensure_no_errors(response).expect_err("errors must surface as Err");
        let msg = err.to_string();
        assert!(msg.contains("Bad credentials"), "joined messages: {msg}");
        assert!(
            msg.contains("Something went wrong while executing your query."),
            "joined messages: {msg}"
        );
    }

    #[test]
    fn no_errors_passes_response_through() {
        let raw = r#"{ "data": { "repository": {} } }"#;
        let response: ReviewStatusResponse = serde_json::from_str(raw).expect("deserialize");

        let passed = ensure_no_errors(response).expect("clean response passes through");
        assert!(parse_review_status(passed).is_empty());
    }
}
