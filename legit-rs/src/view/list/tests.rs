use chrono::{DateTime, TimeZone, Utc};
use ratatui::{Terminal, backend::TestBackend};

use crate::{
    app::{model::Model, pr_list::PrList},
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

/// Build a `PrList` containing `prs`, in the Loaded phase. Mirrors the steady
/// state the runtime reaches after `Msg::PrListLoaded` lands.
fn pr_list_with(prs: Vec<PR>) -> PrList {
    let mut list = PrList::new();
    list.begin_fetch();
    for pr in prs {
        list.push(pr);
    }
    list.complete_fetch();
    list
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

#[test]
fn empty_pr_list_renders_no_open_pull_requests_placeholder() {
    let (model, _) = Model::new();

    let terminal = render_snapshot(&model, 40, 5);

    assert_eq!(
        buffer_text(&terminal),
        vec![
            "          No open pull requests         ",
            "                                        ",
            "                                        ",
            "                                        ",
            "q quit                                  ",
        ]
    );
}

#[test]
fn populated_pr_list_renders_one_row_per_pull_request() {
    let (mut model, _) = Model::new();
    model.list = pr_list_with(vec![
        pr(42, "Add streaming PR list", "octocat", 3),
        pr(43, "Wire FetchOpenPRs cmd", "alice", 26),
        pr(44, "Render list view", "bob", 168),
    ]);

    let terminal = render_snapshot(&model, 60, 5);

    assert_eq!(
        buffer_text(&terminal),
        vec![
            "#42  Add streaming PR list      octocat       +5/-3 3h      ",
            "#43  Wire FetchOpenPRs cmd      alice         +5/-3 1d      ",
            "#44  Render list view           bob           +5/-3 7d      ",
            "                                                            ",
            "q quit                                                      ",
        ]
    );
}

#[test]
fn pr_list_error_appears_in_the_status_bar() {
    let (mut model, _) = Model::new();
    model.list.begin_fetch();
    model
        .list
        .fail_fetch("list open PRs: network down".to_owned());

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
fn long_titles_truncate_with_ellipsis_to_fit_column() {
    let (mut model, _) = Model::new();
    model.list = pr_list_with(vec![pr(
        7,
        "This title is intentionally far too long to fit in the column",
        "octocat",
        2,
    )]);

    let terminal = render_snapshot(&model, 60, 3);

    let rows = buffer_text(&terminal);
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
        60,
        "row should fill exact terminal width"
    );
}

#[test]
fn draft_pr_is_marked_in_its_row() {
    let (mut model, _) = Model::new();
    let mut draft = pr(50, "Polish things", "octocat", 1);
    draft.is_draft = true;
    model.list = pr_list_with(vec![draft]);

    let terminal = render_snapshot(&model, 60, 3);

    assert_eq!(
        buffer_text(&terminal),
        vec![
            "#50  [draft] Polish things      octocat       +5/-3 1h      ",
            "                                                            ",
            "q quit                                                      ",
        ]
    );
}

#[test]
fn large_diff_size_widens_size_column_for_all_rows() {
    let (mut model, _) = Model::new();
    let mut big = pr(100, "huge diff", "octocat", 1);
    big.additions = 1234;
    big.deletions = 5678;
    model.list = pr_list_with(vec![pr(101, "small diff", "alice", 2), big]);

    let terminal = render_snapshot(&model, 60, 4);
    let rows = buffer_text(&terminal);

    // Both size strings must render in full; neither overflow into the age
    // column nor truncate.
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
    assert_eq!(rows[0].chars().count(), 60);
    assert_eq!(rows[1].chars().count(), 60);
}

#[test]
fn wide_pr_number_widens_num_column_for_all_rows() {
    let (mut model, _) = Model::new();
    model.list = pr_list_with(vec![
        pr(42, "small number", "octocat", 1),
        pr(12345, "huge number", "alice", 2),
    ]);

    let terminal = render_snapshot(&model, 60, 4);
    let rows = buffer_text(&terminal);

    // Both rows should align at the same title column — the wider `#12345`
    // sets the column width for the whole list.
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
    // Row width must still fit the terminal.
    assert_eq!(rows[0].chars().count(), 60);
    assert_eq!(rows[1].chars().count(), 60);
}

#[test]
fn loading_pr_list_renders_loading_placeholder() {
    let (mut model, _) = Model::new();
    model.list.begin_fetch();

    let terminal = render_snapshot(&model, 40, 5);

    assert_eq!(
        buffer_text(&terminal),
        vec![
            "         Loading pull requests…         ",
            "                                        ",
            "                                        ",
            "                                        ",
            "q quit                                  ",
        ]
    );
}
