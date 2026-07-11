use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use octocrab::{Octocrab, Page};
use serde::{Deserialize, de::DeserializeOwned};
use tokio::sync::{OnceCell, mpsc};

use crate::{
    file_category::FileChange,
    github::types::{CheckRun, IssueComment, PRState, Review, ReviewStatus, is_bot},
    secret::Secret,
};

/// Domain type for a pull request. Field set mirrors the TS `PR` interface so
/// the views, blocker engine, and downstream consumers stay in lockstep.
///
/// REST list responses don't include enrichment fields (`review_decision`,
/// `mergeable`, `last_commit_date`, `head_commit_sha`, `additions`,
/// `deletions`); those land via the GraphQL enrichment step in a later
/// milestone. Until then they take the same defaults the TS port uses, and
/// `review_status_loaded` stays false so the view can distinguish unknown size
/// from a real zero-line PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PR {
    pub number: u64,
    /// `owner/repo` slug of the Tracked Repo this PR belongs to. Supplied by
    /// the caller from the repo it was fetched for (not parsed from the wire)
    /// — PR numbers are only unique within a repo, so every cross-repo keyed
    /// structure pairs this with `number` (see `PrKey`).
    pub repo_slug: String,
    pub title: String,
    pub author: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub additions: u64,
    pub deletions: u64,
    pub is_draft: bool,
    pub labels: Vec<Label>,
    pub requested_reviewers: Vec<String>,
    pub assignees: Vec<String>,
    pub review_decision: String,
    pub mergeable: String,
    pub last_commit_date: Option<DateTime<Utc>>,
    pub head_commit_sha: Option<String>,
    pub review_status_loaded: bool,
    pub head_ref: String,
    pub base_ref: String,
    pub head_repository_owner: String,
    pub state: PRState,
}

/// A PR label as legit renders it: its name plus the GitHub colour that drives
/// its Label Chip. The colour rides in on the existing label payload (no new
/// request); it is optional because GitHub may leave it blank, in which case the
/// chip falls back to a stable hashed colour. Mirrors the TS `PullRequestLabel`
/// (`{ name, color }`). Labels stay domain-inert — purely contextual metadata
/// with no sort, filter, or Smart-status effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub name: String,
    /// The label's GitHub colour as a bare `rrggbb` hex string, or `None` when
    /// GitHub left it blank. Not pre-parsed: `label_color` resolves it (or the
    /// hashed fallback) at render time.
    pub color: Option<String>,
}

/// Globally-unique PR identity across Tracked Repos: PR numbers alone collide
/// between repos, so every cross-repo keyed structure (enrichment maps, cached
/// blockers) keys on slug + number.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PrKey {
    pub repo_slug: String,
    pub number: u64,
}

impl PrKey {
    /// The PR's GitHub web URL — the one canonical construction, shared by the
    /// detail header and the `o` browser action's body fallback.
    pub fn html_url(&self) -> String {
        format!("https://github.com/{}/pull/{}", self.repo_slug, self.number)
    }
}

impl PR {
    /// The cross-repo identity key for this PR.
    pub fn key(&self) -> PrKey {
        PrKey {
            repo_slug: self.repo_slug.clone(),
            number: self.number,
        }
    }

    /// Adopt a fresh listing copy of this PR — it carries listing-level changes
    /// (title, labels, draft, updated_at) that nothing else re-fetches — while
    /// keeping the enrichment fields the REST list endpoint can't supply.
    /// Returns whether anything changed.
    ///
    /// The kept set must mirror the fields `apply_review_status` writes, or a
    /// re-list silently wipes one — so `fresh` is destructured exhaustively
    /// (no `..` spread): adding a `PR` field fails to compile here, forcing an
    /// explicit listing-vs-enrichment decision. `state` is kept too: the
    /// enrichment refresh is what detects a MERGED/CLOSED transition, and the
    /// listing's default-Open must not relabel a PR a refresh already marked
    /// merged.
    pub fn adopt_listing(&mut self, fresh: PR) -> bool {
        // Enrichment fields the listing can't supply bind to `_`; their pooled
        // values are kept below.
        let PR {
            number,
            repo_slug,
            title,
            author,
            created_at,
            updated_at,
            additions: _,
            deletions: _,
            is_draft,
            labels,
            requested_reviewers,
            assignees,
            review_decision: _,
            mergeable: _,
            last_commit_date: _,
            head_commit_sha: _,
            review_status_loaded: _,
            head_ref,
            base_ref,
            head_repository_owner,
            state: _,
        } = fresh;
        let merged = PR {
            number,
            repo_slug,
            title,
            author,
            created_at,
            updated_at,
            additions: self.additions,
            deletions: self.deletions,
            is_draft,
            labels,
            requested_reviewers,
            assignees,
            review_decision: self.review_decision.clone(),
            mergeable: self.mergeable.clone(),
            last_commit_date: self.last_commit_date,
            head_commit_sha: self.head_commit_sha.clone(),
            review_status_loaded: self.review_status_loaded,
            head_ref,
            base_ref,
            head_repository_owner,
            state: self.state.clone(),
        };
        if *self == merged {
            return false;
        }
        *self = merged;
        true
    }

    /// Overwrite the enrichment fields with a fresh GraphQL review status —
    /// the fields the REST list endpoint couldn't supply. The write side of
    /// the partition `adopt_listing` keeps across re-lists; `status` is
    /// destructured exhaustively so adding a `ReviewStatus` field fails to
    /// compile until it's applied here (and kept in `adopt_listing`).
    pub fn apply_review_status(&mut self, status: ReviewStatus) {
        let ReviewStatus {
            additions,
            deletions,
            review_decision,
            mergeable,
            state,
            updated_at,
            last_commit_date,
            head_commit_sha,
        } = status;
        self.additions = additions;
        self.deletions = deletions;
        self.review_decision = review_decision;
        self.mergeable = mergeable;
        // A refresh is the only thing that detects a MERGED/CLOSED transition
        // since the list was fetched (the list endpoint only yields OPEN).
        // Applying it lets the row show the real lifecycle state instead of a
        // merged PR's permanent UNKNOWN mergeable.
        self.state = state;
        self.last_commit_date = last_commit_date;
        self.head_commit_sha = head_commit_sha;
        // The activity clock drives the list's sort order and Updated column;
        // a single-PR refresh (`r`) never re-runs the REST listing that
        // otherwise supplies it. An absent value (permissive parse) leaves
        // the clock untouched.
        if let Some(updated_at) = updated_at {
            self.updated_at = updated_at;
        }
        self.review_status_loaded = true;
    }
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
    /// The label's hex colour (bare `rrggbb`), already in the list payload.
    /// Defaulted so a stripped response still parses; an empty string is
    /// normalised to `None` in `parse_pr` so the chip takes the hashed fallback.
    #[serde(default)]
    color: String,
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

/// Parse a raw REST pull request into the domain `PR`. The wire shape doesn't
/// carry the slug of the repo the listing was made against, so the caller
/// supplies it. Pure; tested directly.
fn parse_pr(raw: RawRestPR, repo_slug: &str) -> PR {
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
        repo_slug: repo_slug.to_owned(),
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
        labels: raw
            .labels
            .into_iter()
            .map(|l| Label {
                name: l.name,
                // Normalise GitHub's blank colour to `None` so the chip falls
                // back to its hashed colour rather than an empty hex string.
                color: (!l.color.is_empty()).then_some(l.color),
            })
            .collect(),
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
        review_status_loaded: false,
        head_ref,
        base_ref: raw.base.map(|b| b.ref_field).unwrap_or_default(),
        head_repository_owner,
        state,
    }
}

// ── Octocrab transport ──────────────────────────────────────────────────────

/// A list-load-scoped memo of a repo's Actions `workflow_id → name` map. The map
/// is repo-global and immutable for the session, but a fresh [`OctocrabRest`] is
/// built per check fetch, so without this every PR's check fetch would re-page
/// the identical `GET /actions/workflows` list. Shared by `Arc` inside the per-PR
/// `RequestContext` so the list is fetched once per list-load rather than once
/// per PR; the per-commit workflow-runs lookup still runs per PR.
#[derive(Clone, Default)]
pub struct WorkflowNameCache(Arc<OnceCell<HashMap<u64, String>>>);

impl WorkflowNameCache {
    /// The repo's `workflow_id → name` map, fetching and memoising it on first
    /// use. A failed fetch isn't cached, so a later check fetch retries.
    async fn get_or_init(
        &self,
        rest: &OctocrabRest,
        owner: &str,
        repo: &str,
    ) -> Result<&HashMap<u64, String>> {
        self.0
            .get_or_try_init(|| rest.workflow_names_by_id(owner, repo))
            .await
    }
}

impl std::fmt::Debug for WorkflowNameCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkflowNameCache").finish_non_exhaustive()
    }
}

// The cache is derived data, not part of a request's identity, so two contexts
// are equal regardless of cache state (and it's empty in the equality tests).
impl PartialEq for WorkflowNameCache {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for WorkflowNameCache {}

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
                let pr = parse_pr(raw, &repo_slug);
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

    /// Fetch all CI check runs for a commit, each tagged with its `workflow / job`
    /// name. The companion Actions workflow-name lookup is independent of the
    /// check-runs fetch, so the two run concurrently and the labelling latency
    /// overlaps the check fetch rather than preceding it. The lookup is
    /// best-effort — `join!` (not `try_join!`) lets a label failure yield bare
    /// job names rather than failing the whole fetch.
    #[tracing::instrument(name = "list_check_runs", skip(self, cache))]
    pub async fn list_check_runs(
        &self,
        owner: &str,
        repo: &str,
        commit_sha: &str,
        cache: &WorkflowNameCache,
    ) -> Result<Vec<CheckRun>> {
        let (workflows, raw) = tokio::join!(
            self.workflow_names_by_suite(owner, repo, commit_sha, cache),
            self.fetch_raw_check_runs(owner, repo, commit_sha),
        );
        let workflows = workflows.unwrap_or_else(|error| {
            tracing::warn!(%error, "workflow-name lookup failed; using bare check names");
            HashMap::new()
        });
        Ok(parse_check_runs(raw?, &workflows))
    }

    /// Fetch and accumulate every page of a commit's raw check runs. The
    /// check-runs endpoint nests the array under `check_runs` and paginates by
    /// `page`, so it can't use the Link-header `get_all` helper. Split from the
    /// workflow-name lookup so [`list_check_runs`] can run the two concurrently.
    #[tracing::instrument(name = "fetch_raw_check_runs", skip(self))]
    async fn fetch_raw_check_runs(
        &self,
        owner: &str,
        repo: &str,
        commit_sha: &str,
    ) -> Result<RawCheckRunsResponse> {
        let route = format!("/repos/{owner}/{repo}/commits/{commit_sha}/check-runs");
        let mut check_runs = Vec::new();
        let mut page = 1u32;
        loop {
            let params = PageParams {
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
            check_runs.extend(response.check_runs);
            if count < 100 {
                break;
            }
            page += 1;
        }
        Ok(RawCheckRunsResponse { check_runs })
    }

    /// Map each of a commit's check suites to the display name of the Actions
    /// workflow that produced it, driving the `workflow / job` check labels.
    ///
    /// Composed from two independent lookups, run concurrently: the repo's
    /// workflows (`GET /actions/workflows`, memoised in `cache`) give
    /// `workflow_id → name`, and the commit's workflow runs
    /// (`GET /actions/runs?head_sha=…`) give `check_suite_id → workflow_id`. Both
    /// must succeed for a useful map, so `try_join!` short-circuits on either
    /// error. The workflow's `name:` is used deliberately rather than the per-run
    /// name, which a `run-name:` override can replace (e.g. CodeQL's `PR #123`).
    /// Checks not produced by Actions (external statuses, other apps) won't appear.
    #[tracing::instrument(name = "workflow_names_by_suite", skip(self, cache))]
    async fn workflow_names_by_suite(
        &self,
        owner: &str,
        repo: &str,
        head_sha: &str,
        cache: &WorkflowNameCache,
    ) -> Result<HashMap<u64, String>> {
        let (names_by_id, suite_workflow_ids) = tokio::try_join!(
            cache.get_or_init(self, owner, repo),
            self.suite_workflow_ids(owner, repo, head_sha),
        )?;
        Ok(suite_workflow_ids
            .into_iter()
            .filter_map(|(suite_id, workflow_id)| {
                names_by_id
                    .get(&workflow_id)
                    .map(|name| (suite_id, name.clone()))
            })
            .collect())
    }

    /// Map each of a commit's check suites to the Actions workflow id that
    /// produced it, via `GET /actions/runs?head_sha=…` (paginated by `page`). The
    /// `check_suite_id → workflow_id` half of [`workflow_names_by_suite`]'s join;
    /// the names come from the cached repo workflow list. A re-run reuses the
    /// suite id, so a later page's run wins for that suite — matching the eventual
    /// `collect` into a map.
    #[tracing::instrument(name = "suite_workflow_ids", skip(self))]
    async fn suite_workflow_ids(
        &self,
        owner: &str,
        repo: &str,
        head_sha: &str,
    ) -> Result<HashMap<u64, u64>> {
        let route = format!("/repos/{owner}/{repo}/actions/runs");
        let mut by_suite = HashMap::new();
        let mut page = 1u32;
        loop {
            let params = WorkflowRunParams {
                head_sha,
                per_page: 100,
                page,
            };
            let response: RawWorkflowRunsResponse = self
                .client
                .get(&route, Some(&params))
                .await
                .with_context(|| {
                    format!("listing workflow runs for {owner}/{repo}@{head_sha} (page {page})")
                })?;
            let count = response.workflow_runs.len();
            for run in response.workflow_runs {
                if let (Some(suite_id), Some(workflow_id)) = (run.check_suite_id, run.workflow_id) {
                    by_suite.insert(suite_id, workflow_id);
                }
            }
            if count < 100 {
                break;
            }
            page += 1;
        }
        Ok(by_suite)
    }

    /// Map each Actions workflow in the repo to its display name (the `name:`
    /// field), via `GET /repos/:owner/:repo/actions/workflows`. The join target
    /// for [`workflow_names_by_suite`]; the response nests workflows under
    /// `workflows` and paginates by `page`.
    #[tracing::instrument(name = "workflow_names_by_id", skip(self))]
    async fn workflow_names_by_id(&self, owner: &str, repo: &str) -> Result<HashMap<u64, String>> {
        let route = format!("/repos/{owner}/{repo}/actions/workflows");
        let mut by_id = HashMap::new();
        let mut page = 1u32;
        loop {
            let params = PageParams {
                per_page: 100,
                page,
            };
            let response: RawWorkflowsResponse = self
                .client
                .get(&route, Some(&params))
                .await
                .with_context(|| format!("listing workflows for {owner}/{repo} (page {page})"))?;
            let count = response.workflows.len();
            for workflow in response.workflows {
                by_id.insert(workflow.id, workflow.name);
            }
            if count < 100 {
                break;
            }
            page += 1;
        }
        Ok(by_id)
    }

    /// Fetch a single PR's body (markdown). The single-PR endpoint at
    /// `/repos/{owner}/{repo}/pulls/{number}` is used because the list
    /// endpoint omits `body`; all other PR fields are sourced from the
    /// enriched list PR rather than this response.
    #[tracing::instrument(name = "fetch_pr_detail", skip(self))]
    pub async fn fetch_pr_detail(&self, owner: &str, repo: &str, number: u64) -> Result<String> {
        let route = format!("/repos/{owner}/{repo}/pulls/{number}");
        let raw: RawRestPRDetail = self
            .client
            .get(&route, None::<&()>)
            .await
            .with_context(|| format!("fetching PR detail for {owner}/{repo}#{number}"))?;
        Ok(raw.body.unwrap_or_default())
    }

    /// Fetch the changed files for a PR (path + additions/deletions per file).
    /// Drives the summary panel's File Category breakdown; mirrors the TS
    /// `fetchCategorizedFiles` minus the categorisation, which `update` does
    /// against the config `file_rules`.
    #[tracing::instrument(name = "list_files", skip(self))]
    pub async fn list_files(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<Vec<FileChange>> {
        let route = format!("/repos/{owner}/{repo}/pulls/{number}/files");
        let raw = self
            .get_all::<RawFile>(&route)
            .await
            .with_context(|| format!("listing files for {owner}/{repo}#{number}"))?;
        Ok(raw
            .into_iter()
            .map(|file| FileChange {
                path: file.filename,
                additions: file.additions,
                deletions: file.deletions,
            })
            .collect())
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

/// Generic page cursor for endpoints that paginate by `?per_page&page` (the
/// check-runs and workflows lists).
#[derive(serde::Serialize)]
struct PageParams {
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
    #[serde(default)]
    started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    completed_at: Option<DateTime<Utc>>,
    /// The check suite this run belongs to. Joined against the Actions
    /// workflow-runs endpoint to recover the run's `workflow / job` name.
    #[serde(default)]
    check_suite: Option<RawCheckSuite>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawCheckSuite {
    id: u64,
}

/// Wire shape for the Actions workflow-runs endpoint
/// (`GET /repos/:owner/:repo/actions/runs?head_sha=…`). Links each check suite
/// to the workflow that produced it; the workflow's display name is then looked
/// up via [`RawWorkflowsResponse`].
#[derive(Debug, Clone, Deserialize)]
struct RawWorkflowRunsResponse {
    #[serde(default)]
    workflow_runs: Vec<RawWorkflowRun>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawWorkflowRun {
    /// The workflow this run belongs to; joined against [`RawWorkflow`] for the
    /// display name. Preferred over the run's own `name`, which a `run-name:`
    /// override can replace.
    #[serde(default)]
    workflow_id: Option<u64>,
    /// The check suite this workflow run created; the join key onto `RawCheckRun`.
    #[serde(default)]
    check_suite_id: Option<u64>,
}

#[derive(serde::Serialize)]
struct WorkflowRunParams<'a> {
    head_sha: &'a str,
    per_page: u8,
    page: u32,
}

/// Wire shape for the Actions workflows endpoint
/// (`GET /repos/:owner/:repo/actions/workflows`). Supplies each workflow's
/// display name (its `name:` field) for the `workflow / job` check labels.
#[derive(Debug, Clone, Deserialize)]
struct RawWorkflowsResponse {
    #[serde(default)]
    workflows: Vec<RawWorkflow>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawWorkflow {
    id: u64,
    name: String,
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

/// Wire shape for the single-PR detail endpoint (`GET /repos/:owner/:repo/pulls/:number`).
/// We only need the `body` field here; all other PR fields are sourced from
/// the enriched list PR rather than re-parsed from this response.
#[derive(Debug, Clone, Deserialize)]
struct RawRestPRDetail {
    /// The PR's markdown body, or `null` / absent when the author left it empty.
    #[serde(default)]
    body: Option<String>,
}

/// One entry from the PR `files` endpoint. Only the fields the breakdown needs;
/// `additions`/`deletions` default to 0 so a stripped response still parses.
#[derive(Debug, Clone, Deserialize)]
struct RawFile {
    filename: String,
    #[serde(default)]
    additions: u64,
    #[serde(default)]
    deletions: u64,
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

/// Convert raw check runs to domain `CheckRun`s, resolving each run's workflow
/// name from `workflows` (a check-suite-id → workflow-name map). A run whose
/// suite isn't in the map (a non-Actions check, or a gap in the lookup) keeps a
/// `None` workflow name and renders as its bare job name.
fn parse_check_runs(raw: RawCheckRunsResponse, workflows: &HashMap<u64, String>) -> Vec<CheckRun> {
    raw.check_runs
        .into_iter()
        .map(|run| CheckRun {
            name: run.name,
            workflow_name: run
                .check_suite
                .and_then(|suite| workflows.get(&suite.id).cloned()),
            status: run.status,
            conclusion: run.conclusion,
            started_at: run.started_at,
            completed_at: run.completed_at,
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
    use std::collections::HashMap;

    use chrono::TimeZone;

    use super::{
        Label, PR, PRState, RawCheckRunsResponse, RawIssueComment, RawRestPR, RawReview,
        parse_check_runs, parse_issue_comments, parse_pr, parse_reviews,
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
                "labels": [
                    { "name": "enhancement", "color": "a2eeef" },
                    { "name": "ready-for-agent", "color": "" }
                ],
                "requested_reviewers": [{ "login": "alice" }, { "login": "bob" }],
                "assignees": [{ "login": "octocat" }],
                "head": {
                    "ref": "issue-43-pr-list",
                    "repo": { "owner": { "login": "mayfieldiv" } }
                },
                "base": { "ref": "main" }
            }"#,
        );
        let pr = parse_pr(raw, "mayfieldiv/legit");
        assert_eq!(
            pr,
            PR {
                number: 42,
                repo_slug: "mayfieldiv/legit".to_owned(),
                title: "Add streaming PR list".to_owned(),
                author: "octocat".to_owned(),
                created_at: chrono::Utc.with_ymd_and_hms(2026, 5, 1, 10, 0, 0).unwrap(),
                updated_at: chrono::Utc.with_ymd_and_hms(2026, 5, 2, 11, 30, 0).unwrap(),
                additions: 0,
                deletions: 0,
                is_draft: false,
                labels: vec![
                    Label {
                        name: "enhancement".to_owned(),
                        color: Some("a2eeef".to_owned()),
                    },
                    // A blank GitHub colour normalises to `None`, so the chip
                    // takes the hashed fallback rather than an empty hex string.
                    Label {
                        name: "ready-for-agent".to_owned(),
                        color: None,
                    },
                ],
                requested_reviewers: vec!["alice".to_owned(), "bob".to_owned()],
                assignees: vec!["octocat".to_owned()],
                review_decision: String::new(),
                mergeable: "UNKNOWN".to_owned(),
                last_commit_date: None,
                head_commit_sha: None,
                review_status_loaded: false,
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
        let pr = parse_pr(raw, "mayfieldiv/legit");
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
        assert_eq!(parse_pr(raw, "mayfieldiv/legit").state, PRState::Closed);
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
        assert_eq!(parse_pr(raw, "mayfieldiv/legit").state, PRState::Merged);
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
        assert_eq!(parse_pr(raw, "mayfieldiv/legit").head_repository_owner, "");
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
        let pr = parse_pr(raw, "mayfieldiv/legit");
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
                { "name": "build", "status": "completed", "conclusion": "success",
                  "started_at": "2026-05-01T00:00:00Z", "completed_at": "2026-05-01T00:02:30Z" },
                { "name": "deploy", "status": "in_progress", "conclusion": null,
                  "started_at": "2026-05-01T00:00:00Z" }
            ] }"#,
        )
        .expect("deserialize");

        let checks = parse_check_runs(raw, &HashMap::new());

        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0].name, "build");
        assert_eq!(checks[0].status, "completed");
        assert_eq!(checks[0].conclusion.as_deref(), Some("success"));
        // No check_suite in the payload -> no workflow name, bare job label.
        assert_eq!(checks[0].workflow_name, None);
        // Both endpoints present -> a derived Check Duration of 2m30s.
        assert_eq!(
            checks[0].duration(),
            Some(chrono::Duration::seconds(150)),
            "completed run carries both timestamps"
        );
        assert_eq!(checks[1].status, "in_progress");
        assert_eq!(checks[1].conclusion, None);
        // Only one endpoint present -> no duration.
        assert_eq!(
            checks[1].duration(),
            None,
            "an in-progress run has no completed_at, so no duration"
        );
    }

    #[test]
    fn check_runs_resolve_workflow_name_from_their_suite() {
        let raw: RawCheckRunsResponse = serde_json::from_str(
            r#"{ "total_count": 2, "check_runs": [
                { "name": "Tests", "status": "completed", "conclusion": "success",
                  "check_suite": { "id": 11 } },
                { "name": "Tests", "status": "completed", "conclusion": "success",
                  "check_suite": { "id": 22 } }
            ] }"#,
        )
        .expect("deserialize");

        // Two suites map to two different workflows, disambiguating the two
        // identically-named "Tests" jobs.
        let workflows = HashMap::from([(11, "ci".to_owned()), (22, "e2e".to_owned())]);
        let checks = parse_check_runs(raw, &workflows);

        assert_eq!(checks[0].workflow_name.as_deref(), Some("ci"));
        assert_eq!(checks[1].workflow_name.as_deref(), Some("e2e"));
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
