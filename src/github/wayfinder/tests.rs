use super::{
    EffortReadBatch, FallbackLines, RawRestIssue, TicketRefresh, WayfinderMapResponse, parse_issue,
    parse_refresh, parse_wayfinder_maps, scan_fallback_lines, scan_map_body,
};
use crate::github::graphql::ensure_no_errors;
use crate::github::types::{DependenciesSummary, Issue, IssueState, SubIssuesSummary};
use crate::repo_slug::RepoSlug;
use crate::ticket::{
    Claim, Dependency, Effort, EffortKey, EffortRead, ExternalDependency, TicketKey, TicketState,
    TicketType,
};

fn map_slug() -> RepoSlug {
    RepoSlug::new("mayfieldiv/legit")
}

fn parse_batch(raw: &str) -> anyhow::Result<EffortReadBatch> {
    let response: WayfinderMapResponse = serde_json::from_str(raw).expect("deserialize");
    parse_wayfinder_maps(response, &map_slug())
}

fn same_effort_key(number: u64) -> TicketKey {
    TicketKey::GitHub {
        repo_slug: map_slug(),
        number,
    }
}

fn ready(read: &EffortRead) -> &Effort {
    match read {
        EffortRead::Ready(effort) => effort,
        degraded => panic!("expected Ready, got {degraded:?}"),
    }
}

fn degraded_reason(read: &EffortRead) -> &str {
    match read {
        EffortRead::Degraded { reason, .. } => reason,
        ready => panic!("expected Degraded, got {ready:?}"),
    }
}

#[test]
fn parses_wayfinder_map_into_effort() {
    let raw = r###"{ "data": {
        "rateLimit": { "cost": 10, "remaining": 4990, "resetAt": "2026-08-28T21:00:00Z" },
        "repository": { "issues": {
            "pageInfo": { "hasNextPage": false, "endCursor": null },
            "nodes": [ {
                "number": 123,
                "title": "Map: ticket surface",
                "state": "OPEN",
                "url": "https://github.com/mayfieldiv/legit/issues/123",
                "body": "## Destination\n\nAll eight issues merged to main.\n\n## Notes\n\n- execution map\n",
                "subIssuesSummary": { "total": 3, "completed": 1, "percentCompleted": 33 },
                "subIssues": {
                    "pageInfo": { "hasNextPage": false, "endCursor": null },
                    "nodes": [
                        {
                            "number": 116, "title": "Domain types", "state": "CLOSED",
                            "stateReason": "COMPLETED",
                            "url": "u116",
                            "assignees": { "nodes": [ { "login": "mayfieldiv" } ] },
                            "labels": { "nodes": [
                                { "name": "ready-for-agent" }, { "name": "wayfinder:task" }
                            ] },
                            "issueDependenciesSummary": { "blockedBy": 0, "blocking": 2, "totalBlockedBy": 0, "totalBlocking": 2 },
                            "blockedBy": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [] }
                        },
                        {
                            "number": 117, "title": "GitHub transport", "state": "OPEN",
                            "stateReason": null,
                            "url": "u117",
                            "assignees": { "nodes": [] },
                            "labels": { "nodes": [ { "name": "wayfinder:task" } ] },
                            "issueDependenciesSummary": { "blockedBy": 0, "blocking": 1, "totalBlockedBy": 1, "totalBlocking": 1 },
                            "blockedBy": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [
                                { "number": 116, "state": "CLOSED", "title": "Domain types",
                                  "repository": { "nameWithOwner": "mayfieldiv/legit" } }
                            ] }
                        },
                        {
                            "number": 120, "title": "Fetch integration", "state": "OPEN",
                            "stateReason": null,
                            "url": "u120",
                            "assignees": { "nodes": [] },
                            "labels": { "nodes": [ { "name": "question" } ] },
                            "issueDependenciesSummary": { "blockedBy": 3, "blocking": 0, "totalBlockedBy": 3, "totalBlocking": 0 },
                            "blockedBy": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [
                                { "number": 117, "state": "OPEN", "title": "GitHub transport",
                                  "repository": { "nameWithOwner": "MayfieldIV/Legit" } },
                                { "number": 999, "state": "OPEN", "title": "External thing",
                                  "repository": { "nameWithOwner": "other/repo" } },
                                { "number": 55, "state": "CLOSED", "title": "Same repo, not in map",
                                  "repository": { "nameWithOwner": "mayfieldiv/legit" } }
                            ] }
                        }
                    ]
                }
            } ]
        } }
    } }"###;
    let batch = parse_batch(raw).expect("parse");

    assert!(!batch.has_more_maps);
    assert_eq!(batch.efforts.len(), 1);
    let effort = ready(&batch.efforts[0]);
    assert_eq!(
        effort.key,
        EffortKey::GitHub {
            repo_slug: map_slug(),
            map_number: 123,
        }
    );
    assert_eq!(effort.title, "Map: ticket surface");
    assert_eq!(
        effort.destination.as_deref(),
        Some("All eight issues merged to main.")
    );

    let tickets: Vec<_> = effort.tickets().collect();
    assert_eq!(tickets.len(), 3);

    assert_eq!(tickets[0].key, same_effort_key(116));
    assert_eq!(tickets[0].title, "Domain types");
    assert_eq!(tickets[0].state, TicketState::Closed);
    assert_eq!(tickets[0].claim, Some(Claim::By("mayfieldiv".to_owned())));
    assert_eq!(
        tickets[0].ty,
        TicketType("task".to_owned()),
        "the Type is the wayfinder:<type> label, prefix stripped — triage labels never masquerade as a Type"
    );
    assert!(tickets[0].dependencies.is_empty());

    assert_eq!(tickets[1].claim, None);
    assert_eq!(
        tickets[1].dependencies,
        vec![Dependency::SameEffort(same_effort_key(116))],
        "a closed same-effort Dependency is kept (the detail page shows it ✓)"
    );

    assert_eq!(
        tickets[2].ty,
        TicketType(String::new()),
        "no wayfinder: label yields an empty Type, shown verbatim, Mode Either"
    );
    assert_eq!(
        tickets[2].dependencies,
        vec![
            Dependency::SameEffort(same_effort_key(117)),
            Dependency::External(ExternalDependency {
                key: TicketKey::GitHub {
                    repo_slug: RepoSlug::new("other/repo"),
                    number: 999,
                },
                state: TicketState::Open,
                title: Some("External thing".to_owned()),
            }),
            Dependency::External(ExternalDependency {
                key: same_effort_key(55),
                state: TicketState::Closed,
                title: Some("Same repo, not in map".to_owned()),
            }),
        ],
        "same-effort = a sub-issue of this map, case-insensitively same repo; anything else is \
         External (never Unknown) with the payload's state and title captured — even a same-repo \
         issue outside the map"
    );

    assert!(
        effort
            .ticket(&same_effort_key(117))
            .unwrap()
            .is_on_frontier(),
        "blocked-ness derives from the dependency list filtered on state — a closed Dependency \
         target must not block"
    );
    assert!(
        effort.ticket(&same_effort_key(120)).unwrap().is_blocked(),
        "an open same-effort Dependency blocks"
    );
}

#[test]
fn map_parse_flags_more_maps_beyond_first_page() {
    let raw = r#"{ "data": { "repository": { "issues": {
        "pageInfo": { "hasNextPage": true, "endCursor": "c1" },
        "nodes": []
    } } } }"#;
    let batch = parse_batch(raw).expect("parse");

    assert!(batch.has_more_maps);
}

#[test]
fn unreadable_dependency_repo_degrades_to_unknown_dependency() {
    let raw = r#"{ "data": { "repository": { "issues": {
        "pageInfo": { "hasNextPage": false, "endCursor": null },
        "nodes": [ {
            "number": 1, "title": "m", "state": "OPEN", "url": "u", "body": "",
            "subIssuesSummary": { "total": 1, "completed": 0, "percentCompleted": 0 },
            "subIssues": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [ {
                "number": 2, "title": "t", "state": "OPEN",
                "assignees": { "nodes": [] }, "labels": { "nodes": [] },
                "blockedBy": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [
                    { "number": 7, "state": "OPEN", "title": "x", "repository": null }
                ] }
            } ] }
        } ]
    } } } }"#;
    let batch = parse_batch(raw).expect("parse");

    let effort = ready(&batch.efforts[0]);
    let ticket = effort.tickets().next().unwrap();
    assert_eq!(
        ticket.dependencies,
        vec![Dependency::Unknown {
            raw: "#7".to_owned()
        }]
    );
    assert!(ticket.is_blocked(), "an Unknown Dependency always blocks");
}

#[test]
fn truncated_blocked_by_list_keeps_ticket_off_the_frontier() {
    // `first:50` is GitHub's hard cap, so this "can't happen" — but if it
    // ever does, unseen Dependencies must not put the ticket on the Frontier.
    let raw = r#"{ "data": { "repository": { "issues": {
        "pageInfo": { "hasNextPage": false, "endCursor": null },
        "nodes": [ {
            "number": 1, "title": "m", "state": "OPEN", "url": "u", "body": "",
            "subIssuesSummary": { "total": 1, "completed": 0, "percentCompleted": 0 },
            "subIssues": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [ {
                "number": 2, "title": "t", "state": "OPEN",
                "assignees": { "nodes": [] }, "labels": { "nodes": [] },
                "blockedBy": { "pageInfo": { "hasNextPage": true, "endCursor": "c" }, "nodes": [] }
            } ] }
        } ]
    } } } }"#;
    let batch = parse_batch(raw).expect("parse");

    let effort = ready(&batch.efforts[0]);
    assert!(effort.tickets().next().unwrap().is_blocked());
}

#[test]
fn truncated_sub_issue_list_degrades_the_map() {
    // `first:100` is GitHub's hard sub-issue cap, so this "can't happen" —
    // but a partial ticket set would silently drop tickets and misread the
    // missing ones' same-effort Dependencies as External, so the map degrades.
    let raw = r#"{ "data": { "repository": { "issues": {
        "pageInfo": { "hasNextPage": false, "endCursor": null },
        "nodes": [ {
            "number": 1, "title": "huge map", "state": "OPEN", "url": "u", "body": "",
            "subIssuesSummary": { "total": 150, "completed": 0, "percentCompleted": 0 },
            "subIssues": { "pageInfo": { "hasNextPage": true, "endCursor": "c" }, "nodes": [ {
                "number": 2, "title": "t", "state": "OPEN",
                "assignees": { "nodes": [] }, "labels": { "nodes": [] },
                "blockedBy": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [] }
            } ] }
        } ]
    } } } }"#;
    let batch = parse_batch(raw).expect("parse");

    let EffortRead::Degraded { title, reason, .. } = &batch.efforts[0] else {
        panic!("expected Degraded, got {:?}", batch.efforts[0]);
    };
    assert_eq!(title, "huge map");
    assert!(
        reason.contains("truncated"),
        "reason names the cause: {reason}"
    );
}

#[test]
fn map_with_task_list_body_and_no_sub_issues_degrades_as_fallback_dialect() {
    let raw = r###"{ "data": { "repository": { "issues": {
        "pageInfo": { "hasNextPage": false, "endCursor": null },
        "nodes": [
            {
                "number": 1, "title": "fallback map", "state": "OPEN", "url": "u",
                "body": "Tickets:\n\n- [x] #2 done thing\n- [ ] #3 open thing\n",
                "subIssuesSummary": { "total": 0, "completed": 0, "percentCompleted": 0 },
                "subIssues": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [] }
            },
            {
                "number": 4, "title": "genuinely empty map", "state": "OPEN", "url": "u",
                "body": "## Destination\n\nNothing charted yet.\n",
                "subIssuesSummary": { "total": 0, "completed": 0, "percentCompleted": 0 },
                "subIssues": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [] }
            }
        ]
    } } } }"###;
    let batch = parse_batch(raw).expect("parse");

    let EffortRead::Degraded {
        key,
        title,
        destination,
        reason,
    } = &batch.efforts[0]
    else {
        panic!("expected Degraded, got {:?}", batch.efforts[0]);
    };
    assert_eq!(
        *key,
        EffortKey::GitHub {
            repo_slug: map_slug(),
            map_number: 1,
        },
        "degradation keeps the map's identity for the effort card's error line"
    );
    assert_eq!(title, "fallback map");
    assert_eq!(*destination, None);
    assert!(
        reason.contains("fallback dialect"),
        "reason names the cause: {reason}"
    );

    let effort = ready(&batch.efforts[1]);
    assert_eq!(
        effort.tickets().count(),
        0,
        "an empty map without a task list is innocently empty — Ready, not Degraded"
    );
    assert_eq!(effort.destination.as_deref(), Some("Nothing charted yet."));
}

#[test]
fn a_malformed_map_degrades_without_discarding_the_others() {
    // Duplicate ticket keys (Effort::new's guard) are the one way
    // normalization can fail; the read's other maps must survive it — parse
    // failures degrade per-Effort, never silently drop (spec §5.5).
    let raw = r#"{ "data": { "repository": { "issues": {
        "pageInfo": { "hasNextPage": false, "endCursor": null },
        "nodes": [
            {
                "number": 1, "title": "broken", "state": "OPEN", "url": "u", "body": "",
                "subIssues": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [
                    { "number": 2, "title": "a", "state": "OPEN",
                      "assignees": { "nodes": [] }, "labels": { "nodes": [] },
                      "blockedBy": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [] } },
                    { "number": 2, "title": "a again", "state": "OPEN",
                      "assignees": { "nodes": [] }, "labels": { "nodes": [] },
                      "blockedBy": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [] } }
                ] }
            },
            {
                "number": 9, "title": "healthy", "state": "OPEN", "url": "u", "body": "",
                "subIssues": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [] }
            }
        ]
    } } } }"#;
    let batch = parse_batch(raw).expect("parse");

    assert_eq!(batch.efforts.len(), 2);
    let EffortRead::Degraded {
        key, title, reason, ..
    } = &batch.efforts[0]
    else {
        panic!("expected Degraded, got {:?}", batch.efforts[0]);
    };
    assert_eq!(
        *key,
        EffortKey::GitHub {
            repo_slug: map_slug(),
            map_number: 1,
        }
    );
    assert_eq!(title, "broken");
    assert!(
        reason.contains("duplicate"),
        "error names the cause: {reason}"
    );
    assert_eq!(
        ready(&batch.efforts[1]).title,
        "healthy",
        "sibling maps survive in GitHub order"
    );
}

#[test]
fn absent_authoritative_connections_degrade_the_map() {
    // Absent ≠ present-empty: reading a missing subIssues/assignees/blockedBy
    // as empty would manufacture unclaimed/unblocked facts and could put a
    // ticket falsely on the Frontier. No conservative reading exists, so the
    // map degrades per-Effort; sibling maps survive.
    let raw = r#"{ "data": { "repository": { "issues": {
        "pageInfo": { "hasNextPage": false, "endCursor": null },
        "nodes": [
            { "number": 1, "title": "no subIssues", "state": "OPEN", "url": "u", "body": "" },
            {
                "number": 2, "title": "no assignees", "state": "OPEN", "url": "u", "body": "",
                "subIssues": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [ {
                    "number": 10, "title": "t", "state": "OPEN",
                    "labels": { "nodes": [] },
                    "blockedBy": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [] }
                } ] }
            },
            {
                "number": 3, "title": "no blockedBy", "state": "OPEN", "url": "u", "body": "",
                "subIssues": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [ {
                    "number": 11, "title": "t", "state": "OPEN",
                    "assignees": { "nodes": [] }, "labels": { "nodes": [] }
                } ] }
            },
            {
                "number": 4, "title": "healthy", "state": "OPEN", "url": "u", "body": "",
                "subIssues": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [ {
                    "number": 12, "title": "t", "state": "OPEN",
                    "assignees": { "nodes": [] },
                    "blockedBy": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [] }
                } ] }
            }
        ]
    } } } }"#;
    let batch = parse_batch(raw).expect("parse");

    assert!(degraded_reason(&batch.efforts[0]).contains("subIssues"));
    assert!(degraded_reason(&batch.efforts[1]).contains("#10 payload missing assignees"));
    assert!(degraded_reason(&batch.efforts[2]).contains("#11 payload missing blockedBy"));
    // `labels` absent stays Ready: Type only feeds the Mode filter, never
    // the Frontier, so an absent list is safely an empty Type.
    let effort = ready(&batch.efforts[3]);
    assert_eq!(
        effort.tickets().next().unwrap().ty,
        TicketType(String::new())
    );
}

#[test]
fn absent_nodes_inside_present_connections_degrade_the_map() {
    // The same absent ≠ present-empty rule one level deeper: a connection
    // whose `nodes` list is missing must not read as an empty list.
    let raw = r#"{ "data": { "repository": { "issues": {
        "pageInfo": { "hasNextPage": false, "endCursor": null },
        "nodes": [
            {
                "number": 1, "title": "subIssues without nodes", "state": "OPEN", "url": "u", "body": "",
                "subIssues": { "pageInfo": { "hasNextPage": false, "endCursor": null } }
            },
            {
                "number": 2, "title": "assignees without nodes", "state": "OPEN", "url": "u", "body": "",
                "subIssues": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [ {
                    "number": 10, "title": "t", "state": "OPEN",
                    "assignees": {},
                    "labels": { "nodes": [] },
                    "blockedBy": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [] }
                } ] }
            },
            {
                "number": 3, "title": "blockedBy without nodes", "state": "OPEN", "url": "u", "body": "",
                "subIssues": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [ {
                    "number": 11, "title": "t", "state": "OPEN",
                    "assignees": { "nodes": [] }, "labels": { "nodes": [] },
                    "blockedBy": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": null }
                } ] }
            }
        ]
    } } } }"#;
    let batch = parse_batch(raw).expect("parse");

    assert!(degraded_reason(&batch.efforts[0]).contains("subIssues nodes"));
    assert!(degraded_reason(&batch.efforts[1]).contains("#10 payload missing assignees"));
    assert!(
        degraded_reason(&batch.efforts[2]).contains("#11 payload missing blockedBy nodes"),
        "an explicit null degrades like a missing key"
    );
}

#[test]
fn issues_connection_without_nodes_is_an_error() {
    let raw = r#"{ "data": { "repository": { "issues": {
        "pageInfo": { "hasNextPage": false, "endCursor": null }
    } } } }"#;
    let err = parse_batch(raw).expect_err("malformed payload");
    assert!(err.to_string().contains("repository.issues nodes"));
}

#[test]
fn payload_without_issues_connection_is_an_error() {
    // With GraphQL-level errors already surfaced by `ensure_no_errors`, a
    // payload lacking `repository.issues` is malformed — parsing it as an
    // empty batch would be indistinguishable from a repo with no maps.
    let raw = r#"{ "data": { "repository": null } }"#;
    let err = parse_batch(raw).expect_err("malformed payload");
    assert!(err.to_string().contains("repository.issues"));
}

#[test]
fn wayfinder_map_errors_surface_as_err() {
    // The envelope must `impl GraphQlErrors`: GitHub reports query failures
    // as HTTP 200 + `errors`, which must not parse as an empty success.
    let raw = r#"{ "data": null, "errors": [ { "message": "Bad credentials" } ] }"#;
    let response: WayfinderMapResponse = serde_json::from_str(raw).expect("deserialize");

    let err = ensure_no_errors(response).expect_err("errors must surface as Err");
    assert!(err.to_string().contains("Bad credentials"));
}

// ── map-body helpers ─────────────────────────────────────────────────────────

fn destination(body: &str) -> Option<String> {
    scan_map_body(body).destination
}

#[test]
fn destination_extracts_first_paragraph_under_the_heading() {
    let body = "## Destination\n\nAll eight issues merged\nand usable in the TUI.\n\nSecond paragraph.\n\n## Notes\n";
    assert_eq!(
        destination(body).as_deref(),
        Some("All eight issues merged and usable in the TUI.")
    );
    assert_eq!(
        destination("# Destination\n\nAny heading level works.\n").as_deref(),
        Some("Any heading level works."),
        "real maps drift from the template's ## level"
    );
    assert_eq!(destination("no heading here"), None);
    assert_eq!(
        destination("## Destination\n\n## Notes\n"),
        None,
        "an empty section yields None, not an empty string"
    );
}

#[test]
fn destination_heading_recognition_is_real_markdown() {
    assert_eq!(
        destination("## Destination ##\n\nClosed ATX heading.\n").as_deref(),
        Some("Closed ATX heading.")
    );
    assert_eq!(
        destination("Destination\n-----------\n\nSetext heading.\n").as_deref(),
        Some("Setext heading.")
    );
    assert_eq!(
        destination("```\n# Destination\n\nnot a destination\n```\n"),
        None,
        "a heading inside a code fence is code, not a heading"
    );
}

#[test]
fn task_list_detection_ignores_code_fences() {
    assert!(scan_map_body("- [ ] #3 open thing\n").has_task_list);
    assert!(
        !scan_map_body("```\n- [ ] #3 fenced example\n```\n").has_task_list,
        "a task list inside a code fence is code, not tickets"
    );
}

#[test]
fn detects_fallback_dependency_lines_in_leading_body_lines() {
    let part_of = |body: &str| scan_fallback_lines(body).part_of;
    let blocked_by = |body: &str| scan_fallback_lines(body).blocked_by;

    assert!(part_of("Part of #123\n\nSome body."));
    assert!(blocked_by("Blocked by: #4, #5\n\nSome body."));
    assert_eq!(
        scan_fallback_lines("Part of #1\nBlocked by: #2"),
        FallbackLines {
            part_of: true,
            blocked_by: true,
        },
        "each line kind is detected independently, never conflated"
    );
    assert!(
        !blocked_by("Part of #1"),
        "a Part of line never reads as Blocked by"
    );
    assert!(
        part_of("part of mayfieldiv/legit#123"),
        "case-insensitive, slug-qualified refs count"
    );
    assert!(
        !part_of("## Question\n\nPart of #123 appears after a heading"),
        "scanning stops at the first heading"
    );
    assert!(
        !blocked_by("Blocked by: the weather"),
        "a dependency line needs an issue ref"
    );
    assert!(!part_of("This ticket is part of the plan."));
    assert!(
        !part_of("```\nPart of #123\n```\n\nSome body."),
        "a dependency line inside a code fence is an example, not a dependency"
    );
    assert!(
        blocked_by("Intro sentence.\nBlocked by: #4\n\n## Notes"),
        "any leading line before the first ## heading counts, not just the first"
    );
}

// ── issue parsing (single-ticket refresh) ────────────────────────────────────

#[test]
fn parses_issue_from_single_issue_endpoint() {
    let raw: RawRestIssue = serde_json::from_str(
        r#"{
            "number": 117,
            "title": "GitHub transport",
            "state": "open",
            "state_reason": null,
            "html_url": "https://github.com/mayfieldiv/legit/issues/117",
            "body": "Implements part of the wayfinder ticket surface.",
            "labels": [
                { "name": "wayfinder:task", "color": "5319E7" },
                { "name": "ready-for-agent", "color": "" }
            ],
            "assignees": [{ "login": "mayfieldiv" }, { "login": "alice" }],
            "sub_issues_summary": { "total": 0, "completed": 0, "percent_completed": 0 },
            "issue_dependencies_summary": {
                "blocked_by": 1, "total_blocked_by": 2,
                "blocking": 3, "total_blocking": 3
            },
            "parent_issue_url": "https://api.github.com/repos/mayfieldiv/legit/issues/123"
        }"#,
    )
    .expect("deserialize");

    let issue = parse_issue(raw);

    assert_eq!(
        issue,
        Issue {
            number: 117,
            title: "GitHub transport".to_owned(),
            state: IssueState::Open,
            url: "https://github.com/mayfieldiv/legit/issues/117".to_owned(),
            body: "Implements part of the wayfinder ticket surface.".to_owned(),
            labels: vec!["wayfinder:task".to_owned(), "ready-for-agent".to_owned()],
            assignees: vec!["mayfieldiv".to_owned(), "alice".to_owned()],
            sub_issues_summary: SubIssuesSummary {
                total: 0,
                completed: 0,
                percent_completed: 0,
            },
            dependencies_summary: DependenciesSummary {
                blocked_by: 1,
                blocking: 3,
                total_blocked_by: 2,
                total_blocking: 3,
            },
            parent_issue_url: Some(
                "https://api.github.com/repos/mayfieldiv/legit/issues/123".to_owned()
            ),
        }
    );
}

#[test]
fn issue_parse_defaults_everything_but_number_and_title() {
    // Permissive posture: a stripped payload (or one from a GHES-ish proxy
    // that omits the newer summary objects) still parses.
    let raw: RawRestIssue =
        serde_json::from_str(r#"{ "number": 5, "title": "bare" }"#).expect("deserialize");

    let issue = parse_issue(raw);

    assert_eq!(issue.number, 5);
    assert_eq!(issue.state, IssueState::Open, "absent state defaults Open");
    assert_eq!(issue.body, "");
    assert!(issue.labels.is_empty());
    assert!(issue.assignees.is_empty());
    assert_eq!(issue.sub_issues_summary, SubIssuesSummary::default());
    assert_eq!(issue.dependencies_summary, DependenciesSummary::default());
    assert_eq!(issue.parent_issue_url, None);
}

#[test]
fn refresh_flags_fallback_lines_lacking_their_native_counterpart() {
    let refresh = |json: &str| -> TicketRefresh {
        parse_refresh(serde_json::from_str(json).expect("deserialize"))
    };

    assert!(
        refresh(r#"{ "number": 7, "title": "t", "body": "Part of #123\n\nBody." }"#)
            .fallback_dialect
    );
    assert!(!refresh(r#"{ "number": 7, "title": "t", "body": "Just a body." }"#).fallback_dialect);
    assert!(
        !refresh(
            r#"{ "number": 7, "title": "t", "body": "Part of #123",
                 "parent_issue_url": "https://api.github.com/repos/o/r/issues/123" }"#
        )
        .fallback_dialect,
        "native parent wins; a stale Part of line is advisory (§4.4)"
    );
    assert!(
        !refresh(
            r#"{ "number": 7, "title": "t", "body": "Blocked by: #4",
                 "issue_dependencies_summary": { "total_blocked_by": 1 } }"#
        )
        .fallback_dialect,
        "native dependencies win over a Blocked by line (§4.4)"
    );
    assert!(
        refresh(
            r#"{ "number": 7, "title": "t", "body": "Blocked by: #4",
                 "parent_issue_url": "https://api.github.com/repos/o/r/issues/123" }"#
        )
        .fallback_dialect,
        "native-wins is per representation: a parent covers Part of, not Blocked by (§4.4)"
    );
    assert!(
        refresh(
            r#"{ "number": 7, "title": "t", "body": "Part of #123",
                 "issue_dependencies_summary": { "total_blocked_by": 1 } }"#
        )
        .fallback_dialect,
        "native-wins is per representation: dependencies cover Blocked by, not Part of (§4.4)"
    );
}

#[test]
fn issue_parse_reads_closed_state() {
    let raw: RawRestIssue =
        serde_json::from_str(r#"{ "number": 6, "title": "done", "state": "closed" }"#)
            .expect("deserialize");

    assert_eq!(parse_issue(raw).state, IssueState::Closed);
}
