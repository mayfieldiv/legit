//! GitHub wayfinder transport — the ticket surface's one GitHub interface.
//!
//! [`Wayfinder`] exposes two operations, each exactly one HTTP request so
//! the command layer's limiter stays canonical (one permit wraps one call):
//! [`Wayfinder::read_efforts`], the whole-map GraphQL read (the N+1
//! collapse, spec §4.1), and [`Wayfinder::refresh_ticket`], the
//! single-issue REST refresh. Which transport serves which read, the map
//! label, the wire shapes, normalization into [`Effort`], and the map-body
//! dialect rules are all implementation.

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
    /// sub-issues and their Dependencies, normalized into Efforts. The N+1
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
        let batch = parse_wayfinder_maps(response, slug)?;
        if batch.has_more_maps {
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
    pub async fn refresh_ticket(&self, slug: &RepoSlug, number: u64) -> Result<TicketRefresh> {
        let route = format!("/repos/{slug}/issues/{number}");
        let raw: RawRestIssue = OctocrabRest::new(&self.token)?
            .get_resource(&route)
            .await
            .with_context(|| format!("fetching issue {slug}#{number}"))?;
        Ok(parse_refresh(raw))
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
    issues: Option<RawConnection<RawMapNode>>,
}

/// A paged GraphQL connection as the map query selects it. `nodes` is
/// `Option`, never defaulted to empty: for these lists absent ≠
/// present-empty (a silently-empty authoritative list manufactures facts,
/// §5.5), so each caller decides how absence degrades.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawConnection<T> {
    page_info: RawPageInfo,
    // No `#[serde(default)]`: it would force a spurious `T: Default` bound
    // (serde-rs/serde#1541); an `Option` field is already `None` when absent.
    nodes: Option<Vec<T>>,
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
    sub_issues: Option<RawConnection<RawTicketNode>>,
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
    blocked_by: Option<RawConnection<RawBlockedByNode>>,
}

#[derive(Debug, Deserialize)]
struct RawLoginConnection {
    #[serde(default)]
    nodes: Option<Vec<RawLogin>>,
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
struct RawBlockedByNode {
    number: u64,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    title: Option<String>,
    /// The dependency target's home repo. Read from the payload, never
    /// assumed: a target in another repo is an External Dependency.
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
    pub has_more_maps: bool,
}

/// Parse a whole-map response into per-map [`EffortRead`]s. Blocked-ness
/// inputs come from each ticket's `blockedBy` list (filtered on state at
/// derivation time) — never from the eventually-consistent
/// `issueDependenciesSummary` counters, which this parse doesn't even read.
/// Errs when the payload lacks the `repository.issues` connection or its
/// `nodes`: with GraphQL-level errors already surfaced, such a payload is
/// malformed, and an empty batch would be indistinguishable from a repo
/// with no maps — a startup failure must never be silently missing (§5.5).
fn parse_wayfinder_maps(
    response: WayfinderMapResponse,
    slug: &RepoSlug,
) -> Result<EffortReadBatch> {
    let connection = response
        .data
        .and_then(|d| d.repository)
        .and_then(|r| r.issues)
        .context("payload missing repository.issues connection")?;
    let nodes = connection
        .nodes
        .context("payload missing repository.issues nodes")?;
    Ok(EffortReadBatch {
        efforts: nodes
            .into_iter()
            .map(|node| read_map_node(node, slug))
            .collect(),
        has_more_maps: connection.page_info.has_next_page,
    })
}

fn read_map_node(node: RawMapNode, slug: &RepoSlug) -> EffortRead {
    let key = EffortKey::GitHub {
        repo_slug: slug.clone(),
        map_number: node.number,
    };
    let title = node.title;
    let body_facts = scan_map_body(&node.body);
    let destination = body_facts.destination;
    match normalize_map(
        node.sub_issues,
        body_facts.has_task_list,
        &key,
        &title,
        &destination,
        slug,
    ) {
        Ok(effort) => EffortRead::Ready(effort),
        Err(reason) => EffortRead::Degraded {
            key,
            title,
            destination,
            reason,
        },
    }
}

/// The fallible half of one map's normalization. An `Err` is the
/// human-readable degradation reason the effort card renders (§5.5).
fn normalize_map(
    sub_issues: Option<RawConnection<RawTicketNode>>,
    has_task_list: bool,
    key: &EffortKey,
    title: &str,
    destination: &Option<String>,
    slug: &RepoSlug,
) -> Result<Effort, String> {
    // Absent ≠ present-empty: the query always selects these connections and
    // their `nodes`, so a payload missing either is malformed, and treating
    // it as empty would manufacture facts (an empty Effort here; unclaimed /
    // unblocked tickets in `parse_ticket_node`) that can put a ticket falsely
    // on the Frontier. No conservative reading exists, so the map degrades
    // (§5.5).
    let sub_issues = sub_issues.ok_or_else(|| "payload missing subIssues connection".to_owned())?;
    // `first:100` is the hard sub-issue cap per parent, so a next page
    // "can't" exist; if it ever does, a partial ticket set would silently
    // drop tickets and misread the missing ones' same-effort Dependencies
    // as External — no conservative reading, so the map degrades (§5.5).
    if sub_issues.page_info.has_next_page {
        return Err("sub-issue list truncated at GitHub's 100-per-parent cap".to_owned());
    }
    let ticket_nodes = sub_issues
        .nodes
        .ok_or_else(|| "payload missing subIssues nodes".to_owned())?;
    // Same-effort membership is "is a sub-issue of this map", not "lives in
    // this repo": a same-repo dependency target outside the map stays
    // External.
    let members: HashSet<u64> = ticket_nodes.iter().map(|t| t.number).collect();
    let tickets: Vec<Ticket> = ticket_nodes
        .into_iter()
        .map(|t| parse_ticket_node(t, slug, &members))
        .collect::<Result<_>>()
        .map_err(|error| format!("{error:#}"))?;
    // Zero native sub-issues + a task-list body = the fallback dialect,
    // which v1 detects but never parses (§4.4) — degraded, not an
    // innocently empty Effort.
    if tickets.is_empty() && has_task_list {
        return Err("task-list map (fallback dialect) — not parsed in v1".to_owned());
    }
    Effort::new(key.clone(), title.to_owned(), destination.clone(), tickets)
        .map_err(|error| format!("{error:#}"))
}

/// Normalize one sub-issue node. Errs when a Frontier-authoritative list
/// (`assignees`, `blockedBy`) is absent at either level, connection or
/// `nodes` — treating absence as empty would read as unclaimed/unblocked
/// and could manufacture a false Frontier Ticket, so the whole map degrades
/// instead (see `read_map_node`). `labels` stays defaulted: Type only feeds
/// the Mode filter, never the Frontier, so an absent list is safely an
/// empty Type.
fn parse_ticket_node(
    node: RawTicketNode,
    slug: &RepoSlug,
    members: &HashSet<u64>,
) -> Result<Ticket> {
    let assignees = node
        .assignees
        .and_then(|connection| connection.nodes)
        .with_context(|| format!("ticket #{} payload missing assignees", node.number))?;
    let blocked_by = node.blocked_by.with_context(|| {
        format!(
            "ticket #{} payload missing blockedBy connection",
            node.number
        )
    })?;
    let blocked_by_truncated = blocked_by.page_info.has_next_page;
    let blocked_by_nodes = blocked_by
        .nodes
        .with_context(|| format!("ticket #{} payload missing blockedBy nodes", node.number))?;
    let claim = claim_from_assignees(assignees.into_iter().map(|assignee| assignee.login));
    let labels = node.labels.map(|c| c.nodes).unwrap_or_default();
    let ty = ticket_type_from_labels(labels.iter().map(|label| label.name.as_str()));
    let mut dependencies: Vec<Dependency> = blocked_by_nodes
        .into_iter()
        .map(|target| parse_dependency(target, slug, members))
        .collect();
    // `first:50` is GitHub's hard relation cap, so this "can't" be true —
    // but unseen Dependencies must never put a ticket on the Frontier, so
    // a truncated list degrades to an Unknown Dependency.
    if blocked_by_truncated {
        dependencies.push(Dependency::Unknown {
            raw: "additional dependencies".to_owned(),
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

fn parse_dependency(node: RawBlockedByNode, slug: &RepoSlug, members: &HashSet<u64>) -> Dependency {
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

/// What one Markdown parse of a map body yields: the Destination one-liner
/// and the task-list half of the §4.4 fallback signal.
struct MapBodyFacts {
    /// The first paragraph under a `Destination` heading (any heading level,
    /// ATX or setext — the wayfinder template says `##`, real maps drift),
    /// wrapped lines joined. `None` when the body has no such heading or the
    /// section is empty.
    destination: Option<String>,
    /// The body carries a GitHub task list (a real one — a `- [ ]` line
    /// inside a code fence is not a task list).
    has_task_list: bool,
}

/// Scan a map body once with `pulldown-cmark` — the renderer's own parser,
/// so heading and fence recognition can't drift from what the user sees.
fn scan_map_body(body: &str) -> MapBodyFacts {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

    enum DestinationScan {
        SearchingForHeading,
        CollectingHeadingText(String),
        AwaitingParagraph,
        CapturingParagraph(String),
        Done(Option<String>),
    }
    use DestinationScan::*;

    let mut scan = SearchingForHeading;
    let mut has_task_list = false;
    for event in Parser::new_ext(body, Options::ENABLE_TASKLISTS) {
        if matches!(event, Event::TaskListMarker(_)) {
            has_task_list = true;
        }
        scan = match (scan, &event) {
            (SearchingForHeading, Event::Start(Tag::Heading { .. })) => {
                CollectingHeadingText(String::new())
            }
            (CollectingHeadingText(mut text), Event::Text(t) | Event::Code(t)) => {
                text.push_str(t);
                CollectingHeadingText(text)
            }
            (CollectingHeadingText(text), Event::End(TagEnd::Heading(_))) => {
                if text.trim().eq_ignore_ascii_case("destination") {
                    AwaitingParagraph
                } else {
                    SearchingForHeading
                }
            }
            // A heading before any paragraph: the Destination section is empty.
            (AwaitingParagraph, Event::Start(Tag::Heading { .. })) => Done(None),
            (AwaitingParagraph, Event::Start(Tag::Paragraph)) => CapturingParagraph(String::new()),
            (CapturingParagraph(mut text), Event::Text(t) | Event::Code(t)) => {
                text.push_str(t);
                CapturingParagraph(text)
            }
            (CapturingParagraph(mut text), Event::SoftBreak | Event::HardBreak) => {
                text.push(' ');
                CapturingParagraph(text)
            }
            (CapturingParagraph(text), Event::End(TagEnd::Paragraph)) => {
                let trimmed = text.trim();
                Done((!trimmed.is_empty()).then(|| trimmed.to_owned()))
            }
            (state, _) => state,
        };
    }
    MapBodyFacts {
        destination: match scan {
            Done(destination) => destination,
            _ => None,
        },
        has_task_list,
    }
}

/// Which of the fallback dialect's dependency lines open a ticket body —
/// the ticket-body half of the §4.4 signal, kept per line kind so
/// native-wins can compare each line with its own native representation.
#[derive(Debug, Default, PartialEq)]
struct FallbackLines {
    /// A leading `Part of #n` line.
    part_of: bool,
    /// A leading `Blocked by: #n` line.
    blocked_by: bool,
}

/// Scan a ticket body for the fallback dialect's dependency lines. Parsed
/// as Markdown: only text before the first `##`-or-deeper heading counts,
/// and code blocks never match, so a ref in a fenced example or under a
/// heading doesn't false-trigger. A bare "blocked by" with no issue ref
/// doesn't either. Detection only — the lines are never parsed into the
/// model.
fn scan_fallback_lines(body: &str) -> FallbackLines {
    use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

    fn classify(line: &str, found: &mut FallbackLines) {
        let trimmed = line.trim();
        if !line_has_issue_ref(trimmed) {
            return;
        }
        let lower = trimmed.to_ascii_lowercase();
        found.part_of |= lower.starts_with("part of ");
        found.blocked_by |= lower.starts_with("blocked by:");
    }

    // Accumulate one rendered line at a time (soft/hard breaks and block
    // ends both end a line), skipping code blocks and heading text.
    let mut found = FallbackLines::default();
    let mut line = String::new();
    let mut skipping = 0u32;
    for event in Parser::new_ext(body, Options::empty()) {
        match event {
            Event::Start(Tag::Heading { level, .. }) if level > HeadingLevel::H1 => {
                return found;
            }
            Event::Start(Tag::Heading { .. }) | Event::Start(Tag::CodeBlock(_)) => {
                skipping += 1;
            }
            Event::End(TagEnd::Heading(_)) | Event::End(TagEnd::CodeBlock) => {
                skipping -= 1;
            }
            Event::Text(text) | Event::Code(text) if skipping == 0 => {
                line.push_str(&text);
            }
            Event::SoftBreak
            | Event::HardBreak
            | Event::End(TagEnd::Paragraph)
            | Event::End(TagEnd::Item) => {
                classify(&line, &mut found);
                line.clear();
            }
            _ => {}
        }
    }
    classify(&line, &mut found);
    found
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

/// One ticket's refresh outcome: the issue plus the §4.4 fallback-dialect
/// verdict, decided here so no caller learns the body dialect.
#[derive(Debug)]
pub struct TicketRefresh {
    pub issue: Issue,
    /// The body opens with a fallback dependency line whose native
    /// counterpart is absent — `Part of #n` with no native parent, or
    /// `Blocked by: #n` with no native dependencies — so the Effort shows
    /// a degradation notice (§4.4). Native-wins is per representation: a
    /// line whose own native counterpart exists is advisory and never sets
    /// this.
    pub fallback_dialect: bool,
}

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

fn parse_refresh(raw: RawRestIssue) -> TicketRefresh {
    let issue = parse_issue(raw);
    let lines = scan_fallback_lines(&issue.body);
    // Native-wins compares each line kind with its own representation
    // (§4.4): a native parent says nothing about a `Blocked by:` line, and
    // vice versa. `total_blocked_by` counts open and closed native
    // dependencies, so zero really means "none exist", not "all closed".
    let fallback_dialect = (lines.part_of && issue.parent_issue_url.is_none())
        || (lines.blocked_by && issue.dependencies_summary.total_blocked_by == 0);
    TicketRefresh {
        issue,
        fallback_dialect,
    }
}

#[cfg(test)]
mod tests;
