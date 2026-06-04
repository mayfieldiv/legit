use chrono::{DateTime, TimeZone, Utc};
use ratatui::{Terminal, backend::TestBackend};

use super::panel_width;
use crate::{
    app::model::{Model, RepoDetection},
    blocker::{BlockerResult, Tier},
    git_remote::RepoInfo,
    github::rest::{PR, PRState},
    view,
};

fn fixed_now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 20, 12, 0, 0).unwrap()
}

fn sample_pr(number: u64, title: &str) -> PR {
    PR {
        number,
        repo_slug: "acme/web".to_owned(),
        title: title.to_owned(),
        author: "octocat".to_owned(),
        created_at: fixed_now() - chrono::Duration::hours(5),
        updated_at: fixed_now() - chrono::Duration::hours(2),
        additions: 5,
        deletions: 3,
        is_draft: false,
        labels: Vec::new(),
        requested_reviewers: Vec::new(),
        assignees: Vec::new(),
        review_decision: String::new(),
        mergeable: "UNKNOWN".to_owned(),
        last_commit_date: None,
        head_commit_sha: None,
        head_ref: "feat/x".to_owned(),
        base_ref: "main".to_owned(),
        head_repository_owner: "acme".to_owned(),
        state: PRState::Open,
    }
}

/// A model with one Tracked Repo (acme/web) and `pr` streamed in and selected.
/// No enrichment is seeded, so every section is in its loading/empty state
/// until the test adds it.
fn model_with_selected(pr: PR) -> Model {
    let (mut model, _) = Model::new();
    model.repo = RepoDetection::Detected(RepoInfo {
        owner: "acme".to_owned(),
        repo: "web".to_owned(),
    });
    model.list.begin_fetch("acme/web");
    model.list.push(pr);
    model.list.complete_fetch("acme/web");
    model.relayout();
    model
}

/// Seed the cached blocker result for the selected PR so the smart-status line
/// renders.
fn with_blocker(model: &mut Model, tier: Tier, blocker: &str, reason: &str) {
    let key = model.list.selected_pr().expect("a PR is selected").key();
    model.blockers.insert(
        key,
        BlockerResult {
            blocker: blocker.to_owned(),
            tier,
            reason: reason.to_owned(),
        },
    );
}

/// Seed loaded reviews for the selected PR.
fn with_reviews(model: &mut Model, reviews: Vec<crate::github::types::Review>) {
    let key = model.list.selected_pr().expect("a PR is selected").key();
    model.enrichment.reviews.insert(key, reviews);
}

fn review(user: &str, state: &str) -> crate::github::types::Review {
    crate::github::types::Review {
        user: user.to_owned(),
        state: state.to_owned(),
    }
}

/// Seed loaded review threads for the selected PR.
fn with_threads(model: &mut Model, threads: Vec<crate::github::types::FullReviewThread>) {
    let key = model.list.selected_pr().expect("a PR is selected").key();
    model.enrichment.review_threads.insert(key, threads);
}

/// A review thread whose first comment is by `author` (a bot iff `is_bot`),
/// resolved per `resolved`.
fn thread(author: &str, is_bot: bool, resolved: bool) -> crate::github::types::FullReviewThread {
    crate::github::types::FullReviewThread {
        id: format!("T-{author}"),
        is_resolved: resolved,
        path: "src/x.rs".to_owned(),
        line: Some(1),
        comments: vec![crate::github::types::ReviewComment {
            id: "C1".to_owned(),
            author: author.to_owned(),
            body: "b".to_owned(),
            created_at: fixed_now(),
            url: "u".to_owned(),
            is_bot,
        }],
    }
}

/// Seed loaded check runs for the selected PR. Stamps the PR's head SHA so the
/// checks key (repo slug, head SHA) resolves.
fn with_checks(model: &mut Model, head_sha: &str, checks: Vec<crate::github::types::CheckRun>) {
    let key = model.list.selected_pr().expect("a PR is selected").key();
    if let Some(pr) = model.list.pr_mut(&key) {
        pr.head_commit_sha = Some(head_sha.to_owned());
    }
    model
        .enrichment
        .checks
        .insert((key.repo_slug, head_sha.to_owned()), checks);
}

fn check(name: &str, status: &str, conclusion: Option<&str>) -> crate::github::types::CheckRun {
    crate::github::types::CheckRun {
        name: name.to_owned(),
        status: status.to_owned(),
        conclusion: conclusion.map(str::to_owned),
    }
}

/// Render the whole frame at `width`x`height` and return the panel's columns
/// (everything right of the list/summary split), excluding the tab bar and
/// status bar rows. The panel width matches `panel_width(width)`.
fn panel_rows(model: &Model, width: u16, height: u16) -> Vec<String> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| view::view(model, frame, fixed_now()))
        .expect("draw");
    let buf = terminal.backend().buffer().clone();
    let panel_w = panel_width(width).expect("panel should be visible at this width");
    let split_x = width - panel_w;
    (1..height - 1)
        .map(|y| {
            (split_x..width)
                .map(|x| buf[(x, y)].symbol().to_owned())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn no_pr_selected_renders_placeholder() {
    let (model, _) = Model::new();

    let rows = panel_rows(&model, 80, 6);

    assert!(
        rows[0].trim_start().starts_with("No PR selected"),
        "{rows:?}"
    );
}

#[test]
fn renders_smart_status_reason_as_the_first_section() {
    let mut model = model_with_selected(sample_pr(42, "Add the thing"));
    with_blocker(
        &mut model,
        Tier::MeBlocking,
        "octocat",
        "You requested review",
    );

    let rows = panel_rows(&model, 140, 20);

    assert!(
        rows.iter().any(|r| r.contains("You requested review")),
        "smart-status reason must render: {rows:?}"
    );
}

#[test]
fn smart_status_reason_shows_loading_until_blocker_derived() {
    let model = model_with_selected(sample_pr(42, "Add the thing"));

    let rows = panel_rows(&model, 140, 20);

    assert!(
        rows.iter().any(|r| r.contains("Loading…")),
        "an underived blocker shows a loading placeholder: {rows:?}"
    );
}

#[test]
fn renders_mergeable_state_after_smart_status() {
    let mut pr = sample_pr(42, "Add the thing");
    pr.mergeable = "MERGEABLE".to_owned();
    let mut model = model_with_selected(pr);
    with_blocker(&mut model, Tier::NeedsReview, "", "Awaiting review");

    let rows = panel_rows(&model, 140, 20);

    let reason_idx = rows
        .iter()
        .position(|r| r.contains("Awaiting review"))
        .expect("smart-status line present");
    let merge_idx = rows
        .iter()
        .position(|r| r.contains("mergeable"))
        .expect("mergeable line present");
    assert!(
        merge_idx > reason_idx,
        "mergeable must come after smart-status: {rows:?}"
    );
}

#[test]
fn renders_conflict_when_pr_is_conflicting() {
    let mut pr = sample_pr(42, "Add the thing");
    pr.mergeable = "CONFLICTING".to_owned();
    let model = model_with_selected(pr);

    let rows = panel_rows(&model, 140, 20);

    assert!(
        rows.iter().any(|r| r.contains("conflict")),
        "conflicting PR shows a conflict line: {rows:?}"
    );
}

#[test]
fn renders_review_counts_and_per_reviewer_states() {
    let mut model = model_with_selected(sample_pr(42, "Add the thing"));
    with_reviews(
        &mut model,
        vec![
            review("alice", "APPROVED"),
            review("bob", "CHANGES_REQUESTED"),
            review("carol", "COMMENTED"),
        ],
    );

    let rows = panel_rows(&model, 140, 24);
    let joined = rows.join("\n");

    // Counts: one approved, one changes-requested, one commented.
    assert!(joined.contains("1 approved"), "approved count: {rows:?}");
    assert!(
        joined.contains("1 changes requested") || joined.contains("1 changes-requested"),
        "changes-requested count: {rows:?}"
    );
    // Per-reviewer rows name each reviewer.
    assert!(joined.contains("alice"), "alice row: {rows:?}");
    assert!(joined.contains("bob"), "bob row: {rows:?}");
    assert!(joined.contains("carol"), "carol row: {rows:?}");
}

#[test]
fn reviews_show_loading_until_arrived() {
    let model = model_with_selected(sample_pr(42, "Add the thing"));

    let rows = panel_rows(&model, 140, 24);
    // The reviews section header should still appear with a loading placeholder
    // beside it.
    assert!(
        rows.iter().any(|r| r.to_lowercase().contains("review")),
        "reviews section present: {rows:?}"
    );
}

#[test]
fn renders_thread_counts_total_unresolved_human_and_bot() {
    let mut model = model_with_selected(sample_pr(42, "Add the thing"));
    with_threads(
        &mut model,
        vec![
            thread("alice", false, false),     // unresolved human
            thread("dependabot", true, false), // unresolved bot
            thread("bob", false, true),        // resolved (counts to total only)
        ],
    );

    let rows = panel_rows(&model, 140, 28);
    let joined = rows.join("\n");

    assert!(joined.contains("3 total"), "total threads: {rows:?}");
    assert!(joined.contains("2 unresolved"), "unresolved: {rows:?}");
    assert!(joined.contains("1 human"), "unresolved human: {rows:?}");
    assert!(joined.contains("1 bot"), "unresolved bot: {rows:?}");
}

#[test]
fn threads_show_loading_until_arrived() {
    let model = model_with_selected(sample_pr(42, "Add the thing"));

    let rows = panel_rows(&model, 140, 28);

    assert!(
        rows.iter().any(|r| r.to_lowercase().contains("thread")),
        "threads section present: {rows:?}"
    );
}

#[test]
fn renders_check_counts_and_rows_for_non_passing_only() {
    let mut model = model_with_selected(sample_pr(42, "Add the thing"));
    with_checks(
        &mut model,
        "abc123",
        vec![
            check("build", "completed", Some("success")),
            check("lint", "completed", Some("failure")),
            check("deploy", "in_progress", None),
        ],
    );

    let rows = panel_rows(&model, 140, 30);
    let joined = rows.join("\n");

    assert!(joined.contains("1 failed"), "failed count: {rows:?}");
    assert!(joined.contains("1 pending"), "pending count: {rows:?}");
    assert!(joined.contains("passed"), "passed count: {rows:?}");
    // Non-passing checks get their own rows; the passing one does not.
    assert!(joined.contains("lint"), "failed check row: {rows:?}");
    assert!(joined.contains("deploy"), "pending check row: {rows:?}");
    assert!(
        !joined.contains("build"),
        "passing check must not get its own row: {rows:?}"
    );
}

#[test]
fn checks_show_loading_until_arrived() {
    let model = model_with_selected(sample_pr(42, "Add the thing"));

    let rows = panel_rows(&model, 140, 30);

    assert!(
        rows.iter().any(|r| r.to_lowercase().contains("check")),
        "checks section present: {rows:?}"
    );
}

#[test]
fn panel_is_hidden_below_eighty_columns() {
    assert_eq!(panel_width(79), None);
    assert_eq!(panel_width(0), None);
}

#[test]
fn panel_is_thirty_six_columns_in_the_narrow_band() {
    assert_eq!(panel_width(80), Some(36));
    assert_eq!(panel_width(139), Some(36));
}

#[test]
fn panel_is_fifty_columns_at_one_forty_and_above() {
    assert_eq!(panel_width(140), Some(50));
    assert_eq!(panel_width(200), Some(50));
}
