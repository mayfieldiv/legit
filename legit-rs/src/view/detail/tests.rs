use chrono::{DateTime, TimeZone, Utc};
use ratatui::{Terminal, backend::TestBackend};

use crate::{
    app::model::{DetailState, Model, RepoDetection, ViewMode},
    git_remote::RepoInfo,
    github::rest::{PR, PRState},
    github::types::CheckRun,
    view,
};

fn fixed_now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 20, 12, 0, 0).unwrap()
}

fn render_snapshot(model: &Model, width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| view::view(model, frame, fixed_now()))
        .expect("draw");
    terminal
}

/// Extract the rendered buffer's text as one string per row.
fn buffer_text(terminal: &Terminal<TestBackend>) -> Vec<String> {
    let buf = terminal.backend().buffer();
    let area = *buf.area();
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buf[(x, y)].symbol().to_owned())
                .collect::<String>()
        })
        .collect()
}

fn sample_pr() -> PR {
    PR {
        number: 42,
        repo_slug: "acme/web".to_owned(),
        title: "Add streaming PR list".to_owned(),
        author: "octocat".to_owned(),
        created_at: fixed_now() - chrono::Duration::hours(5),
        updated_at: fixed_now() - chrono::Duration::hours(2),
        additions: 10,
        deletions: 3,
        is_draft: false,
        labels: Vec::new(),
        requested_reviewers: Vec::new(),
        assignees: Vec::new(),
        review_decision: String::new(),
        mergeable: "MERGEABLE".to_owned(),
        last_commit_date: None,
        head_commit_sha: None,
        head_ref: "feat/stream".to_owned(),
        base_ref: "main".to_owned(),
        head_repository_owner: "acme".to_owned(),
        state: PRState::Open,
    }
}

/// Build a model in List mode with the given PR in the list and detected repo.
fn model_with_pr_in_list(pr: PR) -> Model {
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

/// Build a model in Detail mode for `pr`, with the body already arrived.
/// The PR is held in the list (enriched source of truth); the `DetailState`
/// carries the description pre-rendered to lines, matching how
/// `Msg::PRDetailArrived` caches it.
fn model_in_detail(pr: PR, body: &str) -> Model {
    let key = pr.key();
    let mut model = model_with_pr_in_list(pr);
    model.view_mode = ViewMode::Detail(DetailState {
        key,
        body: Some(super::render_description_lines(body)),
        scroll: 0,
    });
    model
}

/// Build a model in Detail mode for `pr`, with checks seeded in enrichment.
/// `head_commit_sha` is set on the **list PR** (the enriched copy) so that
/// `checks_for` can resolve the check runs — this exercises the real data path
/// where `Msg::ReviewStatusArrived` populates the list PR's SHA.
fn model_in_detail_with_checks(pr: PR, body: &str, checks: Vec<CheckRun>) -> Model {
    let sha = "abc123".to_owned();
    let mut pr = pr;
    // Set the SHA on the PR before pushing it into the list so the enriched
    // copy has the SHA, matching how ReviewStatusArrived writes it.
    pr.head_commit_sha = Some(sha.clone());
    let key = pr.key();
    let repo_slug = pr.repo_slug.clone();
    let mut model = model_with_pr_in_list(pr);
    model.view_mode = ViewMode::Detail(DetailState {
        key,
        body: Some(super::render_description_lines(body)),
        scroll: 0,
    });
    model.enrichment.checks.insert((repo_slug, sha), checks);
    model
}

fn check(name: &str, status: &str, conclusion: Option<&str>) -> CheckRun {
    CheckRun {
        name: name.to_owned(),
        status: status.to_owned(),
        conclusion: conclusion.map(str::to_owned),
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[test]
fn detail_loading_state_shows_loading_placeholder() {
    let pr = sample_pr();
    let key = pr.key();
    let mut model = model_with_pr_in_list(pr);
    // Enter detail mode with no body yet (simulates in-flight fetch).
    model.view_mode = ViewMode::Detail(DetailState {
        key,
        body: None,
        scroll: 0,
    });

    let terminal = render_snapshot(&model, 60, 5);
    let rows = buffer_text(&terminal);

    assert!(
        rows.iter().any(|r| r.contains("Loading PR detail")),
        "loading placeholder must appear: {rows:?}"
    );
}

#[test]
fn detail_header_shows_number_title_and_author() {
    let model = model_in_detail(sample_pr(), "");

    let terminal = render_snapshot(&model, 80, 10);
    let rows = buffer_text(&terminal);

    assert!(
        rows[0].contains("#42") && rows[0].contains("Add streaming PR list"),
        "first row must contain PR number and title: {:?}",
        rows[0]
    );
    assert!(
        rows[1].contains("octocat"),
        "second row must contain the author: {:?}",
        rows[1]
    );
}

#[test]
fn detail_header_shows_github_url() {
    let model = model_in_detail(sample_pr(), "");

    let terminal = render_snapshot(&model, 80, 10);
    let rows = buffer_text(&terminal);

    let url = "https://github.com/acme/web/pull/42";
    assert!(
        rows.iter().any(|r| r.contains(url)),
        "URL must appear in the header: {rows:?}"
    );
}

#[test]
fn detail_header_shows_branch_and_mergeable() {
    let model = model_in_detail(sample_pr(), "");

    let terminal = render_snapshot(&model, 80, 10);
    let rows = buffer_text(&terminal);

    // head_ref -> base_ref and mergeable state
    assert!(
        rows.iter()
            .any(|r| r.contains("feat/stream") && r.contains("main")),
        "branch row must show head -> base: {rows:?}"
    );
    assert!(
        rows.iter().any(|r| r.contains("mergeable")),
        "mergeable state must appear: {rows:?}"
    );
}

#[test]
fn detail_no_body_shows_no_description_placeholder() {
    let model = model_in_detail(sample_pr(), "");

    let terminal = render_snapshot(&model, 80, 12);
    let rows = buffer_text(&terminal);

    assert!(
        rows.iter().any(|r| r.contains("No description")),
        "no-body placeholder must appear: {rows:?}"
    );
}

#[test]
fn detail_with_body_renders_markdown() {
    let model = model_in_detail(sample_pr(), "## Summary\n\nFixes a bug.");

    let terminal = render_snapshot(&model, 80, 14);
    let rows = buffer_text(&terminal);

    assert!(
        rows.iter().any(|r| r.contains("Summary")),
        "markdown heading must render: {rows:?}"
    );
    assert!(
        rows.iter().any(|r| r.contains("Fixes a bug")),
        "markdown paragraph must render: {rows:?}"
    );
}

#[test]
fn detail_no_checks_shows_no_checks_section() {
    // No checks in enrichment -> no checks section header.
    let model = model_in_detail(sample_pr(), "");

    let terminal = render_snapshot(&model, 80, 14);
    let rows = buffer_text(&terminal);

    assert!(
        !rows.iter().any(|r| r.contains("CI Checks")),
        "CI Checks section must not appear when no checks: {rows:?}"
    );
}

#[test]
fn detail_with_checks_shows_summary_and_rows() {
    let checks = vec![
        check("build", "completed", Some("success")),
        check("lint", "completed", Some("failure")),
        check("deploy", "in_progress", None),
    ];
    let model = model_in_detail_with_checks(sample_pr(), "", checks);

    let terminal = render_snapshot(&model, 80, 18);
    let rows = buffer_text(&terminal);

    assert!(
        rows.iter().any(|r| r.contains("CI Checks")),
        "CI Checks section header must appear: {rows:?}"
    );
    // Summary counts
    assert!(
        rows.iter().any(|r| r.contains("passed")),
        "summary line must mention passed count: {rows:?}"
    );
    // Individual check names
    assert!(
        rows.iter().any(|r| r.contains("build")),
        "build check must render: {rows:?}"
    );
    assert!(
        rows.iter().any(|r| r.contains("lint")),
        "lint check must render: {rows:?}"
    );
    assert!(
        rows.iter().any(|r| r.contains("deploy")),
        "deploy check must render: {rows:?}"
    );
}

#[test]
fn detail_status_bar_shows_esc_and_r_hints() {
    let model = model_in_detail(sample_pr(), "");

    let terminal = render_snapshot(&model, 80, 10);
    let rows = buffer_text(&terminal);

    let status = rows.last().expect("status row");
    assert!(
        status.contains("esc") || status.contains("Esc") || status.contains("ESC"),
        "status bar must mention Esc: {status:?}"
    );
    assert!(
        status.contains(" r ") || status.contains("r ") || status.contains(" r"),
        "status bar must mention r key: {status:?}"
    );
}

#[test]
fn detail_draft_pr_shows_draft_marker() {
    let mut pr = sample_pr();
    pr.is_draft = true;
    let model = model_in_detail(pr, "");

    let terminal = render_snapshot(&model, 80, 10);
    let rows = buffer_text(&terminal);

    assert!(
        rows.iter().any(|r| r.to_lowercase().contains("draft")),
        "draft marker must appear: {rows:?}"
    );
}

#[test]
fn detail_body_scrolls_when_detail_scroll_is_nonzero() {
    // Build a multi-line body: lines "Line 1" … "Line 10" so the first line
    // is distinct. A scroll offset of 1 must push "Line 1" off the top.
    let body: String = (1..=10).map(|n| format!("Line {n}\n\n")).collect();
    let mut model = model_in_detail(sample_pr(), &body);

    // At scroll 0 the first line of the body must be visible.
    let terminal = render_snapshot(&model, 80, 14);
    let rows = buffer_text(&terminal);
    assert!(
        rows.iter().any(|r| r.contains("Line 1")),
        "Line 1 must be visible at scroll 0: {rows:?}"
    );

    // With scroll offset 2 (skipping the first two rendered lines), "Line 1"
    // should no longer be visible in the body area.
    if let ViewMode::Detail(detail) = &mut model.view_mode {
        detail.scroll = 2;
    }
    let terminal = render_snapshot(&model, 80, 14);
    let rows = buffer_text(&terminal);
    assert!(
        !rows.iter().any(|r| r.contains("Line 1")),
        "Line 1 must be scrolled off at detail_scroll=2: {rows:?}"
    );
    assert!(
        rows.iter().any(|r| r.contains("Line 2")),
        "Line 2 must still be visible at detail_scroll=2: {rows:?}"
    );
}

#[test]
fn detail_status_bar_shows_jk_scroll_hint() {
    let model = model_in_detail(sample_pr(), "");

    let terminal = render_snapshot(&model, 80, 10);
    let rows = buffer_text(&terminal);

    let status = rows.last().expect("status row");
    assert!(
        status.contains("j/k"),
        "status bar must mention j/k scroll hint: {status:?}"
    );
}
