use super::{
    ReviewStatusResponse, ThreadsResponse, WayfinderMapResponse, destination_from_map_body,
    ensure_no_errors, has_fallback_dependency_lines, parse_review_status, parse_review_threads,
    parse_wayfinder_maps,
};
use crate::github::types::PRState;
use crate::repo_slug::RepoSlug;
use crate::ticket::{
    Claim, Dependency, EffortKey, ExternalDependency, TicketKey, TicketState, TicketType,
};

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

// ── wayfinder map parsing ────────────────────────────────────────────────────

fn map_slug() -> RepoSlug {
    RepoSlug::new("mayfieldiv/legit")
}

fn same_effort_key(number: u64) -> TicketKey {
    TicketKey::GitHub {
        repo_slug: map_slug(),
        number,
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
    let response: WayfinderMapResponse = serde_json::from_str(raw).expect("deserialize");

    let page = parse_wayfinder_maps(response, &map_slug()).expect("parse");

    assert!(!page.has_more_maps);
    assert_eq!(page.maps.len(), 1);
    let read = &page.maps[0];
    assert!(!read.fallback_dialect);

    let effort = &read.effort;
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
    // The Type is the `wayfinder:<type>` label, prefix stripped; other labels
    // (triage vocabulary) never masquerade as a Type.
    assert_eq!(tickets[0].ty, TicketType("task".to_owned()));
    assert!(tickets[0].dependencies.is_empty());

    assert_eq!(tickets[1].claim, None);
    assert_eq!(
        tickets[1].dependencies,
        vec![Dependency::SameEffort(same_effort_key(116))],
        "a closed same-effort blocker is kept (the detail page shows it ✓)"
    );

    // No `wayfinder:` label at all → an empty Type, shown verbatim, Mode Either.
    assert_eq!(tickets[2].ty, TicketType(String::new()));
    assert_eq!(
        tickets[2].dependencies,
        vec![
            // Same repo (case-insensitively) + a sub-issue of this map.
            Dependency::SameEffort(same_effort_key(117)),
            // Another repo: External, with the captured state and title.
            Dependency::External(ExternalDependency {
                key: TicketKey::GitHub {
                    repo_slug: RepoSlug::new("other/repo"),
                    number: 999,
                },
                state: TicketState::Open,
                title: Some("External thing".to_owned()),
            }),
            // Same repo but not one of this map's sub-issues: also External —
            // the payload's state/title are captured, so lookup must not
            // degrade it to Unknown.
            Dependency::External(ExternalDependency {
                key: same_effort_key(55),
                state: TicketState::Closed,
                title: Some("Same repo, not in map".to_owned()),
            }),
        ]
    );

    // Blocked-ness comes from the dependency list filtered on state — #117's
    // only blocker is closed, so it is on the Frontier; #120 waits on #117.
    assert!(
        effort
            .ticket(&same_effort_key(117))
            .unwrap()
            .is_on_frontier(),
        "closed blockers must not block"
    );
    assert!(effort.ticket(&same_effort_key(120)).unwrap().is_blocked());
}

#[test]
fn map_parse_flags_more_maps_beyond_first_page() {
    let raw = r#"{ "data": { "repository": { "issues": {
        "pageInfo": { "hasNextPage": true, "endCursor": "c1" },
        "nodes": []
    } } } }"#;
    let response: WayfinderMapResponse = serde_json::from_str(raw).expect("deserialize");

    let page = parse_wayfinder_maps(response, &map_slug()).expect("parse");

    assert!(page.has_more_maps);
}

#[test]
fn unreadable_blocker_repo_degrades_to_unknown_dependency() {
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
    let response: WayfinderMapResponse = serde_json::from_str(raw).expect("deserialize");

    let page = parse_wayfinder_maps(response, &map_slug()).expect("parse");

    let effort = &page.maps[0].effort;
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
fn truncated_blocker_list_keeps_ticket_off_the_frontier() {
    // `first:50` is GitHub's hard cap, so this "can't happen" — but if it
    // ever does, the unseen blockers must not put the ticket on the Frontier.
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
    let response: WayfinderMapResponse = serde_json::from_str(raw).expect("deserialize");

    let page = parse_wayfinder_maps(response, &map_slug()).expect("parse");

    let effort = &page.maps[0].effort;
    assert!(effort.tickets().next().unwrap().is_blocked());
}

#[test]
fn map_with_task_list_body_and_no_sub_issues_is_fallback_dialect() {
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
    let response: WayfinderMapResponse = serde_json::from_str(raw).expect("deserialize");

    let page = parse_wayfinder_maps(response, &map_slug()).expect("parse");

    assert!(
        page.maps[0].fallback_dialect,
        "task-list body with zero sub-issues"
    );
    assert!(
        !page.maps[1].fallback_dialect,
        "an empty map without a task list is just empty"
    );
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

#[test]
fn destination_extracts_first_paragraph_under_the_heading() {
    let body = "## Destination\n\nAll eight issues merged\nand usable in the TUI.\n\nSecond paragraph.\n\n## Notes\n";
    assert_eq!(
        destination_from_map_body(body).as_deref(),
        Some("All eight issues merged and usable in the TUI.")
    );
    assert_eq!(destination_from_map_body("no heading here"), None);
    assert_eq!(
        destination_from_map_body("## Destination\n\n## Notes\n"),
        None,
        "an empty section yields None, not an empty string"
    );
}

#[test]
fn detects_fallback_dependency_lines_in_leading_body_lines() {
    assert!(has_fallback_dependency_lines("Part of #123\n\nSome body."));
    assert!(has_fallback_dependency_lines(
        "Blocked by: #4, #5\n\nSome body."
    ));
    assert!(
        has_fallback_dependency_lines("part of mayfieldiv/legit#123"),
        "case-insensitive, slug-qualified refs count"
    );
    assert!(
        !has_fallback_dependency_lines("## Question\n\nPart of #123 appears after a heading"),
        "scanning stops at the first heading"
    );
    assert!(
        !has_fallback_dependency_lines("Blocked by: the weather"),
        "a dependency line needs an issue ref"
    );
    assert!(!has_fallback_dependency_lines(
        "This ticket is part of the plan."
    ));
}

#[test]
fn no_errors_passes_response_through() {
    let raw = r#"{ "data": { "repository": {} } }"#;
    let response: ReviewStatusResponse = serde_json::from_str(raw).expect("deserialize");

    let passed = ensure_no_errors(response).expect("clean response passes through");
    assert!(parse_review_status(passed).is_empty());
}
