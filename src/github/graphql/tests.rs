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
