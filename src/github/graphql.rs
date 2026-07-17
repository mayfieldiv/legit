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
mod tests;
