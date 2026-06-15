use chrono::{DateTime, TimeZone, Utc};
use ratatui::{Terminal, backend::TestBackend};

use crate::{
    app::detail_items::{DetailFocus, DetailItems},
    app::model::{DetailState, Model, RepoDetection, ViewMode},
    git_remote::RepoInfo,
    github::rest::{PR, PRState},
    github::types::{CheckRun, FullReviewThread, IssueComment, ReviewComment},
    test_fixtures::{self, review_comment},
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
        review_status_loaded: true,
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
        body: Some(crate::app::detail_layout::render_description_lines(body)),
        scroll: 0,
        focus: DetailFocus::Body,
        followed: None,
        expanded: std::collections::HashSet::new(),
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
        body: Some(crate::app::detail_layout::render_description_lines(body)),
        scroll: 0,
        focus: DetailFocus::Body,
        followed: None,
        expanded: std::collections::HashSet::new(),
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

/// The shared fixture thread with this module's explicit location knobs (the
/// snapshots assert the rendered `path:line`).
fn thread(
    id: &str,
    path: &str,
    line: Option<u64>,
    is_resolved: bool,
    comments: Vec<ReviewComment>,
) -> FullReviewThread {
    FullReviewThread {
        path: path.to_owned(),
        line,
        ..test_fixtures::thread(id, is_resolved, comments)
    }
}

/// Seed the enrichment maps for the detail PR: threads and issue comments
/// arrived, through the same `store_*` writers `Msg::ThreadsArrived` /
/// `Msg::IssueCommentsArrived` use (so the render-once markdown cache is
/// populated exactly like the real data path).
fn seed_threads(model: &mut Model, threads: Vec<FullReviewThread>) {
    let ViewMode::Detail(detail) = &model.view_mode else {
        panic!("expected Detail mode");
    };
    let key = detail.key.clone();
    model.enrichment.store_threads(key, threads);
}

fn issue_comment(id: u64, author: &str, body: &str, is_bot: bool) -> IssueComment {
    IssueComment {
        is_bot,
        ..test_fixtures::issue_comment(id, author, body)
    }
}

fn seed_comments(model: &mut Model, comments: Vec<IssueComment>) {
    let ViewMode::Detail(detail) = &model.view_mode else {
        panic!("expected Detail mode");
    };
    let key = detail.key.clone();
    model.enrichment.store_issue_comments(key, comments);
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[test]
fn detail_loading_state_shows_header_and_loading_placeholder() {
    let pr = sample_pr();
    let key = pr.key();
    let mut model = model_with_pr_in_list(pr);
    // Enter detail mode with no body yet (simulates in-flight fetch).
    model.view_mode = ViewMode::Detail(DetailState {
        key,
        body: None,
        scroll: 0,
        focus: DetailFocus::Body,
        followed: None,
        expanded: std::collections::HashSet::new(),
    });

    // Tall enough for the 5-row header plus a body row for the placeholder.
    let terminal = render_snapshot(&model, 60, 10);
    let rows = buffer_text(&terminal);

    // The header is built from the list PR alone, so it shows immediately —
    // before the body fetch returns — alongside the body-area placeholder.
    assert!(
        rows[0].contains("#42") && rows[0].contains("Add streaming PR list"),
        "header must show while the body is still loading: {:?}",
        rows[0]
    );
    assert!(
        rows.iter().any(|r| r.contains("Loading PR detail")),
        "loading placeholder must appear in the body area: {rows:?}"
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

// ── Review threads section ─────────────────────────────────────────────────

#[test]
fn detail_threads_section_shows_thread_card() {
    let mut model = model_in_detail(sample_pr(), "The description.");
    seed_threads(
        &mut model,
        vec![thread(
            "t1",
            "src/lib.rs",
            Some(12),
            false,
            vec![review_comment("c1", "alice", "Please rename this.")],
        )],
    );

    let terminal = render_snapshot(&model, 80, 24);
    let rows = buffer_text(&terminal);

    assert!(
        rows.iter().any(|r| r.contains("Review Threads")),
        "threads section header must appear: {rows:?}"
    );
    assert!(
        rows.iter().any(|r| r.contains("src/lib.rs:12")),
        "thread card must show file:line: {rows:?}"
    );
    assert!(
        rows.iter().any(|r| r.contains("● unreplied")),
        "unreplied status badge must appear: {rows:?}"
    );
    assert!(
        rows.iter().any(|r| r.contains("alice")),
        "root comment author must appear: {rows:?}"
    );
    assert!(
        rows.iter().any(|r| r.contains("Please rename this.")),
        "root comment body must render: {rows:?}"
    );
}

#[test]
fn detail_thread_replies_render_indented_with_arrow_prefix() {
    let mut model = model_in_detail(sample_pr(), "The description.");
    seed_threads(
        &mut model,
        vec![thread(
            "t1",
            "src/lib.rs",
            Some(12),
            false,
            vec![
                review_comment("c1", "alice", "Please rename this."),
                review_comment("c2", "octocat", "Done, renamed."),
            ],
        )],
    );

    let terminal = render_snapshot(&model, 80, 24);
    let rows = buffer_text(&terminal);

    let reply_row = rows
        .iter()
        .find(|r| r.contains("↳"))
        .unwrap_or_else(|| panic!("a reply row with ↳ prefix must render: {rows:?}"));
    assert!(
        reply_row.contains("octocat"),
        "reply row must name the reply author: {reply_row:?}"
    );
    assert!(
        rows.iter().any(|r| r.contains("Done, renamed.")),
        "reply body must render: {rows:?}"
    );
}

// ── Conversation section ───────────────────────────────────────────────────

#[test]
fn detail_conversation_section_shows_comment_cards_with_bot_styling() {
    let mut model = model_in_detail(sample_pr(), "The description.");
    seed_comments(
        &mut model,
        vec![
            issue_comment(1, "carol", "Looks good overall.", false),
            issue_comment(2, "ci-reporter", "Coverage: 98%", true),
        ],
    );

    let terminal = render_snapshot(&model, 80, 24);
    let rows = buffer_text(&terminal);

    assert!(
        rows.iter().any(|r| r.contains("Conversation")),
        "conversation section header must appear: {rows:?}"
    );
    assert!(
        rows.iter().any(|r| r.contains("carol")),
        "human comment author must appear: {rows:?}"
    );
    assert!(
        rows.iter().any(|r| r.contains("Looks good overall.")),
        "comment body must render: {rows:?}"
    );
    assert!(
        rows.iter().any(|r| r.contains("ci-reporter [bot]")),
        "bot comment must carry the [bot] tag: {rows:?}"
    );
}

#[test]
fn detail_shows_loading_placeholders_until_threads_and_comments_arrive() {
    // Body arrived, but neither threads nor issue comments have landed yet.
    let model = model_in_detail(sample_pr(), "The description.");

    let terminal = render_snapshot(&model, 80, 24);
    let rows = buffer_text(&terminal);

    assert!(
        rows.iter().any(|r| r.contains("Loading threads")),
        "threads loading placeholder must appear before ThreadsArrived: {rows:?}"
    );
    assert!(
        rows.iter().any(|r| r.contains("Loading comments")),
        "comments loading placeholder must appear before IssueCommentsArrived: {rows:?}"
    );
}

#[test]
fn detail_arrived_but_empty_threads_and_comments_show_no_sections() {
    let mut model = model_in_detail(sample_pr(), "The description.");
    seed_threads(&mut model, Vec::new());
    seed_comments(&mut model, Vec::new());

    let terminal = render_snapshot(&model, 80, 24);
    let rows = buffer_text(&terminal);

    assert!(
        !rows.iter().any(|r| r.contains("Loading threads")),
        "no threads placeholder once an empty thread list arrived: {rows:?}"
    );
    assert!(
        !rows.iter().any(|r| r.contains("Review Threads")),
        "no threads section for an empty thread list: {rows:?}"
    );
    assert!(
        !rows.iter().any(|r| r.contains("Conversation")),
        "no conversation section for an empty comment list: {rows:?}"
    );
}

// ── Focus borders ───────────────────────────────────────────────────────────

/// Focus the Focus Sequence item at `index`, resolved to its identity through
/// the same derivation `update` uses.
fn set_focus(model: &mut Model, index: usize) {
    let ViewMode::Detail(detail) = &model.view_mode else {
        panic!("expected Detail mode");
    };
    let focus = DetailItems::derive(
        model.enrichment.threads_for(&detail.key),
        model.enrichment.comments_for(&detail.key),
        model.detail_filters(),
    )
    .focus_at(index);
    match &mut model.view_mode {
        ViewMode::Detail(detail) => detail.focus = focus,
        ViewMode::List => panic!("expected Detail mode"),
    }
}

/// A detail model with one two-comment thread and one issue comment: the focus
/// sequence is body(0), thread root(1), reply(2), comment(3).
fn focusable_model() -> Model {
    let mut model = model_in_detail(sample_pr(), "The description.");
    seed_threads(
        &mut model,
        vec![thread(
            "t1",
            "src/lib.rs",
            Some(12),
            false,
            vec![
                review_comment("c1", "alice", "Please rename this."),
                review_comment("c2", "octocat", "Done, renamed."),
            ],
        )],
    );
    seed_comments(
        &mut model,
        vec![issue_comment(10, "carol", "Looks good overall.", false)],
    );
    model
}

/// The 0-based row index of the first row containing `needle`.
fn row_of(rows: &[String], needle: &str) -> usize {
    rows.iter()
        .position(|r| r.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} not found in {rows:?}"))
}

#[test]
fn detail_adjacent_cards_share_a_single_separator_row() {
    // Adjacent cards sit one row apart: the row is the previous card's bottom
    // border and the next card's top border at once (whichever is focused
    // draws it; both blank otherwise). Two rows apart would make focus
    // changes read as the cards jumping.
    let model = focusable_model();
    let rows = buffer_text(&render_snapshot(&model, 80, 30));

    let root_body = row_of(&rows, "Please rename this.");
    let reply_byline = row_of(&rows, "↳");
    assert_eq!(
        reply_byline,
        root_body + 2,
        "adjacent cards must be separated by exactly one shared row: {rows:?}"
    );
}

#[test]
fn detail_focus_on_body_renders_no_card_borders() {
    // The body (focus 0) is unstyled — matching the TS DetailView, where only
    // thread/reply/comment cards carry a border.
    let model = focusable_model();

    let terminal = render_snapshot(&model, 80, 30);
    let rows = buffer_text(&terminal);

    assert!(
        !rows.iter().any(|r| r.contains('╭') || r.contains('│')),
        "no visible card border while the body is focused: {rows:?}"
    );
}

#[test]
fn detail_focused_reply_gets_a_border_without_shifting_the_layout() {
    let mut model = focusable_model();
    let unfocused_rows = buffer_text(&render_snapshot(&model, 80, 30));

    set_focus(&mut model, 2); // the reply card
    let focused_rows = buffer_text(&render_snapshot(&model, 80, 30));

    // Same layout footprint: every content row stays on the same line.
    assert_eq!(
        row_of(&unfocused_rows, "Done, renamed."),
        row_of(&focused_rows, "Done, renamed."),
        "focusing a card must not shift the layout"
    );

    // The focused reply row carries the left border; the rows above/below it
    // carry the top/bottom border.
    let reply_row = row_of(&focused_rows, "Done, renamed.");
    assert!(
        focused_rows[reply_row].contains('│'),
        "focused card content rows must show the side border: {:?}",
        focused_rows[reply_row]
    );
    assert!(
        focused_rows[..reply_row].iter().any(|r| r.contains('╭')),
        "focused card must show a top border above it: {focused_rows:?}"
    );
    assert!(
        focused_rows[reply_row..].iter().any(|r| r.contains('╰')),
        "focused card must show a bottom border below it: {focused_rows:?}"
    );

    // The unfocused thread-root card shows no border chars.
    let root_row = row_of(&focused_rows, "Please rename this.");
    assert!(
        !focused_rows[root_row].contains('│'),
        "unfocused cards must not render border chars: {:?}",
        focused_rows[root_row]
    );
}

// ── Section combinations ────────────────────────────────────────────────────

#[test]
fn detail_fully_loaded_shows_checks_threads_and_conversation() {
    let mut model = model_in_detail_with_checks(
        sample_pr(),
        "The description.",
        vec![check("build", "completed", Some("success"))],
    );
    seed_threads(
        &mut model,
        vec![thread(
            "t1",
            "src/lib.rs",
            Some(12),
            false,
            vec![review_comment("c1", "alice", "Please rename this.")],
        )],
    );
    seed_comments(
        &mut model,
        vec![issue_comment(10, "carol", "Looks good overall.", false)],
    );

    let rows = buffer_text(&render_snapshot(&model, 80, 40));

    for section in [
        "The description.",
        "CI Checks",
        "Review Threads",
        "Conversation",
    ] {
        assert!(
            rows.iter().any(|r| r.contains(section)),
            "{section:?} must render in the fully loaded view: {rows:?}"
        );
    }
    assert!(
        !rows.iter().any(|r| r.contains("Loading")),
        "nothing is loading once everything arrived: {rows:?}"
    );
}

#[test]
fn detail_threads_only_shows_no_conversation_section() {
    let mut model = model_in_detail(sample_pr(), "The description.");
    seed_threads(
        &mut model,
        vec![thread(
            "t1",
            "src/lib.rs",
            Some(12),
            false,
            vec![review_comment("c1", "alice", "Please rename this.")],
        )],
    );
    seed_comments(&mut model, Vec::new());

    let rows = buffer_text(&render_snapshot(&model, 80, 30));

    assert!(
        rows.iter().any(|r| r.contains("Review Threads")),
        "threads section must render: {rows:?}"
    );
    assert!(
        !rows.iter().any(|r| r.contains("Conversation")),
        "no conversation section for an arrived-empty comment list: {rows:?}"
    );
    assert!(
        !rows.iter().any(|r| r.contains("Loading comments")),
        "no comments placeholder once the empty list arrived: {rows:?}"
    );
}

#[test]
fn detail_conversation_only_shows_no_threads_section() {
    let mut model = model_in_detail(sample_pr(), "The description.");
    seed_threads(&mut model, Vec::new());
    seed_comments(
        &mut model,
        vec![issue_comment(10, "carol", "Looks good overall.", false)],
    );

    let rows = buffer_text(&render_snapshot(&model, 80, 30));

    assert!(
        rows.iter().any(|r| r.contains("Conversation")),
        "conversation section must render: {rows:?}"
    );
    assert!(
        !rows.iter().any(|r| r.contains("Review Threads")),
        "no threads section for an arrived-empty thread list: {rows:?}"
    );
    assert!(
        !rows.iter().any(|r| r.contains("Loading threads")),
        "no threads placeholder once the empty list arrived: {rows:?}"
    );
}

// ── Long-body collapse (enter expands) ──────────────────────────────────────

#[test]
fn detail_long_card_bodies_collapse_with_a_more_marker_until_expanded() {
    // The collapse threshold is a backstop for pathological bodies (100+
    // rendered lines), so the fixture body must clear it: 60 paragraphs
    // render to 120 lines.
    let long_body: String = (1..=60).map(|n| format!("Para {n}\n\n")).collect();
    let mut model = model_in_detail(sample_pr(), "The description.");
    seed_comments(
        &mut model,
        vec![issue_comment(10, "carol", &long_body, false)],
    );

    let rows = buffer_text(&render_snapshot(&model, 80, 40));
    assert!(
        rows.iter().any(|r| r.contains("Para 1")),
        "the collapsed card must show the body's first lines: {rows:?}"
    );

    // Scroll to the bottom (the render backstop clamps the huge offset) so
    // the card's tail is in the viewport.
    if let ViewMode::Detail(detail) = &mut model.view_mode {
        detail.scroll = 10_000;
    }
    let rows = buffer_text(&render_snapshot(&model, 80, 40));
    assert!(
        !rows.iter().any(|r| r.contains("Para 60")),
        "the collapsed card must hide the body's tail: {rows:?}"
    );
    assert!(
        rows.iter().any(|r| r.contains("more line")),
        "a collapsed card must advertise its hidden lines: {rows:?}"
    );

    // Expanding (what Enter toggles) reveals the full body.
    if let ViewMode::Detail(detail) = &mut model.view_mode {
        detail
            .expanded
            .insert("https://example.test/c/10".to_owned());
    }
    let rows = buffer_text(&render_snapshot(&model, 80, 40));
    assert!(
        rows.iter().any(|r| r.contains("Para 60")),
        "an expanded card must show the full body: {rows:?}"
    );
    assert!(
        !rows.iter().any(|r| r.contains("more line")),
        "an expanded card needs no truncation marker: {rows:?}"
    );
}

#[test]
fn detail_hundred_line_card_bodies_render_in_full() {
    // Ordinary long comments must not fold — only the pathological backstop
    // (past 100 rendered lines) truncates.
    let body: String = (1..=49).map(|n| format!("Para {n}\n\n")).collect();
    let mut model = model_in_detail(sample_pr(), "The description.");
    seed_comments(&mut model, vec![issue_comment(10, "carol", &body, false)]);

    if let ViewMode::Detail(detail) = &mut model.view_mode {
        detail.scroll = 10_000;
    }
    let rows = buffer_text(&render_snapshot(&model, 80, 40));

    assert!(
        rows.iter().any(|r| r.contains("Para 49")),
        "a sub-threshold body must render to its end: {rows:?}"
    );
    assert!(
        !rows.iter().any(|r| r.contains("more line")),
        "a sub-threshold body must not truncate: {rows:?}"
    );
}

#[test]
fn detail_long_body_lines_wrap_to_the_terminal_width() {
    // Markdown bodies wrap at layout time (bylines and headers clip instead,
    // like the TS truncate rows), so a long paragraph must reach its last
    // word across multiple rows rather than being clipped at the right edge.
    let long_paragraph = "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima";
    let mut model = model_in_detail(sample_pr(), long_paragraph);
    seed_comments(
        &mut model,
        vec![issue_comment(10, "carol", long_paragraph, false)],
    );

    let rows = buffer_text(&render_snapshot(&model, 40, 30));

    let description_first = row_of(&rows, "alpha");
    let description_last = row_of(&rows, "lima");
    assert!(
        description_last > description_first,
        "the description must wrap onto continuation rows: {rows:?}"
    );
    // The comment body repeats the paragraph: its words must all survive too
    // (two "lima" rows in total — description + card).
    assert_eq!(
        rows.iter().filter(|r| r.contains("lima")).count(),
        2,
        "the card body must wrap instead of clipping its tail: {rows:?}"
    );
}

#[test]
fn detail_short_card_bodies_never_show_a_marker() {
    let mut model = model_in_detail(sample_pr(), "The description.");
    seed_comments(
        &mut model,
        vec![issue_comment(10, "carol", "Short and sweet.", false)],
    );

    let rows = buffer_text(&render_snapshot(&model, 80, 30));

    assert!(
        rows.iter().any(|r| r.contains("Short and sweet.")),
        "short bodies render in full: {rows:?}"
    );
    assert!(
        !rows.iter().any(|r| r.contains("more line")),
        "short bodies never truncate: {rows:?}"
    );
}

// ── Resolved / bot filters ──────────────────────────────────────────────────

/// One unresolved + one resolved thread, for the t-toggle tests.
fn model_with_mixed_resolution_threads() -> Model {
    let mut model = model_in_detail(sample_pr(), "The description.");
    seed_threads(
        &mut model,
        vec![
            thread(
                "open",
                "src/lib.rs",
                Some(12),
                false,
                vec![review_comment("c1", "alice", "Please rename this.")],
            ),
            thread(
                "done",
                "src/main.rs",
                Some(3),
                true,
                vec![review_comment("c2", "bob", "Fixed in the next commit.")],
            ),
        ],
    );
    model
}

#[test]
fn detail_hides_resolved_threads_by_default_and_counts_them_as_hidden() {
    let model = model_with_mixed_resolution_threads();

    let rows = buffer_text(&render_snapshot(&model, 80, 30));

    assert!(
        rows.iter().any(|r| r.contains("1 shown · 1 hidden")),
        "header must count the hidden resolved thread: {rows:?}"
    );
    assert!(
        rows.iter().any(|r| r.contains("src/lib.rs:12")),
        "the unresolved thread must render: {rows:?}"
    );
    assert!(
        !rows.iter().any(|r| r.contains("src/main.rs:3")),
        "the resolved thread must be hidden by default: {rows:?}"
    );
}

#[test]
fn detail_shows_resolved_threads_when_toggled_on() {
    let mut model = model_with_mixed_resolution_threads();
    model.show_resolved = true;

    let rows = buffer_text(&render_snapshot(&model, 80, 30));

    assert!(
        rows.iter().any(|r| r.contains("2 shown")),
        "header must count both threads: {rows:?}"
    );
    assert!(
        rows.iter().any(|r| r.contains("src/main.rs:3")),
        "the resolved thread must render when shown: {rows:?}"
    );
    assert!(
        rows.iter().any(|r| r.contains("✓ resolved")),
        "the resolved badge must render: {rows:?}"
    );
}

#[test]
fn detail_hiding_bots_drops_bot_threads_replies_and_comments() {
    let mut model = model_in_detail(sample_pr(), "The description.");
    let bot_comment = |id: &str, body: &str| ReviewComment {
        is_bot: true,
        ..review_comment(id, "linter", body)
    };
    seed_threads(
        &mut model,
        vec![
            // Human root with a bot reply: the reply disappears.
            thread(
                "mixed",
                "src/lib.rs",
                Some(12),
                false,
                vec![
                    review_comment("c1", "alice", "Please rename this."),
                    bot_comment("b1", "Lint: unused variable."),
                ],
            ),
            // Bot-only thread: hidden entirely.
            thread(
                "botonly",
                "src/main.rs",
                Some(3),
                false,
                vec![bot_comment("b2", "Coverage decreased.")],
            ),
        ],
    );
    seed_comments(
        &mut model,
        vec![
            issue_comment(10, "carol", "Looks good overall.", false),
            issue_comment(11, "ci-reporter", "Coverage: 98%", true),
        ],
    );
    model.show_bot_comments = false;

    let rows = buffer_text(&render_snapshot(&model, 80, 36));

    assert!(
        rows.iter().any(|r| r.contains("1 shown · 1 hidden")),
        "the bot-only thread must count as hidden: {rows:?}"
    );
    assert!(
        !rows.iter().any(|r| r.contains("Lint: unused variable.")),
        "bot replies must be hidden: {rows:?}"
    );
    assert!(
        !rows.iter().any(|r| r.contains("src/main.rs:3")),
        "the bot-only thread must be hidden: {rows:?}"
    );
    assert!(
        !rows.iter().any(|r| r.contains("ci-reporter")),
        "bot issue comments must be hidden: {rows:?}"
    );
    let conversation = &rows[row_of(&rows, "Conversation")];
    assert!(
        conversation.contains("1 shown · 1 hidden"),
        "the conversation header must count visible and hidden comments: {conversation:?}"
    );
}

#[test]
fn detail_all_bot_conversation_hidden_shows_counts_and_placeholder() {
    // Comments arrived but every one is bot-filtered: the header must still
    // count them as hidden (not render a dangling zero) and say why the
    // section is empty.
    let mut model = model_in_detail(sample_pr(), "The description.");
    seed_comments(
        &mut model,
        vec![
            issue_comment(10, "ci-reporter", "Coverage: 98%", true),
            issue_comment(11, "release-bot", "Preview deployed.", true),
        ],
    );
    model.show_bot_comments = false;

    let rows = buffer_text(&render_snapshot(&model, 80, 30));

    let conversation = &rows[row_of(&rows, "Conversation")];
    assert!(
        conversation.contains("0 shown · 2 hidden"),
        "the header must tally the bot-hidden comments: {conversation:?}"
    );
    assert!(
        rows.iter().any(|r| r.contains("All comments hidden.")),
        "an all-hidden conversation must say why it is empty: {rows:?}"
    );
}

#[test]
fn detail_status_bar_shows_focus_filter_and_open_hints() {
    let model = model_in_detail(sample_pr(), "");

    let rows = buffer_text(&render_snapshot(&model, 100, 10));
    let status = rows.last().expect("status row");

    for hint in ["o", "t", "b"] {
        assert!(
            status.split_whitespace().any(|word| word == hint),
            "status bar must mention the {hint} key: {status:?}"
        );
    }
}

#[test]
fn detail_status_bar_shows_transient_status_messages() {
    // A CommandFailed raised while the detail view is open (e.g. a failed `o`)
    // must be visible in the detail status bar — not only back in the list,
    // where the scheduled clear may wipe it before the user returns.
    let mut model = model_in_detail(sample_pr(), "");
    model.status = Some(crate::app::model::StatusMessage {
        kind: crate::app::model::StatusKind::Error,
        text: "open url: spawn browser opener failed".to_owned(),
    });

    let rows = buffer_text(&render_snapshot(&model, 100, 10));

    let status = rows.last().expect("status row");
    assert!(
        status.contains("open url: spawn browser opener failed"),
        "the transient status must render in the detail status bar: {status:?}"
    );
}

#[test]
fn detail_status_bar_shows_jk_focus_hint() {
    let model = model_in_detail(sample_pr(), "");

    let terminal = render_snapshot(&model, 80, 10);
    let rows = buffer_text(&terminal);

    let status = rows.last().expect("status row");
    assert!(
        status.contains("j/k"),
        "status bar must mention j/k scroll hint: {status:?}"
    );
}
