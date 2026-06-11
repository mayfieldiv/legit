use chrono::{DateTime, TimeZone, Utc};
use ratatui::{Terminal, backend::TestBackend, style::Color};

use crate::{
    app::{
        grouping::Grouping,
        model::{Model, RepoDetection, StatusKind, StatusMessage},
    },
    blocker::{BlockerResult, Tier},
    git_remote::RepoInfo,
    github::limiter::NetworkStats,
    github::rest::{PR, PRState},
    view,
};

fn fixed_now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 20, 12, 0, 0).unwrap()
}

fn render_snapshot(model: &Model, width: u16, height: u16) -> Terminal<TestBackend> {
    render_snapshot_at(model, width, height, fixed_now())
}

fn render_snapshot_at(
    model: &Model,
    width: u16,
    height: u16,
    now: DateTime<Utc>,
) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| view::view(model, frame, now))
        .expect("draw");
    terminal
}

fn pr(number: u64, title: &str, author: &str, hours_ago: i64) -> PR {
    let created_at = fixed_now() - chrono::Duration::hours(hours_ago);
    PR {
        number,
        repo_slug: "acme/web".to_owned(),
        title: title.to_owned(),
        author: author.to_owned(),
        created_at,
        updated_at: created_at,
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
        head_ref: format!("feat/{number}"),
        base_ref: "main".to_owned(),
        head_repository_owner: "mayfieldiv".to_owned(),
        state: PRState::Open,
    }
}

/// A model whose Open PR List holds `prs` (Loaded phase) under `grouping`, with
/// each PR's blocker pre-seeded from `tier_of` so the smart-status grouping has
/// something to group by. `tier_of` returns `None` for a PR that should land in
/// "Loading details…". Drives the same `relayout` path the runtime uses.
fn model_with(prs: Vec<PR>, grouping: Grouping, tier_of: impl Fn(&PR) -> Option<Tier>) -> Model {
    let (mut model, _) = Model::new();
    model.repo = RepoDetection::Detected(RepoInfo {
        owner: "acme".to_owned(),
        repo: "web".to_owned(),
    });
    model.list.begin_fetch("acme/web");
    for pr in prs {
        if let Some(tier) = tier_of(&pr) {
            model.blockers.insert(
                pr.key(),
                BlockerResult {
                    blocker: "someone".to_owned(),
                    tier,
                    reason: reason_for(tier),
                },
            );
        }
        model.list.push(pr);
    }
    model.list.complete_fetch("acme/web");
    set_grouping(&mut model, grouping);
    model
}

/// A short, recognisable reason per tier for snapshot assertions.
fn reason_for(tier: Tier) -> String {
    match tier {
        Tier::MeBlocking => "You are a requested reviewer".to_owned(),
        Tier::NeedsReview => "Awaiting review".to_owned(),
        Tier::WaitingOnAuthor => "Draft".to_owned(),
    }
}

/// Cycle the list's grouping to `target` (from the SmartStatus default) and
/// rebuild the layout. `cycle_grouping` resets the selection, so callers that
/// care about selection set it afterwards.
fn set_grouping(model: &mut Model, target: Grouping) {
    while model.list.grouping() != target {
        model.list.cycle_grouping();
    }
    model.relayout();
}

/// Extract the rendered buffer's text as one string per row. Snapshot tests
/// focus on the text layout the user sees; cell styling (colors, BOLD, etc.)
/// is left to manual review.
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

/// Rows of the list region only: excluding the tab bar (first row), the status
/// bar (last row), and — at >=80 columns — the summary panel that splits off the
/// right of the row. Slicing to the list columns keeps these list-layout
/// assertions about the list alone, independent of the panel beside it.
fn list_rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
    let width = terminal.backend().buffer().area().width;
    let summary_and_divider = view::summary::panel_width(width).map_or(0, |w| w + 1);
    let list_width = width - summary_and_divider;
    let mut rows: Vec<String> = buffer_text(terminal)
        .into_iter()
        .map(|row| row.chars().take(list_width as usize).collect())
        .collect();
    rows.pop();
    rows.remove(0);
    rows
}

#[test]
fn empty_pr_list_renders_no_open_pull_requests_placeholder() {
    let (model, _) = Model::new();

    let terminal = render_snapshot(&model, 40, 5);

    assert_eq!(
        buffer_text(&terminal),
        vec![
            "[All]                                   ",
            "          No open pull requests         ",
            "                                        ",
            "                                        ",
            "q quit  g group: smart-status  h/l tabs ",
        ]
    );
}

#[test]
fn flat_list_renders_one_row_per_pull_request() {
    let model = model_with(
        vec![
            pr(42, "Add streaming PR list", "octocat", 3),
            pr(43, "Wire FetchOpenPRs cmd", "alice", 26),
            pr(44, "Render list view", "bob", 168),
        ],
        Grouping::None,
        |_| Some(Tier::NeedsReview),
    );

    // Render at 116 so the list region is 79 columns wide (the panel takes the
    // right 36 plus a 1-cell divider); the list-layout assertions stay about
    // the list alone.
    let terminal = render_snapshot(&model, 116, 5);

    assert_eq!(
        list_rows(&terminal),
        vec![
            "#42   Add streaming PR l… octocat        +5/-3  3h     Awaiting review         ",
            "#43   Wire FetchOpenPRs … alice          +5/-3  1d     Awaiting review         ",
            "#44   Render list view    bob            +5/-3  7d     Awaiting review         ",
        ]
    );
}

#[test]
fn smart_status_grouping_renders_a_header_per_tier_in_order() {
    let model = model_with(
        vec![
            pr(1, "Waiting one", "carol", 1),
            pr(2, "Needs one", "dave", 2),
            pr(3, "Me one", "erin", 3),
            pr(4, "Needs two", "frank", 4),
        ],
        Grouping::SmartStatus,
        |pr| {
            Some(match pr.number {
                1 => Tier::WaitingOnAuthor,
                3 => Tier::MeBlocking,
                _ => Tier::NeedsReview,
            })
        },
    );

    let terminal = render_snapshot(&model, 80, 9);
    let rows = list_rows(&terminal);

    // Headers appear in tier order, each above its PRs; needs-review groups two.
    assert!(rows[0].starts_with("── Me blocking "), "{rows:?}");
    assert!(rows[1].contains("#3"), "{rows:?}");
    assert!(rows[2].starts_with("── Needs review "), "{rows:?}");
    assert!(rows[3].contains("#2"), "{rows:?}");
    assert!(rows[4].contains("#4"), "{rows:?}");
    assert!(rows[5].starts_with("── Waiting on author "), "{rows:?}");
    assert!(rows[6].contains("#1"), "{rows:?}");
}

#[test]
fn smart_status_grouping_omits_empty_tiers_single_tier_list() {
    // All needs-review -> a single group, no Me blocking / Waiting headers.
    let model = model_with(
        vec![pr(1, "one", "carol", 1), pr(2, "two", "dave", 2)],
        Grouping::SmartStatus,
        |_| Some(Tier::NeedsReview),
    );

    let terminal = render_snapshot(&model, 80, 6);
    let rows = list_rows(&terminal);

    assert!(rows[0].starts_with("── Needs review "), "{rows:?}");
    assert!(rows[1].contains("#1"), "{rows:?}");
    assert!(rows[2].contains("#2"), "{rows:?}");
    let headers = rows.iter().filter(|r| r.starts_with("──")).count();
    assert_eq!(
        headers, 1,
        "only the populated tier gets a header: {rows:?}"
    );
}

#[test]
fn smart_status_undelivered_blockers_render_under_loading_details() {
    let model = model_with(
        vec![pr(1, "derived", "carol", 1), pr(2, "pending", "dave", 2)],
        Grouping::SmartStatus,
        |pr| (pr.number == 1).then_some(Tier::NeedsReview),
    );

    let terminal = render_snapshot(&model, 80, 6);
    let rows = list_rows(&terminal);

    assert!(rows[0].starts_with("── Needs review "), "{rows:?}");
    assert!(rows[1].contains("#1"), "{rows:?}");
    assert!(rows[2].starts_with("── Loading details… "), "{rows:?}");
    assert!(rows[3].contains("#2"), "{rows:?}");
    // The pending PR shows the "…" placeholder reason hint.
    assert!(rows[3].trim_end().ends_with('…'), "{rows:?}");
}

#[test]
fn repo_grouping_renders_one_header_for_the_detected_repo() {
    let model = model_with(
        vec![pr(1, "one", "carol", 1), pr(2, "two", "dave", 2)],
        Grouping::Repo,
        |_| Some(Tier::NeedsReview),
    );

    let terminal = render_snapshot(&model, 80, 6);
    let rows = list_rows(&terminal);

    assert!(rows[0].starts_with("── acme/web "), "{rows:?}");
    assert!(rows[1].contains("#1"), "{rows:?}");
    assert!(rows[2].contains("#2"), "{rows:?}");
}

#[test]
fn all_tab_grouped_by_repo_shows_one_header_per_repo() {
    let mut other = pr(2, "two", "dave", 2);
    other.repo_slug = "zeta/api".to_owned();
    let model = model_with(
        vec![pr(1, "one", "carol", 1), other],
        Grouping::Repo,
        |_| Some(Tier::NeedsReview),
    );

    let terminal = render_snapshot(&model, 80, 7);
    let rows = list_rows(&terminal);

    assert!(rows[0].starts_with("── acme/web "), "{rows:?}");
    assert!(rows[1].contains("#1"), "{rows:?}");
    assert!(rows[2].starts_with("── zeta/api "), "{rows:?}");
    assert!(rows[3].contains("#2"), "{rows:?}");
}

#[test]
fn all_tab_multi_repo_rows_show_the_repo_column() {
    let mut other = pr(2, "two", "dave", 2);
    other.repo_slug = "zeta/api".to_owned();
    let model = model_with(
        vec![pr(1, "one", "carol", 1), other],
        Grouping::None,
        |_| Some(Tier::NeedsReview),
    );

    let terminal = render_snapshot(&model, 136, 4);
    let rows = list_rows(&terminal);

    assert!(
        rows[0].contains("web") && rows[1].contains("api"),
        "All-tab rows should include the short repo name: {rows:?}"
    );
}

#[test]
fn list_and_summary_are_separated_by_a_divider_cell() {
    let model = model_with(vec![pr(1, "one", "carol", 1)], Grouping::None, |_| {
        Some(Tier::NeedsReview)
    });
    let width = 116;
    let terminal = render_snapshot(&model, width, 4);
    let panel_width = view::summary::panel_width(width).expect("panel visible");
    let divider_x = width - panel_width - 1;

    let buffer = terminal.backend().buffer();
    assert_eq!(
        buffer[(divider_x, 1)].symbol(),
        "│",
        "list and summary should not touch with no separator"
    );
}

#[test]
fn list_cells_use_distinct_ts_parity_colours() {
    let model = model_with(
        vec![
            pr(1, "selected", "carol", 1),
            pr(2, "not selected", "alice", 2),
        ],
        Grouping::None,
        |_| Some(Tier::NeedsReview),
    );
    let terminal = render_snapshot(&model, 116, 4);
    let buffer = terminal.backend().buffer();
    let rows = buffer_text(&terminal);
    let row = &rows[2]; // second PR row: first non-selected list row
    let author_x = row.find("alice").expect("author rendered") as u16;

    assert_eq!(
        buffer[(0, 2)].fg,
        Color::Cyan,
        "PR numbers should use the accent colour"
    );
    assert_eq!(
        buffer[(author_x, 2)].fg,
        Color::Green,
        "author names should use the success colour"
    );
}

// ── repo tabs ──────────────────────────────────────────────────────────────

/// First row of a rendered snapshot — the Repo Tab bar.
fn tab_row(terminal: &Terminal<TestBackend>) -> String {
    buffer_text(terminal).remove(0)
}

/// `model_with` plus a second Tracked Repo (acme/api) in config, so the tab
/// set is `All | acme/api | acme/web`.
fn two_repo_model() -> Model {
    let mut model = model_with(vec![pr(1, "one", "carol", 1)], Grouping::None, |_| {
        Some(Tier::NeedsReview)
    });
    model.config.repos = vec![crate::config::RepoConfig {
        slug: "acme/api".to_owned(),
        ..Default::default()
    }];
    model
}

#[test]
fn tab_bar_lists_all_plus_tracked_repos_with_active_bracketed() {
    let model = two_repo_model();

    let row = tab_row(&render_snapshot(&model, 50, 4));

    assert!(
        row.starts_with("[All]  acme/api   acme/web "),
        "All active: {row:?}"
    );
}

#[test]
fn tab_bar_brackets_follow_the_active_tab() {
    let mut model = two_repo_model();
    model.active_tab = 2; // acme/web (config repo first, detected second)
    model.relayout();

    let row = tab_row(&render_snapshot(&model, 50, 4));

    assert!(
        row.starts_with(" All   acme/api  [acme/web] "),
        "acme/web active: {row:?}"
    );
}

// ── substring filter ────────────────────────────────────────────────────────

/// `model_with` one matching and one non-matching PR plus the filter put into
/// `state` ("editing" or "applied") with `text`.
fn filtered_model(text: &str, editing: bool) -> Model {
    let mut model = model_with(
        vec![
            pr(1, "Fix rust panic", "carol", 1),
            pr(2, "Update docs", "dave", 2),
        ],
        Grouping::None,
        |_| Some(Tier::NeedsReview),
    );
    model.list.filter_open();
    for c in text.chars() {
        model.list.filter_push(c);
    }
    if !editing {
        model.list.filter_submit();
    }
    model.relayout();
    model
}

#[test]
fn filter_editing_renders_chip_with_cursor_and_editor_hints() {
    let model = filtered_model("rust", true);

    let terminal = render_snapshot(&model, 60, 5);
    let rows = buffer_text(&terminal);

    // Row 1 sits under the tab bar: the chip text plus a block cursor.
    assert!(rows[1].starts_with("/rust█"), "{rows:?}");
    assert!(rows[2].contains("#1"), "only the match renders: {rows:?}");
    assert!(
        !rows.iter().any(|r| r.contains("#2")),
        "non-match hidden: {rows:?}"
    );
    let status = rows.last().expect("status row");
    assert!(
        status.starts_with("enter apply  esc clear"),
        "editor hints while editing: {status:?}"
    );
}

#[test]
fn applied_filter_renders_chip_without_cursor_and_normal_hints() {
    let model = filtered_model("rust", false);

    let terminal = render_snapshot(&model, 60, 5);
    let rows = buffer_text(&terminal);

    assert!(rows[1].starts_with("/rust "), "{rows:?}");
    assert!(!rows[1].contains('█'), "no cursor once applied: {rows:?}");
    let status = rows.last().expect("status row");
    assert!(status.starts_with("q quit"), "normal hints: {status:?}");
}

#[test]
fn filter_with_no_matches_renders_no_matching_prs() {
    let model = filtered_model("zzz", true);

    let terminal = render_snapshot(&model, 60, 5);
    let rows = buffer_text(&terminal);

    assert!(
        rows.iter().any(|r| r.contains("No matching PRs")),
        "{rows:?}"
    );
}

#[test]
fn no_grouping_renders_no_headers() {
    let model = model_with(
        vec![pr(1, "one", "carol", 1), pr(2, "two", "dave", 2)],
        Grouping::None,
        |_| Some(Tier::NeedsReview),
    );

    let terminal = render_snapshot(&model, 80, 6);
    let rows = list_rows(&terminal);

    assert!(
        !rows.iter().any(|r| r.starts_with("──")),
        "no grouping must not emit headers: {rows:?}"
    );
    assert!(rows[0].contains("#1"), "{rows:?}");
    assert!(rows[1].contains("#2"), "{rows:?}");
}

#[test]
fn empty_list_with_smart_status_grouping_shows_placeholder() {
    let model = model_with(Vec::new(), Grouping::SmartStatus, |_| None);

    let terminal = render_snapshot(&model, 40, 3);
    let rows = list_rows(&terminal);

    assert!(
        rows[0].contains("No open pull requests"),
        "empty grouped list shows the placeholder: {rows:?}"
    );
}

#[test]
fn pr_list_error_appears_in_the_status_bar() {
    let (mut model, _) = Model::new();
    model.list.begin_fetch("acme/web");
    model
        .list
        .fail_fetch("acme/web", "list open PRs: network down".to_owned());

    let terminal = render_snapshot(&model, 60, 3);

    let rows = buffer_text(&terminal);
    let status = rows.last().expect("status row");
    assert!(
        status.contains("list open PRs: network down"),
        "status row should surface the fetch failure: {:?}",
        status,
    );
}

#[test]
fn fatal_error_appears_in_the_status_bar_ahead_of_a_list_failure() {
    let (mut model, _) = Model::new();
    // A per-repo listing failed too; the fatal must win the status bar.
    model.list.begin_fetch("acme/web");
    model
        .list
        .fail_fetch("acme/web", "list open PRs: network down".to_owned());
    model.fatal = Some("config error: invalid bot_logins entry".to_owned());

    let status = status_row(&render_snapshot(&model, 80, 3));

    assert!(
        status.contains("config error: invalid bot_logins entry"),
        "the app-level fatal takes precedence over the list failure: {status:?}"
    );
    assert!(
        !status.contains("network down"),
        "the list failure must be masked by the fatal: {status:?}"
    );
}

#[test]
fn long_titles_truncate_with_ellipsis_to_fit_column() {
    let model = model_with(
        vec![pr(
            7,
            "This title is intentionally far too long to fit in the column",
            "octocat",
            2,
        )],
        Grouping::None,
        |_| Some(Tier::NeedsReview),
    );

    // 116 total -> 79-col list region (panel takes the right 36 plus divider).
    let terminal = render_snapshot(&model, 116, 3);

    let rows = list_rows(&terminal);
    assert!(
        rows[0].contains('…'),
        "expected ellipsis truncation, got row: {:?}",
        rows[0]
    );
    assert!(
        rows[0].contains("octocat"),
        "author column must remain intact: {:?}",
        rows[0]
    );
    assert!(
        rows[0].contains("2h"),
        "age column must remain intact: {:?}",
        rows[0]
    );
    assert_eq!(
        rows[0].chars().count(),
        79,
        "row should fill exact terminal width"
    );
}

#[test]
fn long_author_names_truncate_in_the_middle() {
    let model = model_with(
        vec![pr(7, "Short title", "very-long-author-name", 2)],
        Grouping::None,
        |_| Some(Tier::NeedsReview),
    );

    // 116 total -> 80-col list region (panel takes the right 36).
    let terminal = render_snapshot(&model, 116, 3);
    let rows = list_rows(&terminal);

    assert!(
        rows[0].contains("very-lo…r-name"),
        "author should preserve both ends when truncated: {:?}",
        rows[0]
    );
    assert!(
        !rows[0].contains("very-long-aut…"),
        "author should not use end-only truncation: {:?}",
        rows[0]
    );
}

#[test]
fn draft_pr_is_marked_in_its_row() {
    let mut draft = pr(50, "Polish things", "octocat", 1);
    draft.is_draft = true;
    let model = model_with(vec![draft], Grouping::None, |_| Some(Tier::WaitingOnAuthor));

    // 116 total -> 79-col list region (panel + divider split off the right).
    let terminal = render_snapshot(&model, 116, 3);
    let rows = list_rows(&terminal);

    assert!(rows[0].contains("[draft]"), "{rows:?}");
    assert!(rows[0].contains("Polish"), "{rows:?}");
    assert!(rows[0].contains("octocat"), "{rows:?}");
}

#[test]
fn large_diff_size_widens_size_column_for_all_rows() {
    let mut big = pr(100, "huge diff", "octocat", 1);
    big.additions = 1234;
    big.deletions = 5678;
    let model = model_with(
        vec![pr(101, "small diff", "alice", 2), big],
        Grouping::None,
        |_| Some(Tier::NeedsReview),
    );

    // 126 total -> 89-col list region (panel takes the right 36 plus divider).
    let terminal = render_snapshot(&model, 126, 4);
    let rows = list_rows(&terminal);

    assert!(
        rows[0].contains("+5/-3"),
        "small-diff size must render in full: {:?}",
        rows[0]
    );
    assert!(
        rows[1].contains("+1234/-5678"),
        "large-diff size must render in full: {:?}",
        rows[1]
    );
    assert_eq!(rows[0].chars().count(), 89);
    assert_eq!(rows[1].chars().count(), 89);
}

#[test]
fn wide_pr_number_widens_num_column_for_all_rows() {
    let model = model_with(
        vec![
            pr(42, "small number", "octocat", 1),
            pr(12345, "huge number", "alice", 2),
        ],
        Grouping::None,
        |_| Some(Tier::NeedsReview),
    );

    // 126 total -> 89-col list region (panel takes the right 36 plus divider).
    let terminal = render_snapshot(&model, 126, 4);
    let rows = list_rows(&terminal);

    let title_start = rows[0]
        .find("small number")
        .expect("first row should contain title");
    let title_start_2 = rows[1]
        .find("huge number")
        .expect("second row should contain title");
    assert_eq!(
        title_start, title_start_2,
        "title columns must align; got row1={:?} row2={:?}",
        rows[0], rows[1]
    );
    assert_eq!(rows[0].chars().count(), 89);
    assert_eq!(rows[1].chars().count(), 89);
}

#[test]
fn loading_pr_list_renders_loading_placeholder() {
    let (mut model, _) = Model::new();
    model.list.begin_fetch("acme/web");

    let terminal = render_snapshot(&model, 40, 5);

    assert_eq!(
        buffer_text(&terminal),
        vec![
            "[All]                                   ",
            "         Loading pull requests…         ",
            "                                        ",
            "                                        ",
            "q quit  g group: smart-status  h/l tabs ",
        ]
    );
}

/// Last row of a rendered snapshot — the status bar.
fn status_row(terminal: &Terminal<TestBackend>) -> String {
    buffer_text(terminal).pop().expect("status row")
}

#[test]
fn status_bar_shows_grouping_mode_and_cycles_with_g() {
    let model = model_with(vec![pr(1, "a", "octocat", 1)], Grouping::Repo, |_| {
        Some(Tier::NeedsReview)
    });

    let status = status_row(&render_snapshot(&model, 60, 3));

    assert!(status.contains("g group: repo"), "shows mode: {status:?}");
}

#[test]
fn status_bar_with_no_network_activity_shows_only_hints() {
    let model = model_with(vec![pr(1, "a", "octocat", 1)], Grouping::None, |_| {
        Some(Tier::NeedsReview)
    });

    let status = status_row(&render_snapshot(&model, 60, 3));

    assert!(status.starts_with("q quit"), "hints at col 0: {status:?}");
    assert!(
        !status.contains("in flight"),
        "no indicator when idle: {status:?}"
    );
}

#[test]
fn status_bar_shows_in_flight_and_waiting_counts() {
    let mut model = model_with(vec![pr(1, "a", "octocat", 1)], Grouping::None, |_| {
        Some(Tier::NeedsReview)
    });
    model.network_stats = NetworkStats {
        in_flight: 3,
        waiting: 5,
    };

    let status = status_row(&render_snapshot(&model, 60, 3));

    assert!(
        status.contains("[3 in flight, 5 waiting]"),
        "indicator shows both counts: {status:?}"
    );
    assert!(status.contains("q quit"), "hints still present: {status:?}");
}

#[test]
fn status_bar_shows_in_flight_only_when_nothing_waiting() {
    let mut model = model_with(vec![pr(1, "a", "octocat", 1)], Grouping::None, |_| {
        Some(Tier::NeedsReview)
    });
    model.network_stats = NetworkStats {
        in_flight: 2,
        waiting: 0,
    };

    let status = status_row(&render_snapshot(&model, 60, 3));

    assert!(
        status.contains("[2 in flight]"),
        "no waiting segment when zero queued: {status:?}"
    );
    assert!(!status.contains("waiting"), "waiting omitted: {status:?}");
}

#[test]
fn status_bar_shows_info_message_on_the_right() {
    let mut model = model_with(vec![pr(1, "a", "octocat", 1)], Grouping::None, |_| {
        Some(Tier::NeedsReview)
    });
    model.status = Some(StatusMessage {
        kind: StatusKind::Info,
        text: "loading details".to_owned(),
    });

    let status = status_row(&render_snapshot(&model, 80, 3));

    assert!(
        status.starts_with("q quit"),
        "hints on the left: {status:?}"
    );
    assert!(
        status.trim_end().ends_with("loading details"),
        "info message on the right: {status:?}"
    );
}

#[test]
fn status_bar_shows_error_message_on_the_right() {
    let mut model = model_with(vec![pr(1, "a", "octocat", 1)], Grouping::None, |_| {
        Some(Tier::NeedsReview)
    });
    model.status = Some(StatusMessage {
        kind: StatusKind::Error,
        text: "fetch review status: 500".to_owned(),
    });

    let status = status_row(&render_snapshot(&model, 80, 3));

    assert!(
        status.contains("fetch review status: 500"),
        "error message rendered: {status:?}"
    );
}

#[test]
fn narrow_width_empties_age_rather_than_overflowing_the_row() {
    // Choose a width where the title clamps to its 1-column floor and the age
    // column saturates to 0. The age must then render empty, not pass the full
    // age string through and overflow into the trailing reason cell — a row
    // must never render wider than its width.
    let pr_num_col = 6;
    let size_col = 8;
    let column_count = 6;
    let gaps = column_count - 1;
    let width =
        (pr_num_col + super::AUTHOR_COL + size_col + super::AGE_COL + super::REASON_COL + gaps + 1)
            as u16;

    let pr = pr(
        1234,
        "a title far too long to fit in this row",
        "octocat",
        72,
    );
    let blocker = BlockerResult {
        blocker: "someone".to_owned(),
        tier: Tier::NeedsReview,
        reason: "needs review".to_owned(),
    };
    let line = super::row_line(
        &pr,
        Some(&blocker),
        width,
        pr_num_col,
        size_col,
        false,
        fixed_now(),
        false,
    );

    assert!(
        line.width() <= width as usize,
        "row overflowed its width: {} > {width}",
        line.width(),
    );
}
