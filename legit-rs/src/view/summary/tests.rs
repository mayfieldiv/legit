use chrono::{DateTime, TimeZone, Utc};
use ratatui::{Terminal, backend::TestBackend};

use crate::{
    app::list_layout::panel_width,
    app::model::{Model, RepoDetection},
    blocker::{BlockerResult, Tier},
    git_remote::RepoInfo,
    github::rest::{PR, PRState},
    view,
    worktree::WorktreeEntry,
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
        review_status_loaded: true,
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

/// Seed categorised files for the selected PR, running `categorize` with no
/// user rules (the built-in heuristics decide categories).
fn with_files(model: &mut Model, paths: &[(&str, u64, u64)]) {
    let key = model.list.selected_pr().expect("a PR is selected").key();
    let changes: Vec<crate::file_category::FileChange> = paths
        .iter()
        .map(|(path, add, del)| crate::file_category::FileChange {
            path: (*path).to_owned(),
            additions: *add,
            deletions: *del,
        })
        .collect();
    let categorization = crate::file_category::categorize(&changes, &[]);
    model
        .enrichment
        .files
        .insert(key, crate::app::model::FilesState::Loaded(categorization));
}

fn with_worktree(model: &mut Model, path: &str, branch: &str) {
    model.worktrees_by_repo.insert(
        "acme/web".to_owned(),
        vec![WorktreeEntry {
            path: path.to_owned(),
            head: "a".repeat(40),
            branch_ref: Some(format!("refs/heads/{branch}")),
            branch_name: Some(branch.to_owned()),
            detached: false,
            bare: false,
            locked: None,
            prunable: None,
        }],
    );
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
    (2..height - 1)
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
fn renders_identity_metadata_labels_assignees_and_requested_reviewers() {
    let mut pr = sample_pr(42, "Add the thing");
    pr.is_draft = true;
    pr.labels = vec!["enhancement".to_owned(), "ready-for-agent".to_owned()];
    pr.assignees = vec!["octocat".to_owned()];
    pr.requested_reviewers = vec!["alice".to_owned(), "bob".to_owned()];
    let model = model_with_selected(pr);

    let rows = panel_rows(&model, 140, 24);
    let joined = rows.join("\n");

    assert!(joined.contains("Add the thing"), "title: {rows:?}");
    assert!(joined.contains("octocat #42 draft"), "meta: {rows:?}");
    assert!(
        joined.contains("feat/x") && joined.contains("main"),
        "branches: {rows:?}"
    );
    assert!(joined.contains("created 5h updated 2h"), "dates: {rows:?}");
    assert!(
        joined.contains("labels: enhancement, ready-for-agent"),
        "labels: {rows:?}"
    );
    assert!(joined.contains("assignees: octocat"), "assignees: {rows:?}");
    assert!(joined.contains("requested"), "requested header: {rows:?}");
    assert!(joined.contains("alice pending"), "alice pending: {rows:?}");
    assert!(joined.contains("bob pending"), "bob pending: {rows:?}");
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
fn renders_mergeable_state_as_visible_checkout_line() {
    let mut pr = sample_pr(42, "Add the thing");
    pr.mergeable = "MERGEABLE".to_owned();
    let mut model = model_with_selected(pr);
    with_blocker(&mut model, Tier::NeedsReview, "", "Awaiting review");

    let rows = panel_rows(&model, 140, 20);

    let checkout_idx = rows
        .iter()
        .position(|r| r.contains("feat/x") && r.contains("main"))
        .expect("checkout status line present");
    let merge_idx = rows
        .iter()
        .position(|r| r.contains("mergeable"))
        .expect("mergeable line present");
    let reason_idx = rows
        .iter()
        .position(|r| r.contains("Awaiting review"))
        .expect("smart-status line present");
    assert!(
        !rows[checkout_idx].contains("mergeable"),
        "branch row must not hide mergeability offscreen: {rows:?}"
    );
    assert_eq!(
        merge_idx,
        checkout_idx + 1,
        "mergeability must stay adjacent to checkout status: {rows:?}"
    );
    assert!(
        merge_idx < reason_idx,
        "checkout status metadata must come before smart-status: {rows:?}"
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
fn renders_file_category_breakdown_per_category() {
    let mut model = model_with_selected(sample_pr(42, "Add the thing"));
    with_files(
        &mut model,
        &[
            ("src/app.rs", 10, 2),     // code
            ("README.md", 3, 0),       // docs
            ("src/app_test.rs", 4, 1), // still code (no test heuristic match)
        ],
    );

    let rows = panel_rows(&model, 140, 34);
    let joined = rows.join("\n");

    // A code row with its combined size and a docs row.
    assert!(joined.contains("code"), "code category row: {rows:?}");
    assert!(joined.contains("docs"), "docs category row: {rows:?}");
    // Code adds 10+4 = 14, deletes 2+1 = 3.
    assert!(joined.contains("+14/-3"), "code size: {rows:?}");
    assert!(joined.contains("+3/-0"), "docs size: {rows:?}");
}

#[test]
fn files_show_loading_until_arrived() {
    let model = model_with_selected(sample_pr(42, "Add the thing"));

    let rows = panel_rows(&model, 140, 34);

    assert!(
        rows.iter().any(|r| r.to_lowercase().contains("files")),
        "files section present: {rows:?}"
    );
}

#[test]
fn renders_github_url_near_top() {
    let model = model_with_selected(sample_pr(42, "Add the thing"));

    let rows = panel_rows(&model, 140, 34);

    let url_idx = rows
        .iter()
        .position(|r| r.contains("https://github.com/acme/web/pull/42"))
        .unwrap_or_else(|| panic!("GitHub URL must render near top: {rows:?}"));
    let branch_idx = rows
        .iter()
        .position(|r| r.contains("feat/x") && r.contains("main"))
        .unwrap_or_else(|| panic!("branch row: {rows:?}"));

    assert!(
        url_idx <= 2,
        "URL should be part of identity block: {rows:?}"
    );
    assert!(
        url_idx < branch_idx,
        "URL should render before checkout metadata: {rows:?}"
    );
}

#[test]
fn renders_worktree_path_when_present() {
    let mut pr = sample_pr(42, "Add the thing");
    pr.mergeable = "MERGEABLE".to_owned();
    let mut model = model_with_selected(pr);
    with_worktree(&mut model, "/w/42", "feat/x");

    let rows = panel_rows(&model, 140, 34);
    let branch_idx = rows
        .iter()
        .position(|r| r.contains("feat/x") && r.contains("main"))
        .unwrap_or_else(|| panic!("checkout row: {rows:?}"));
    let worktree_idx = rows
        .iter()
        .position(|r| r.contains("worktree:"))
        .unwrap_or_else(|| panic!("worktree row: {rows:?}"));
    let merge_idx = rows
        .iter()
        .position(|r| r.contains("mergeable"))
        .unwrap_or_else(|| panic!("mergeability row: {rows:?}"));

    assert_eq!(
        worktree_idx,
        branch_idx + 1,
        "worktree path stays adjacent to branch: {rows:?}"
    );
    assert!(
        rows[worktree_idx].contains("/w/42"),
        "worktree path: {rows:?}"
    );
    assert_eq!(
        merge_idx,
        worktree_idx + 1,
        "mergeability stays grouped below worktree status: {rows:?}"
    );
}

#[test]
fn long_checkout_status_splits_worktree_next_to_branch() {
    let mut pr = sample_pr(42, "Add the thing");
    pr.head_ref = "feature/very-long-branch-name-that-will-not-fit".to_owned();
    pr.base_ref = "master".to_owned();
    pr.mergeable = "MERGEABLE".to_owned();
    let mut model = model_with_selected(pr);
    with_worktree(
        &mut model,
        "~/dev/immytrees/8887",
        "feature/very-long-branch-name-that-will-not-fit",
    );

    let rows = panel_rows(&model, 140, 34);
    let branch_idx = rows
        .iter()
        .position(|r| r.contains("feature/very-long"))
        .unwrap_or_else(|| panic!("branch row: {rows:?}"));
    let worktree_idx = rows
        .iter()
        .position(|r| r.contains("worktree:"))
        .unwrap_or_else(|| panic!("worktree row: {rows:?}"));

    assert_eq!(
        worktree_idx,
        branch_idx + 1,
        "worktree must stay adjacent to a long branch row: {rows:?}"
    );
    assert!(
        rows[worktree_idx].contains("~/dev/immytrees/8887"),
        "worktree path stays visible: {rows:?}"
    );
    let merge_idx = rows
        .iter()
        .position(|r| r.contains("mergeable"))
        .unwrap_or_else(|| panic!("mergeability row: {rows:?}"));
    assert_eq!(
        merge_idx,
        worktree_idx + 1,
        "mergeability must stay visible below worktree status: {rows:?}"
    );
    assert!(
        !rows[branch_idx].contains("mergeable"),
        "long branch row must not carry mergeability offscreen: {rows:?}"
    );
}

#[test]
fn omits_worktree_line_when_absent() {
    let model = model_with_selected(sample_pr(42, "Add the thing"));

    let rows = panel_rows(&model, 140, 34);

    assert!(
        !rows.join("\n").contains("worktree:"),
        "absent worktree should not render a placeholder: {rows:?}"
    );
}

// ── full-panel snapshot scenarios across the supported widths ────────────────

/// A model with every section's enrichment present: a derived blocker, reviews,
/// threads, checks, and categorised files. Drives the "fully loaded" snapshots.
fn fully_loaded_model() -> Model {
    let mut pr = sample_pr(42, "Add the thing");
    pr.mergeable = "MERGEABLE".to_owned();
    let mut model = model_with_selected(pr);
    with_blocker(&mut model, Tier::NeedsReview, "alice", "Awaiting reviewer");
    with_reviews(
        &mut model,
        vec![review("alice", "APPROVED"), review("bob", "COMMENTED")],
    );
    with_threads(
        &mut model,
        vec![thread("carol", false, false), thread("bot", true, false)],
    );
    with_checks(
        &mut model,
        "abc123",
        vec![
            check("build", "completed", Some("success")),
            check("lint", "completed", Some("failure")),
        ],
    );
    with_files(&mut model, &[("src/app.rs", 10, 2), ("README.md", 3, 0)]);
    model
}

/// Every section of a fully-loaded panel renders, with no `Loading…`
/// placeholders, at all three supported widths.
#[test]
fn fully_loaded_panel_renders_every_section_at_each_width() {
    for width in [80, 140, 200] {
        let model = fully_loaded_model();
        let rows = panel_rows(&model, width, 40);
        let joined = rows.join("\n");

        assert!(
            joined.contains("Awaiting reviewer"),
            "smart-status @ {width}: {rows:?}"
        );
        assert!(
            joined.contains("mergeable"),
            "mergeable @ {width}: {rows:?}"
        );
        assert!(joined.contains("reviews"), "reviews @ {width}: {rows:?}");
        assert!(joined.contains("threads"), "threads @ {width}: {rows:?}");
        assert!(joined.contains("checks"), "checks @ {width}: {rows:?}");
        assert!(joined.contains("files"), "files @ {width}: {rows:?}");
        assert!(
            joined.contains("https://github.com/acme/web/pull/42"),
            "footer URL @ {width}: {rows:?}"
        );
        assert!(
            !joined.contains("Loading…"),
            "fully loaded panel has no loading placeholders @ {width}: {rows:?}"
        );
    }
}

/// With only some enrichment in, the arrived sections render their data while
/// the missing ones show `Loading…`, at all three widths.
#[test]
fn partial_enrichment_mixes_data_and_loading_placeholders() {
    for width in [80, 140, 200] {
        // Reviews arrived; threads, checks, files have not.
        let mut model = model_with_selected(sample_pr(42, "Add the thing"));
        with_blocker(&mut model, Tier::NeedsReview, "", "Awaiting review");
        with_reviews(&mut model, vec![review("alice", "APPROVED")]);

        let rows = panel_rows(&model, width, 40);
        let joined = rows.join("\n");

        assert!(
            joined.contains("1 approved"),
            "reviews data present @ {width}: {rows:?}"
        );
        assert!(
            joined.contains("Loading…"),
            "missing sections show loading @ {width}: {rows:?}"
        );
    }
}

/// Everything else loaded but the files fetch still in flight: the files
/// section shows `Loading…` while the rest render, at all three widths.
#[test]
fn missing_files_shows_loading_for_the_breakdown_only() {
    for width in [80, 140, 200] {
        let mut model = fully_loaded_model();
        // Drop just the files enrichment.
        let key = model.list.selected_pr().unwrap().key();
        model.enrichment.files.remove(&key);

        let rows = panel_rows(&model, width, 40);
        let joined = rows.join("\n");

        // The files header is still there, now with a loading placeholder.
        let files_line = rows
            .iter()
            .find(|r| r.to_lowercase().contains("files"))
            .unwrap_or_else(|| panic!("files section present @ {width}: {rows:?}"));
        assert!(
            files_line.contains("Loading…"),
            "files breakdown loading @ {width}: {files_line:?}"
        );
        // Other sections still rendered their data.
        assert!(joined.contains("checks"), "checks still present @ {width}");
    }
}

/// No PR selected: the panel shows only the placeholder, at all three widths.
#[test]
fn no_pr_selected_at_each_width() {
    for width in [80, 140, 200] {
        let (model, _) = Model::new();
        let rows = panel_rows(&model, width, 40);
        assert!(
            rows[0].trim_start().starts_with("No PR selected"),
            "no-PR placeholder @ {width}: {rows:?}"
        );
    }
}
