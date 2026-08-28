use crate::repo_slug::RepoSlug;
use std::collections::HashMap;

use chrono::TimeZone;

use super::{
    Label, PR, PRState, RawCheckRunsResponse, RawIssueComment, RawRestIssue, RawRestPR, RawReview,
    parse_check_runs, parse_issue, parse_issue_comments, parse_issues, parse_pr, parse_reviews,
};
use crate::github::types::{DependenciesSummary, Issue, IssueState, SubIssuesSummary};

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
    let pr = parse_pr(raw, &RepoSlug::new("mayfieldiv/legit"));
    assert_eq!(
        pr,
        PR {
            number: 42,
            repo_slug: RepoSlug::new("mayfieldiv/legit"),
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
    let pr = parse_pr(raw, &RepoSlug::new("mayfieldiv/legit"));
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
    assert_eq!(
        parse_pr(raw, &RepoSlug::new("mayfieldiv/legit")).state,
        PRState::Closed
    );
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
    assert_eq!(
        parse_pr(raw, &RepoSlug::new("mayfieldiv/legit")).state,
        PRState::Merged
    );
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
    assert_eq!(
        parse_pr(raw, &RepoSlug::new("mayfieldiv/legit")).head_repository_owner,
        ""
    );
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
    let pr = parse_pr(raw, &RepoSlug::new("mayfieldiv/legit"));
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

// ── issue parsing (wayfinder ticket refresh) ─────────────────────────────────

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
fn issue_parse_reads_closed_state() {
    let raw: RawRestIssue =
        serde_json::from_str(r#"{ "number": 6, "title": "done", "state": "closed" }"#)
            .expect("deserialize");

    assert_eq!(parse_issue(raw).state, IssueState::Closed);
}

#[test]
fn issue_listing_drops_pull_requests() {
    // `GET /issues?labels=…` returns PRs too, marked by the `pull_request`
    // key; the listing parse must filter them out.
    let raw: Vec<RawRestIssue> = serde_json::from_str(
        r#"[
            { "number": 1, "title": "a real issue" },
            { "number": 2, "title": "a PR", "pull_request": { "url": "u" } }
        ]"#,
    )
    .expect("deserialize");

    let issues = parse_issues(raw);

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].number, 1);
}
