use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use octocrab::{Octocrab, Page};
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::secret::Secret;

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

        loop {
            let items = page.take_items();
            let count = items.len();
            for raw in items {
                let pr = parse_pr(raw);
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
}

#[derive(serde::Serialize)]
struct ListParams {
    state: &'static str,
    per_page: u8,
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::{PR, PRState, RawRestPR, parse_pr};

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
}
