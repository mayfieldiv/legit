//! GitHub wayfinder transport — the ticket surface's one GitHub interface.
//!
//! [`Wayfinder`] exposes two operations, each exactly one HTTP request so
//! the command layer's limiter stays canonical (one permit wraps one call):
//! [`Wayfinder::read_efforts`], the whole-map GraphQL read (the N+1
//! collapse, spec §4.1), and [`Wayfinder::refresh_ticket`], the
//! single-issue REST refresh. Which transport serves which read, the map
//! label, the wire shapes, normalization into [`Effort`], and the map-body
//! dialect rules are all implementation.
//!
//! Parsing is split into pure functions (`parse_wayfinder_maps`,
//! `parse_issue`) tested directly against fixture JSON — the same posture
//! as `github::rest::parse_pr`.

// TODO(#120): remove once the fetch layer dispatches map reads and ticket
// refreshes.
#![allow(dead_code)]

use std::collections::HashSet;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::json;

use crate::{
    github::{
        graphql::{GraphQlClient, GraphQlError, GraphQlErrors, GraphQlRequest, RawPageInfo},
        rest::OctocrabRest,
        types::{
            DependenciesSummary, Issue, IssueState, SubIssuesSummary, claim_from_assignees,
            ticket_type_from_labels,
        },
    },
    repo_slug::RepoSlug,
    secret::Secret,
    ticket::{Dependency, Effort, EffortKey, ExternalDependency, Ticket, TicketKey},
};

/// The label that marks a GitHub issue as a wayfinder Map. Owned here — no
/// caller decides (or sees) how maps are discovered.
const MAP_LABEL: &str = "wayfinder:map";

/// The GitHub reader for the wayfinder ticket surface. Holds only the token;
/// each method builds its transport per call (the command layer's idiom) and
/// issues exactly one HTTP request, so the caller's `request(...)` wrapper
/// stays one permit per call.
pub struct Wayfinder {
    token: Secret<String>,
}

impl Wayfinder {
    pub fn new(token: &Secret<String>) -> Self {
        Self {
            token: token.clone(),
        }
    }

    /// One whole-map read: every open wayfinder map in `slug`, with
    /// sub-issues and their blockers, normalized into Efforts. The N+1
    /// collapse that puts this on GraphQL: one query, measured cost 10 of
    /// 5,000 points/hr, flat in map size (spec §4.1).
    #[tracing::instrument(name = "read_efforts", skip(self))]
    pub async fn read_efforts(&self, slug: &RepoSlug) -> Result<EffortReadBatch> {
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
            variables: json!({ "owner": slug.owner(), "repo": slug.name(), "label": MAP_LABEL }),
        };
        let response: WayfinderMapResponse = GraphQlClient::new(&self.token)?.post(&body).await?;
        if let Some(rate) = response.data.as_ref().and_then(|d| d.rate_limit.as_ref()) {
            tracing::debug!(
                cost = rate.cost,
                remaining = rate.remaining,
                "map read cost"
            );
        }
        let batch = parse_wayfinder_maps(response, slug);
        if batch.more_maps {
            // The fixed query reads one `first:10` window; a repo with more
            // open maps gets the surplus reported, not silently dropped.
            tracing::warn!(%slug, "more than 10 open wayfinder maps; reading the first 10");
        }
        Ok(batch)
    }

    /// The single-ticket refresh (spec §4.1): state, labels, assignees, both
    /// summaries, `parent_issue_url`, and the body all ride on one
    /// `GET /issues/{n}`. Never call `GET …/parent`: it 404s on a parentless
    /// issue, while `parent_issue_url` already answers parentage — so every
    /// 404 here stays a genuine error (including a token without Issues read
    /// scope, which surfaces as 404, not 403).
    #[tracing::instrument(name = "refresh_ticket", skip(self))]
    pub async fn refresh_ticket(&self, slug: &RepoSlug, number: u64) -> Result<Issue> {
        let route = format!("/repos/{slug}/issues/{number}");
        let raw: RawRestIssue = OctocrabRest::new(&self.token)?
            .get_resource(&route)
            .await
            .with_context(|| format!("fetching issue {slug}#{number}"))?;
        Ok(parse_issue(raw))
    }
}

// ── whole-map read: wire shapes ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct WayfinderMapResponse {
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

// ── whole-map read: normalization ────────────────────────────────────────────

/// One map's outcome from the whole-map read — the §5.5 per-Effort
/// degradation boundary made structural: a map is either a fully normalized
/// Effort or visibly degraded, never a silently partial one.
#[derive(Debug)]
pub enum EffortRead {
    /// The map normalized cleanly.
    Ready(Effort),
    /// The map couldn't be represented as a complete Effort — a
    /// normalization failure, or the §4.4 body-line fallback dialect
    /// (detected, never parsed in v1). Identity and map context survive the
    /// failure so the effort card can render its error line (§5.5) with a
    /// real title instead of a bare number.
    Degraded {
        key: EffortKey,
        title: String,
        destination: Option<String>,
        /// Human-readable cause, shown on the card.
        reason: String,
    },
}

/// Every open wayfinder map in one repo — the result of one map read, in
/// GitHub's order.
#[derive(Debug)]
pub struct EffortReadBatch {
    pub efforts: Vec<EffortRead>,
    /// The repo has more open maps than the query's `first:10` window. The
    /// query is fixed (spec §4.1), so the surplus is reported, not fetched.
    pub more_maps: bool,
}

/// Parse a whole-map response into per-map [`EffortRead`]s. Blocked-ness
/// inputs come from each ticket's `blockedBy` list (filtered on state at
/// derivation time) — never from the eventually-consistent
/// `issueDependenciesSummary` counters, which this parse doesn't even read.
fn parse_wayfinder_maps(response: WayfinderMapResponse, slug: &RepoSlug) -> EffortReadBatch {
    let connection = response
        .data
        .and_then(|d| d.repository)
        .and_then(|r| r.issues);
    let Some(connection) = connection else {
        return EffortReadBatch {
            efforts: Vec::new(),
            more_maps: false,
        };
    };
    EffortReadBatch {
        efforts: connection
            .nodes
            .into_iter()
            .map(|node| read_map_node(node, slug))
            .collect(),
        more_maps: connection.page_info.has_next_page,
    }
}

fn read_map_node(node: RawMapNode, slug: &RepoSlug) -> EffortRead {
    let key = EffortKey::GitHub {
        repo_slug: slug.clone(),
        map_number: node.number,
    };
    let title = node.title;
    let destination = destination_from_map_body(&node.body);
    // Absent ≠ present-empty: the query always selects these connections, so
    // a payload without one is malformed, and treating it as empty would
    // manufacture facts (an empty Effort here; unclaimed / unblocked tickets
    // in `parse_ticket_node`) that can put a ticket falsely on the Frontier.
    // No conservative reading exists, so the map degrades (§5.5).
    let Some(sub_issues) = node.sub_issues else {
        return EffortRead::Degraded {
            key,
            title,
            destination,
            reason: "payload missing subIssues connection".to_owned(),
        };
    };
    // `first:100` is the hard sub-issue cap per parent, so a next page
    // "can't" exist; if it ever does, say so rather than silently showing a
    // partial Effort.
    if sub_issues.page_info.has_next_page {
        tracing::warn!(map = node.number, "sub-issue list truncated at 100");
    }
    let ticket_nodes = sub_issues.nodes;
    // Same-effort membership is "is a sub-issue of this map", not "lives in
    // this repo": a same-repo blocker outside the map stays External.
    let members: HashSet<u64> = ticket_nodes.iter().map(|t| t.number).collect();
    let tickets: Vec<Ticket> = match ticket_nodes
        .into_iter()
        .map(|t| parse_ticket_node(t, slug, &members))
        .collect::<Result<_>>()
    {
        Ok(tickets) => tickets,
        Err(error) => {
            return EffortRead::Degraded {
                key,
                title,
                destination,
                reason: format!("{error:#}"),
            };
        }
    };
    // Zero native sub-issues + a task-list body = the fallback dialect,
    // which v1 detects but never parses (§4.4) — degraded, not an
    // innocently empty Effort.
    if tickets.is_empty() && body_has_task_list(&node.body) {
        return EffortRead::Degraded {
            key,
            title,
            destination,
            reason: "task-list map (fallback dialect) — not parsed in v1".to_owned(),
        };
    }
    match Effort::new(key.clone(), title.clone(), destination.clone(), tickets) {
        Ok(effort) => EffortRead::Ready(effort),
        Err(error) => EffortRead::Degraded {
            key,
            title,
            destination,
            reason: format!("{error:#}"),
        },
    }
}

/// Normalize one sub-issue node. Errs when a Frontier-authoritative
/// connection (`assignees`, `blockedBy`) is absent — treating absence as
/// empty would read as unclaimed/unblocked and could manufacture a false
/// Frontier Ticket, so the whole map degrades instead (see `read_map_node`).
/// `labels` stays defaulted: Type only feeds the Mode filter, never the
/// Frontier, so an absent list is safely an empty Type.
fn parse_ticket_node(
    node: RawTicketNode,
    slug: &RepoSlug,
    members: &HashSet<u64>,
) -> Result<Ticket> {
    let assignees = node.assignees.with_context(|| {
        format!(
            "ticket #{} payload missing assignees connection",
            node.number
        )
    })?;
    let blocked_by = node.blocked_by.with_context(|| {
        format!(
            "ticket #{} payload missing blockedBy connection",
            node.number
        )
    })?;
    let claim = claim_from_assignees(assignees.nodes.into_iter().map(|assignee| assignee.login));
    let labels = node.labels.map(|c| c.nodes).unwrap_or_default();
    let ty = ticket_type_from_labels(labels.iter().map(|label| label.name.as_str()));
    let mut dependencies: Vec<Dependency> = blocked_by
        .nodes
        .into_iter()
        .map(|blocker| parse_blocker(blocker, slug, members))
        .collect();
    // `first:50` is GitHub's hard relation cap, so this "can't" be true —
    // but unseen blockers must never put a ticket on the Frontier, so a
    // truncated list degrades to an Unknown Dependency.
    if blocked_by.page_info.has_next_page {
        dependencies.push(Dependency::Unknown {
            raw: "additional blockers".to_owned(),
        });
    }
    Ok(Ticket {
        key: TicketKey::GitHub {
            repo_slug: slug.clone(),
            number: node.number,
        },
        title: node.title,
        state: IssueState::parse(node.state.as_deref()).into(),
        claim,
        ty,
        dependencies,
    })
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

// ── map-body dialect rules ───────────────────────────────────────────────────

/// The first paragraph under the map body's `Destination` heading (any
/// heading level — the wayfinder template says `##`, real maps drift) — the
/// one-liner the effort card and ticket detail header show. Wrapped lines are
/// joined; `None` when the body has no such heading or the section is empty.
fn destination_from_map_body(body: &str) -> Option<String> {
    let mut lines = body.lines();
    lines.by_ref().find(|line| {
        let trimmed = line.trim();
        let level = trimmed.bytes().take_while(|&b| b == b'#').count();
        (1..=6).contains(&level) && trimmed[level..].trim().eq_ignore_ascii_case("destination")
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

// ── single-ticket refresh: wire shape ────────────────────────────────────────

/// Permissive wire shape for a GitHub issue, from the single-issue endpoint
/// (`GET /repos/:owner/:repo/issues/:number`). Same posture as `RawRestPR`:
/// everything GitHub may omit is optional or defaulted. Private — the
/// module's contract is `Issue`.
#[derive(Debug, Deserialize)]
struct RawRestIssue {
    number: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    labels: Vec<RawLabelName>,
    #[serde(default)]
    assignees: Vec<RawLogin>,
    #[serde(default)]
    sub_issues_summary: SubIssuesSummary,
    #[serde(default)]
    issue_dependencies_summary: DependenciesSummary,
    #[serde(default)]
    parent_issue_url: Option<String>,
}

/// Parse a raw REST issue into the domain `Issue`. Pure; tested directly.
fn parse_issue(raw: RawRestIssue) -> Issue {
    Issue {
        number: raw.number,
        title: raw.title,
        state: IssueState::parse(raw.state.as_deref()),
        url: raw.html_url,
        body: raw.body.unwrap_or_default(),
        labels: raw.labels.into_iter().map(|l| l.name).collect(),
        assignees: raw.assignees.into_iter().map(|u| u.login).collect(),
        sub_issues_summary: raw.sub_issues_summary,
        dependencies_summary: raw.issue_dependencies_summary,
        parent_issue_url: raw.parent_issue_url,
    }
}

#[cfg(test)]
mod tests;
