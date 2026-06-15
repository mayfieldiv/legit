// ── enrichment ──────────────────────────────────────────────────────────

use super::*;
use crate::github::types::ReviewStatus;

fn review_status(head_sha: Option<&str>) -> ReviewStatus {
    ReviewStatus {
        additions: 12,
        deletions: 4,
        review_decision: "APPROVED".to_owned(),
        mergeable: "MERGEABLE".to_owned(),
        last_commit_date: None,
        head_commit_sha: head_sha.map(str::to_owned),
    }
}

#[test]
fn pr_list_loaded_fans_out_enrichment_per_pr() {
    let mut model = enriched_model(&[1, 2]);

    let cmds = update(
        &mut model,
        Msg::PrListLoaded {
            repo_slug: "mayfieldiv/legit".to_owned(),
        },
    );

    // 1 batched review-status + (threads + reviews + issue-comments) per PR.
    assert_eq!(cmds.len(), 1 + 2 * 3);
    match &cmds[0] {
        Cmd::FetchReviewStatus { pr_numbers, .. } => assert_eq!(pr_numbers, &[1, 2]),
        other => panic!("first cmd should batch review status, got {other:?}"),
    }
    let threads = cmds
        .iter()
        .filter(|c| matches!(c, Cmd::FetchThreads { .. }))
        .count();
    let reviews = cmds
        .iter()
        .filter(|c| matches!(c, Cmd::FetchReviews { .. }))
        .count();
    let comments = cmds
        .iter()
        .filter(|c| matches!(c, Cmd::FetchIssueComments { .. }))
        .count();
    assert_eq!((threads, reviews, comments), (2, 2, 2));
    // Checks are NOT fanned out yet — they wait on review-status SHAs.
    assert!(!cmds.iter().any(|c| matches!(c, Cmd::FetchChecks { .. })));
}

#[test]
fn pr_list_loaded_with_empty_list_fans_out_nothing() {
    let mut model = enriched_model(&[]);

    let cmds = update(
        &mut model,
        Msg::PrListLoaded {
            repo_slug: "mayfieldiv/legit".to_owned(),
        },
    );

    assert!(cmds.is_empty());
}

#[test]
fn pr_list_loaded_without_auth_fans_out_nothing() {
    let (mut model, _) = Model::new();
    model.repo = RepoDetection::Detected(RepoInfo {
        owner: "mayfieldiv".to_owned(),
        repo: "legit".to_owned(),
    });
    model.list.begin_fetch("mayfieldiv/legit");
    model.list.push(sample_pr(1, "p"));

    let cmds = update(
        &mut model,
        Msg::PrListLoaded {
            repo_slug: "mayfieldiv/legit".to_owned(),
        },
    );

    assert!(
        cmds.is_empty(),
        "no enrichment until the auth token resolves"
    );
}

#[test]
fn review_status_arrived_overwrites_pr_fields_and_fetches_checks() {
    let mut model = enriched_model(&[1]);

    let cmds = update(
        &mut model,
        Msg::ReviewStatusArrived {
            pr: key(1),
            status: review_status(Some("abc123")),
        },
    );

    let pr = &model.list.prs()[0];
    assert_eq!(pr.additions, 12);
    assert_eq!(pr.deletions, 4);
    assert_eq!(pr.review_decision, "APPROVED");
    assert_eq!(pr.mergeable, "MERGEABLE");
    assert_eq!(pr.head_commit_sha.as_deref(), Some("abc123"));
    assert!(pr.review_status_loaded);

    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        Cmd::FetchChecks { head_sha, .. } => assert_eq!(head_sha, "abc123"),
        other => panic!("expected FetchChecks for the new head SHA, got {other:?}"),
    }
}

#[test]
fn review_status_arrived_without_sha_skips_checks() {
    let mut model = enriched_model(&[1]);

    let cmds = update(
        &mut model,
        Msg::ReviewStatusArrived {
            pr: key(1),
            status: review_status(None),
        },
    );

    assert_eq!(model.list.prs()[0].mergeable, "MERGEABLE");
    assert!(cmds.is_empty(), "no SHA means no checks fetch");
}

#[test]
fn review_status_arrived_for_unknown_pr_is_a_noop() {
    let mut model = enriched_model(&[1]);

    let cmds = update(
        &mut model,
        Msg::ReviewStatusArrived {
            pr: key(999),
            status: review_status(Some("abc123")),
        },
    );

    // The known PR is untouched and no checks are fetched for the stray SHA.
    assert_eq!(model.list.prs()[0].mergeable, "UNKNOWN");
    assert!(cmds.is_empty());
}

#[test]
fn review_status_arrived_skips_checks_already_fetched_for_sha() {
    let mut model = enriched_model(&[1]);
    model.enrichment.checks.insert(
        ("mayfieldiv/legit".to_owned(), "abc123".to_owned()),
        Vec::new(),
    );

    let cmds = update(
        &mut model,
        Msg::ReviewStatusArrived {
            pr: key(1),
            status: review_status(Some("abc123")),
        },
    );

    assert!(
        cmds.is_empty(),
        "checks already present for this repo's SHA; don't refetch"
    );
}

#[test]
fn same_sha_in_another_repo_still_fetches_checks() {
    // A fork shares its head SHA with upstream but has its own check runs:
    // upstream's cached checks must not suppress the fork repo's fetch.
    let mut model = enriched_model(&[1]);
    model.config = config_with_repos(&["acme/web"]);
    model.list.push(sample_pr_in("acme/web", 7, "fork pr"));
    model.enrichment.checks.insert(
        ("mayfieldiv/legit".to_owned(), "abc123".to_owned()),
        Vec::new(),
    );

    let cmds = update(
        &mut model,
        Msg::ReviewStatusArrived {
            pr: PrKey {
                repo_slug: "acme/web".to_owned(),
                number: 7,
            },
            status: review_status(Some("abc123")),
        },
    );

    match cmds.as_slice() {
        [Cmd::FetchChecks { ctx, pr, head_sha }] => {
            assert_eq!(ctx.repo.slug(), "acme/web");
            assert_eq!(head_sha, "abc123");
            // Carries the PR the SHA came from, so the limiter can focus-promote it.
            assert_eq!(pr.repo_slug, "acme/web");
            assert_eq!(pr.number, 7);
        }
        other => panic!("expected a FetchChecks for the other repo, got {other:?}"),
    }
}

#[test]
fn threads_arrived_stores_threads_by_pr_number() {
    let mut model = enriched_model(&[1]);
    let thread = crate::github::types::FullReviewThread {
        id: "T1".to_owned(),
        is_resolved: false,
        path: "src/x".to_owned(),
        line: Some(3),
        comments: Vec::new(),
    };

    let cmds = update(
        &mut model,
        Msg::ThreadsArrived {
            pr: key(1),
            threads: vec![thread.clone()],
        },
    );

    assert_eq!(
        model.enrichment.review_threads.get(&key(1)),
        Some(&vec![thread])
    );
    assert!(cmds.is_empty());
}

#[test]
fn reviews_arrived_stores_reviews_by_pr_number() {
    let mut model = enriched_model(&[1]);
    let review = crate::github::types::Review {
        user: "alice".to_owned(),
        state: "APPROVED".to_owned(),
    };

    update(
        &mut model,
        Msg::ReviewsArrived {
            pr: key(1),
            reviews: vec![review.clone()],
        },
    );

    assert_eq!(model.enrichment.reviews.get(&key(1)), Some(&vec![review]));
}

#[test]
fn checks_arrived_stores_checks_by_repo_and_head_sha() {
    let mut model = enriched_model(&[1]);
    let check = crate::github::types::CheckRun {
        name: "build".to_owned(),
        status: "completed".to_owned(),
        conclusion: Some("success".to_owned()),
    };

    update(
        &mut model,
        Msg::ChecksArrived {
            repo_slug: "mayfieldiv/legit".to_owned(),
            head_sha: "abc123".to_owned(),
            checks: vec![check.clone()],
        },
    );

    assert_eq!(
        model
            .enrichment
            .checks
            .get(&("mayfieldiv/legit".to_owned(), "abc123".to_owned())),
        Some(&vec![check])
    );
}

#[test]
fn issue_comments_arrived_stores_comments_by_pr_number() {
    let mut model = enriched_model(&[1]);
    let comment = crate::github::types::IssueComment {
        id: 7,
        author: "bob".to_owned(),
        body: "lgtm".to_owned(),
        created_at: chrono::Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap(),
        url: "u".to_owned(),
        is_bot: false,
    };

    update(
        &mut model,
        Msg::IssueCommentsArrived {
            pr: key(1),
            comments: vec![comment.clone()],
        },
    );

    assert_eq!(
        model.enrichment.issue_comments.get(&key(1)),
        Some(&vec![comment])
    );
}

#[test]
fn enrichment_failure_records_error_and_keeps_data() {
    let mut model = enriched_model(&[1]);
    model.enrichment.reviews.insert(
        key(1),
        vec![crate::github::types::Review {
            user: "alice".to_owned(),
            state: "APPROVED".to_owned(),
        }],
    );

    let cmds = update(
        &mut model,
        Msg::CommandFailed {
            context: "fetch check runs",
            error: "500 Server Error".to_owned(),
        },
    );

    let status = model.status.as_ref().expect("error status recorded");
    assert_eq!(status.kind, StatusKind::Error);
    assert!(status.text.contains("fetch check runs"));
    assert!(status.text.contains("500 Server Error"));
    // Best-effort: nothing already loaded is dropped on a failure.
    assert_eq!(model.list.prs().len(), 1);
    assert!(model.enrichment.reviews.contains_key(&key(1)));
    // The error message is scheduled to auto-clear.
    assert!(matches!(cmds.as_slice(), [Cmd::ScheduleStatusClear { .. }]));
}
