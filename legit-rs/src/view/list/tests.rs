use chrono::{DateTime, TimeZone, Utc};
use ratatui::{Terminal, backend::TestBackend};

use crate::{
    app::model::Model,
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
    model.prs = vec![
        pr(42, "Add streaming PR list", "octocat", 3),
        pr(43, "Wire FetchOpenPRs cmd", "alice", 26),
        pr(44, "Render list view", "bob", 168),
    ];

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
fn draft_pr_is_marked_in_its_row() {
    let (mut model, _) = Model::new();
    let mut draft = pr(50, "Polish things", "octocat", 1);
    draft.is_draft = true;
    model.prs = vec![draft];

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
fn loading_pr_list_renders_loading_placeholder() {
    let (mut model, _) = Model::new();
    model.loading = true;

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
