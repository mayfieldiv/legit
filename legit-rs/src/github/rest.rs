use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use octocrab::{Octocrab, Page};
use serde::Deserialize;
use tokio::sync::mpsc;

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
/// stale or stripped response doesn't fail the whole list.
#[derive(Debug, Clone, Deserialize)]
pub struct RawRestPR {
    pub number: u64,
    pub title: String,
    #[serde(default)]
    pub user: Option<RawUser>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub additions: u64,
    #[serde(default)]
    pub deletions: u64,
    #[serde(default)]
    pub labels: Vec<RawLabel>,
    #[serde(default)]
    pub requested_reviewers: Vec<RawUser>,
    #[serde(default)]
    pub assignees: Vec<RawUser>,
    #[serde(default)]
    pub head: Option<RawHead>,
    #[serde(default)]
    pub base: Option<RawBase>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub merged_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawUser {
    pub login: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawLabel {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawHead {
    #[serde(rename = "ref")]
    pub ref_field: String,
    #[serde(default)]
    pub repo: Option<RawRepo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawBase {
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawRepo {
    #[serde(default)]
    pub owner: Option<RawUser>,
}

/// Parse a raw REST pull request into the domain `PR`. Pure; tested directly.
pub fn parse_pr(raw: RawRestPR) -> PR {
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

// ── Transport trait + octocrab impl ─────────────────────────────────────────

/// REST half of the GitHub client. Behind a trait so tests can swap in a mock.
#[async_trait]
pub trait GitHubRest: Send + Sync {
    /// List every open PR for `owner/repo`, sending each one through `out` as
    /// it streams in from the REST API. Returns once the listing finishes (or
    /// when `out` closes); errors are returned via `Result`.
    async fn list_open_prs(
        &self,
        owner: &str,
        repo: &str,
        out: mpsc::UnboundedSender<PR>,
    ) -> Result<()>;
}

/// Octocrab-backed `GitHubRest`. Uses a personal access token; `_get` lets us
/// deserialize directly into our permissive `RawRestPR` so octocrab's strict
/// model types don't tie us to fields GitHub may omit.
pub struct OctocrabRest {
    client: Octocrab,
}

impl OctocrabRest {
    pub fn new(token: &str) -> Result<Self> {
        let client = Octocrab::builder()
            .personal_token(token.to_owned())
            .build()
            .context("failed to build octocrab client")?;
        Ok(Self { client })
    }

    pub fn with_client(client: Octocrab) -> Self {
        Self { client }
    }
}

#[async_trait]
impl GitHubRest for OctocrabRest {
    #[tracing::instrument(name = "list_open_prs", skip(self, out))]
    async fn list_open_prs(
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
    use std::sync::Mutex;

    use chrono::TimeZone;

    use super::{GitHubRest, PR, PRState, RawRestPR, parse_pr};

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

    // ── Mock transport ──────────────────────────────────────────────────────

    /// Stand-in `GitHubRest` for higher-level tests. Replays a canned list of
    /// PRs into the sink in order.
    struct MockRest {
        prs: Mutex<Vec<PR>>,
    }

    impl MockRest {
        fn new(prs: Vec<PR>) -> Self {
            Self {
                prs: Mutex::new(prs),
            }
        }
    }

    #[async_trait::async_trait]
    impl GitHubRest for MockRest {
        async fn list_open_prs(
            &self,
            _owner: &str,
            _repo: &str,
            out: tokio::sync::mpsc::UnboundedSender<PR>,
        ) -> anyhow::Result<()> {
            let prs = std::mem::take(&mut *self.prs.lock().unwrap());
            for pr in prs {
                let _ = out.send(pr);
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn mock_transport_streams_prs_in_order() {
        let raw_a = deserialize(
            r#"{
                "number": 1, "title": "A", "user": { "login": "a" },
                "created_at": "2026-05-01T00:00:00Z",
                "updated_at": "2026-05-01T00:00:00Z",
                "head": { "ref": "a" }, "base": { "ref": "main" }
            }"#,
        );
        let raw_b = deserialize(
            r#"{
                "number": 2, "title": "B", "user": { "login": "b" },
                "created_at": "2026-05-02T00:00:00Z",
                "updated_at": "2026-05-02T00:00:00Z",
                "head": { "ref": "b" }, "base": { "ref": "main" }
            }"#,
        );
        let mock = MockRest::new(vec![parse_pr(raw_a), parse_pr(raw_b)]);

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        mock.list_open_prs("acme", "widgets", tx).await.unwrap();

        let first = rx.recv().await.expect("first PR streamed");
        let second = rx.recv().await.expect("second PR streamed");
        assert_eq!(first.number, 1);
        assert_eq!(second.number, 2);
        assert!(
            rx.recv().await.is_none(),
            "channel should close after listing"
        );
    }
}
