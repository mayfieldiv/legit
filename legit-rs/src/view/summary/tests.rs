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
