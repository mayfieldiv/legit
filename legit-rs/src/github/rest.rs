use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use octocrab::{Octocrab, Page};
use serde::{Deserialize, de::DeserializeOwned};
use tokio::sync::mpsc;

use crate::{
    github::types::{CheckRun, IssueComment, Review, is_bot},
    secret::Secret,
};

/// Lifecycle state for a pull request. Mirrors the TS `PRState` discriminated
/// type so the rest of the app can compare against the same values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PRState {
    Open,
    Merged,
    Closed,
}

/// Domain type for a pull request. Field set mirrors the TS `PR` interface so
/// the views, blocker engine, and downstream consumers stay in lockstep.
///
/// REST list responses don't include enrichment fields (`review_decision`,
/// `mergeable`, `last_commit_date`, `head_commit_sha`, `additions`,
/// `deletions`); those land via the GraphQL enrichment step in a later
/// milestone. Until then they take the same defaults the TS port uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PR {
    pub number: u64,
    /// `owner/repo` slug of the Tracked Repo this PR belongs to. Stamped by
    /// `list_open_prs` from the repo it was fetched for (not parsed from the
    /// wire) — PR numbers are only unique within a repo, so every cross-repo
    /// keyed structure pairs this with `number` (see `PrKey`).
    pub repo_slug: String,
    pub title: String,
    pub author: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub additions: u64,
    pub deletions: u64,
    pub is_draft: bool,
    pub labels: Vec<String>,
    pub requested_reviewers: Vec<String>,
    pub assignees: Vec<String>,
    pub review_decision: String,
    pub mergeable: String,
    pub last_commit_date: Option<DateTime<Utc>>,
    pub head_commit_sha: Option<String>,
    pub head_ref: String,
    pub base_ref: String,
    pub head_repository_owner: String,
    pub state: PRState,
}

// ── Raw deserialization shape ───────────────────────────────────────────────

/// Permissive intermediate type for parsing GitHub REST responses. Mirrors the
/// TS `RawRestPR`: every field GitHub may omit is optional or defaulted so a
/// stale or stripped response doesn't fail the whole list. Private — the
/// module's contract is `PR`, not the wire shape.
#[derive(Debug, Clone, Deserialize)]
struct RawRestPR {
    number: u64,
    title: String,
    #[serde(default)]
    user: Option<RawUser>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    additions: u64,
    #[serde(default)]
    deletions: u64,
    #[serde(default)]
    labels: Vec<RawLabel>,
    #[serde(default)]
    requested_reviewers: Vec<RawUser>,
    #[serde(default)]
    assignees: Vec<RawUser>,
    #[serde(default)]
    head: Option<RawHead>,
    #[serde(default)]
    base: Option<RawBase>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    merged_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawUser {
    login: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RawLabel {
    name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RawHead {
    #[serde(rename = "ref")]
    ref_field: String,
    #[serde(default)]
    repo: Option<RawRepo>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawBase {
    #[serde(rename = "ref")]
    ref_field: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RawRepo {
    #[serde(default)]
    owner: Option<RawUser>,
}

/// Parse a raw REST pull request into the domain `PR`. Pure; tested directly.
fn parse_pr(raw: RawRestPR) -> PR {
    // GitHub reports merged PRs as state="closed" with merged_at set. Split
    // them into a distinct MERGED state so the UI can distinguish them from
    // PRs closed without being merged. The list endpoint omits `state`
    // entirely, so default to OPEN when absent.
    let state = match (raw.state.as_deref(), raw.merged_at.is_some()) {
        (Some("closed"), true) => PRState::Merged,
        (Some("closed"), false) => PRState::Closed,
        _ => PRState::Open,
    };

    let head_ref = raw
        .head
        .as_ref()
        .map(|h| h.ref_field.clone())
        .unwrap_or_default();
    let head_repository_owner = raw
        .head
        .as_ref()
        .and_then(|h| h.repo.as_ref())
        .and_then(|r| r.owner.as_ref())
        .map(|u| u.login.clone())
        .unwrap_or_default();

    PR {
        number: raw.number,
        // Stamped by `list_open_prs`; the wire shape doesn't carry the slug of
        // the repo the listing was made against.
        repo_slug: String::new(),
        title: raw.title,
        author: raw
            .user
            .map(|u| u.login)
            .unwrap_or_else(|| "ghost".to_owned()),
        created_at: raw.created_at,
        updated_at: raw.updated_at,
        additions: raw.additions,
        deletions: raw.deletions,
        is_draft: raw.draft,
        labels: raw.labels.into_iter().map(|l| l.name).collect(),
        requested_reviewers: raw
            .requested_reviewers
            .into_iter()
            .map(|u| u.login)
            .collect(),
        assignees: raw.assignees.into_iter().map(|u| u.login).collect(),
        review_decision: String::new(),
        mergeable: "UNKNOWN".to_owned(),
        last_commit_date: None,
        head_commit_sha: None,
        head_ref,
        base_ref: raw.base.map(|b| b.ref_field).unwrap_or_default(),
        head_repository_owner,
        state,
    }
}

// ── Octocrab transport ──────────────────────────────────────────────────────

/// Octocrab-backed REST client. Uses a personal access token; the raw `get`
/// lets us deserialize directly into our permissive `RawRestPR` so octocrab's
/// strict model types don't tie us to fields GitHub may omit.
pub struct OctocrabRest {
    client: Octocrab,
}

impl OctocrabRest {
    pub fn new(token: &Secret<String>) -> Result<Self> {
        let client = Octocrab::builder()
            .personal_token(token.expose_secret().to_owned())
            .build()
            .context("failed to build octocrab client")?;
        Ok(Self { client })
    }

    /// List every open PR for `owner/repo`, sending each one through `out` as
    /// it streams in from the REST API. Returns once the listing finishes (or
    /// when `out` closes); errors are returned via `Result`.
    #[tracing::instrument(name = "list_open_prs", skip(self, out))]
    pub async fn list_open_prs(
        &self,
        owner: &str,
        repo: &str,
        out: mpsc::UnboundedSender<PR>,
    ) -> Result<()> {
        let route = format!("/repos/{owner}/{repo}/pulls");
        let params = ListParams {
            state: "open",
            per_page: 100,
        };
        let mut page: Page<RawRestPR> = self
            .client
            .get(&route, Some(&params))
            .await
            .with_context(|| format!("listing open PRs for {owner}/{repo}"))?;

        let repo_slug = format!("{owner}/{repo}");
        loop {
            let items = page.take_items();
            let count = items.len();
            for raw in items {
                let mut pr = parse_pr(raw);
                pr.repo_slug = repo_slug.clone();
                if out.send(pr).is_err() {
                    tracing::debug!("pr receiver dropped; stopping pagination");
                    return Ok(());
                }
            }
            tracing::debug!(count, has_next = page.next.is_some(), "page yielded");

            match self
                .client
                .get_page::<RawRestPR>(&page.next)
                .await
                .with_context(|| format!("fetching next page of PRs for {owner}/{repo}"))?
            {
                Some(next_page) => page = next_page,
                None => return Ok(()),
            }
        }
    }

    /// Fetch all non-pending reviews for a PR, reduced to the latest decision
    /// per user.
    #[tracing::instrument(name = "list_reviews", skip(self))]
    pub async fn list_reviews(&self, owner: &str, repo: &str, number: u64) -> Result<Vec<Review>> {
        let route = format!("/repos/{owner}/{repo}/pulls/{number}/reviews");
        let raw = self
            .get_all::<RawReview>(&route)
            .await
            .with_context(|| format!("listing reviews for {owner}/{repo}#{number}"))?;
        Ok(parse_reviews(raw))
    }

    /// Fetch all top-level conversation comments for a PR, with bot detection.
    #[tracing::instrument(name = "list_issue_comments", skip(self, bot_logins))]
    pub async fn list_issue_comments(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        bot_logins: &[String],
    ) -> Result<Vec<IssueComment>> {
        let route = format!("/repos/{owner}/{repo}/issues/{number}/comments");
        let raw = self
            .get_all::<RawIssueComment>(&route)
            .await
            .with_context(|| format!("listing issue comments for {owner}/{repo}#{number}"))?;
        Ok(parse_issue_comments(raw, bot_logins))
    }

    /// Fetch all CI check runs for a commit. The check-runs endpoint nests the
    /// array under `check_runs` and paginates by `page`, so it can't use the
    /// Link-header `get_all` helper.
    #[tracing::instrument(name = "list_check_runs", skip(self))]
    pub async fn list_check_runs(
        &self,
        owner: &str,
        repo: &str,
        commit_sha: &str,
    ) -> Result<Vec<CheckRun>> {
        let route = format!("/repos/{owner}/{repo}/commits/{commit_sha}/check-runs");
        let mut all = Vec::new();
        let mut page = 1u32;
        loop {
            let params = CheckRunParams {
                per_page: 100,
                page,
            };
            let response: RawCheckRunsResponse = self
                .client
                .get(&route, Some(&params))
                .await
                .with_context(|| {
                    format!("listing check runs for {owner}/{repo}@{commit_sha} (page {page})")
                })?;
            let count = response.check_runs.len();
            all.extend(parse_check_runs(response));
            if count < 100 {
                break;
            }
            page += 1;
        }
        Ok(all)
    }

    /// Follow Link-header pagination for an array endpoint, collecting every
    /// page into one `Vec`. Mirrors the pagination in `list_open_prs`.
    async fn get_all<T: DeserializeOwned>(&self, route: &str) -> octocrab::Result<Vec<T>> {
        let mut items = Vec::new();
        let mut page: Page<T> = self
            .client
            .get(route, Some(&PerPageParams { per_page: 100 }))
            .await?;
        loop {
            items.extend(page.take_items());
            match self.client.get_page::<T>(&page.next).await? {
                Some(next_page) => page = next_page,
                None => return Ok(items),
            }
        }
    }
}

#[derive(serde::Serialize)]
struct ListParams {
    state: &'static str,
    per_page: u8,
}

#[derive(serde::Serialize)]
struct PerPageParams {
    per_page: u8,
}

#[derive(serde::Serialize)]
struct CheckRunParams {
    per_page: u8,
    page: u32,
}

// ── Enrichment raw shapes + parsing ──────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
struct RawReview {
    #[serde(default)]
    user: Option<RawUser>,
    state: String,
    #[serde(default)]
    submitted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawCheckRunsResponse {
    #[serde(default)]
    check_runs: Vec<RawCheckRun>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawCheckRun {
    name: String,
    status: String,
    #[serde(default)]
    conclusion: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawIssueComment {
    id: u64,
    #[serde(default)]
    user: Option<RawCommentUser>,
    #[serde(default)]
    body: String,
    created_at: DateTime<Utc>,
    #[serde(default)]
    html_url: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RawCommentUser {
    login: String,
    #[serde(rename = "type", default)]
    user_type: Option<String>,
}

/// Reduce raw reviews to the latest decision per user. Drops `PENDING` reviews
/// and reviews with no author; ties broken by latest `submitted_at`. Output is
/// sorted by login so the result is deterministic (the list endpoint's order
/// isn't meaningful here).
fn parse_reviews(raw: Vec<RawReview>) -> Vec<Review> {
    let epoch = DateTime::<Utc>::from_timestamp(0, 0).expect("unix epoch is valid");
    let mut latest: HashMap<String, (DateTime<Utc>, String)> = HashMap::new();
    for review in raw {
        if review.state == "PENDING" {
            continue;
        }
        let Some(user) = review.user else {
            continue;
        };
        let submitted = review.submitted_at.unwrap_or(epoch);
        match latest.get(&user.login) {
            Some((existing, _)) if *existing >= submitted => {}
            _ => {
                latest.insert(user.login, (submitted, review.state));
            }
        }
    }
    let mut reviews: Vec<Review> = latest
        .into_iter()
        .map(|(user, (_, state))| Review { user, state })
        .collect();
    reviews.sort_by(|a, b| a.user.cmp(&b.user));
    reviews
}

fn parse_check_runs(raw: RawCheckRunsResponse) -> Vec<CheckRun> {
    raw.check_runs
        .into_iter()
        .map(|run| CheckRun {
            name: run.name,
            status: run.status,
            conclusion: run.conclusion,
        })
        .collect()
}

fn parse_issue_comments(raw: Vec<RawIssueComment>, bot_logins: &[String]) -> Vec<IssueComment> {
    raw.into_iter()
        .map(|comment| {
            let (author, is_bot_author) = match comment.user {
                Some(user) => {
                    let bot = is_bot(&user.login, user.user_type.as_deref(), bot_logins);
                    (user.login, bot)
                }
                None => ("ghost".to_owned(), false),
            };
            IssueComment {
                id: comment.id,
                author,
                body: comment.body,
                created_at: comment.created_at,
                url: comment.html_url,
                is_bot: is_bot_author,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::{
        PR, PRState, RawCheckRunsResponse, RawIssueComment, RawRestPR, RawReview, parse_check_runs,
        parse_issue_comments, parse_pr, parse_reviews,
    };

    fn deserialize(raw: &str) -> RawRestPR {
        serde_json::from_str(raw).expect("fixture should deserialize")
    }

    #[test]
    fn parses_open_pr_from_list_endpoint() {
        let raw = deserialize(
            r#"{
                "number": 42,
                "title": "Add streaming PR list",
                "user": { "login": "octocat" },
                "created_at": "2026-05-01T10:00:00Z",
                "updated_at": "2026-05-02T11:30:00Z",
                "draft": false,
                "labels": [{ "name": "enhancement" }, { "name": "ready-for-agent" }],
                "requested_reviewers": [{ "login": "alice" }, { "login": "bob" }],
                "assignees": [{ "login": "octocat" }],
                "head": {
                    "ref": "issue-43-pr-list",
                    "repo": { "owner": { "login": "mayfieldiv" } }
                },
                "base": { "ref": "main" }
            }"#,
        );
        let pr = parse_pr(raw);
        assert_eq!(
            pr,
            PR {
                number: 42,
                repo_slug: String::new(),
                title: "Add streaming PR list".to_owned(),
                author: "octocat".to_owned(),
                created_at: chrono::Utc.with_ymd_and_hms(2026, 5, 1, 10, 0, 0).unwrap(),
                updated_at: chrono::Utc.with_ymd_and_hms(2026, 5, 2, 11, 30, 0).unwrap(),
                additions: 0,
                deletions: 0,
                is_draft: false,
                labels: vec!["enhancement".to_owned(), "ready-for-agent".to_owned()],
                requested_reviewers: vec!["alice".to_owned(), "bob".to_owned()],
                assignees: vec!["octocat".to_owned()],
                review_decision: String::new(),
                mergeable: "UNKNOWN".to_owned(),
                last_commit_date: None,
                head_commit_sha: None,
                head_ref: "issue-43-pr-list".to_owned(),
                base_ref: "main".to_owned(),
                head_repository_owner: "mayfieldiv".to_owned(),
                state: PRState::Open,
            }
        );
    }

    #[test]
    fn defaults_missing_author_to_ghost() {
        let raw = deserialize(
            r#"{
                "number": 7,
                "title": "Orphaned PR",
                "user": null,
                "created_at": "2026-05-01T00:00:00Z",
                "updated_at": "2026-05-01T00:00:00Z",
                "head": { "ref": "feature" },
                "base": { "ref": "main" }
            }"#,
        );
        let pr = parse_pr(raw);
        assert_eq!(pr.author, "ghost");
    }

    #[test]
    fn parses_closed_pr_as_closed() {
        let raw = deserialize(
            r#"{
                "number": 1,
                "title": "Closed without merge",
                "user": { "login": "octocat" },
                "created_at": "2026-04-01T00:00:00Z",
                "updated_at": "2026-04-02T00:00:00Z",
                "state": "closed",
                "merged_at": null,
                "head": { "ref": "fix/typo" },
                "base": { "ref": "main" }
            }"#,
        );
        assert_eq!(parse_pr(raw).state, PRState::Closed);
    }

    #[test]
    fn parses_merged_pr_as_merged() {
        let raw = deserialize(
            r#"{
                "number": 2,
                "title": "Already merged",
                "user": { "login": "octocat" },
                "created_at": "2026-04-01T00:00:00Z",
                "updated_at": "2026-04-02T00:00:00Z",
                "state": "closed",
                "merged_at": "2026-04-02T01:00:00Z",
                "head": { "ref": "fix/typo" },
                "base": { "ref": "main" }
            }"#,
        );
        assert_eq!(parse_pr(raw).state, PRState::Merged);
    }

    #[test]
    fn defaults_missing_head_repo_owner_to_empty() {
        let raw = deserialize(
            r#"{
                "number": 9,
                "title": "Fork with deleted source",
                "user": { "login": "octocat" },
                "created_at": "2026-05-01T00:00:00Z",
                "updated_at": "2026-05-01T00:00:00Z",
                "head": { "ref": "feat" },
                "base": { "ref": "main" }
            }"#,
        );
        assert_eq!(parse_pr(raw).head_repository_owner, "");
    }

    #[test]
    fn list_endpoint_omits_additions_and_deletions() {
        let raw = deserialize(
            r#"{
                "number": 3,
                "title": "From list endpoint",
                "user": { "login": "octocat" },
                "created_at": "2026-05-01T00:00:00Z",
                "updated_at": "2026-05-01T00:00:00Z",
                "head": { "ref": "feat" },
                "base": { "ref": "main" }
            }"#,
        );
        let pr = parse_pr(raw);
        assert_eq!(pr.additions, 0);
        assert_eq!(pr.deletions, 0);
        assert_eq!(pr.mergeable, "UNKNOWN");
        assert_eq!(pr.review_decision, "");
    }

    #[test]
    fn reviews_keep_latest_decision_per_user() {
        let raw: Vec<RawReview> = serde_json::from_str(
            r#"[
                { "user": { "login": "alice" }, "state": "COMMENTED", "submitted_at": "2026-05-01T00:00:00Z" },
                { "user": { "login": "alice" }, "state": "APPROVED", "submitted_at": "2026-05-02T00:00:00Z" },
                { "user": { "login": "bob" }, "state": "CHANGES_REQUESTED", "submitted_at": "2026-05-01T00:00:00Z" }
            ]"#,
        )
        .expect("deserialize");

        let reviews = parse_reviews(raw);

        // Sorted by login; alice's later APPROVED supersedes her COMMENTED.
        assert_eq!(reviews.len(), 2);
        assert_eq!(reviews[0].user, "alice");
        assert_eq!(reviews[0].state, "APPROVED");
        assert_eq!(reviews[1].user, "bob");
        assert_eq!(reviews[1].state, "CHANGES_REQUESTED");
    }

    #[test]
    fn reviews_drop_pending_and_authorless() {
        let raw: Vec<RawReview> = serde_json::from_str(
            r#"[
                { "user": { "login": "alice" }, "state": "PENDING", "submitted_at": null },
                { "user": null, "state": "APPROVED", "submitted_at": "2026-05-02T00:00:00Z" },
                { "user": { "login": "carol" }, "state": "APPROVED", "submitted_at": "2026-05-03T00:00:00Z" }
            ]"#,
        )
        .expect("deserialize");

        let reviews = parse_reviews(raw);

        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].user, "carol");
    }

    #[test]
    fn check_runs_parse_name_status_conclusion() {
        let raw: RawCheckRunsResponse = serde_json::from_str(
            r#"{ "total_count": 2, "check_runs": [
                { "name": "build", "status": "completed", "conclusion": "success" },
                { "name": "deploy", "status": "in_progress", "conclusion": null }
            ] }"#,
        )
        .expect("deserialize");

        let checks = parse_check_runs(raw);

        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0].name, "build");
        assert_eq!(checks[0].status, "completed");
        assert_eq!(checks[0].conclusion.as_deref(), Some("success"));
        assert_eq!(checks[1].status, "in_progress");
        assert_eq!(checks[1].conclusion, None);
    }

    #[test]
    fn issue_comments_detect_bots_and_default_ghost() {
        let raw: Vec<RawIssueComment> = serde_json::from_str(
            r#"[
                { "id": 1, "user": { "login": "alice", "type": "User" }, "body": "lgtm",
                  "created_at": "2026-05-01T00:00:00Z", "html_url": "u1" },
                { "id": 2, "user": { "login": "ci", "type": "Bot" }, "body": "ran",
                  "created_at": "2026-05-01T01:00:00Z", "html_url": "u2" },
                { "id": 3, "user": { "login": "renovate[bot]", "type": "User" }, "body": "bump",
                  "created_at": "2026-05-01T02:00:00Z", "html_url": "u3" },
                { "id": 4, "user": null, "body": "deleted account",
                  "created_at": "2026-05-01T03:00:00Z", "html_url": "u4" }
            ]"#,
        )
        .expect("deserialize");

        let comments = parse_issue_comments(raw, &["custombot".to_owned()]);

        assert_eq!(comments.len(), 4);
        assert!(!comments[0].is_bot);
        assert_eq!(comments[0].url, "u1");
        assert!(comments[1].is_bot, "type == Bot");
        assert!(comments[2].is_bot, "[bot] suffix");
        assert_eq!(comments[3].author, "ghost");
        assert!(!comments[3].is_bot);
    }
}
