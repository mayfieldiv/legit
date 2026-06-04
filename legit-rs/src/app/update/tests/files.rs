// ── file fetch on selection + categorisation on arrival ──────────────────

use super::*;
use crate::app::model::FilesState;
use crate::file_category::{FileCategory, FileChange};

/// The `FilesState` recorded for `key(number)`, or `None` when the PR has no
/// `Enrichment::files` entry (never requested).
fn files_state(model: &Model, number: u64) -> Option<&FilesState> {
    model.enrichment.files.get(&key(number))
}

/// A model with auth + repo resolved and `numbers` streamed in via
/// `Msg::PrArrived` (so the list is laid out and a PR is selected).
fn model_with_prs(numbers: &[u64]) -> Model {
    let (mut model, _) = Model::new();
    model.auth_token = Some(Secret::new("ghp_test".to_owned()));
    model.repo = RepoDetection::Detected(RepoInfo {
        owner: "mayfieldiv".to_owned(),
        repo: "legit".to_owned(),
    });
    model.list.begin_fetch("mayfieldiv/legit");
    for n in numbers {
        update(&mut model, Msg::PrArrived(sample_pr(*n, "p")));
    }
    model
}

/// The PR numbers of every `FetchFiles` in `cmds`.
fn file_fetch_numbers(cmds: &[Cmd]) -> Vec<u64> {
    cmds.iter()
        .filter_map(|c| match c {
            Cmd::FetchFiles { number, .. } => Some(*number),
            _ => None,
        })
        .collect()
}

#[test]
fn first_pr_arriving_requests_its_files() {
    // The very first PR becomes selected the moment it arrives, so its files
    // should be fetched without any keypress.
    let (mut model, _) = Model::new();
    model.auth_token = Some(Secret::new("ghp_test".to_owned()));
    model.repo = RepoDetection::Detected(RepoInfo {
        owner: "mayfieldiv".to_owned(),
        repo: "legit".to_owned(),
    });
    model.list.begin_fetch("mayfieldiv/legit");

    let cmds = update(&mut model, Msg::PrArrived(sample_pr(1, "first")));

    assert_eq!(file_fetch_numbers(&cmds), [1]);
    assert!(
        matches!(files_state(&model, 1), Some(FilesState::Requested)),
        "dispatching the fetch records the PR as Requested"
    );
}

#[test]
fn moving_selection_requests_the_newly_selected_prs_files() {
    let mut model = model_with_prs(&[1, 2, 3]);

    let cmds = update(&mut model, key_event(KeyCode::Char('j')));

    assert_eq!(
        file_fetch_numbers(&cmds),
        [2],
        "j selects PR 2, so its files are fetched"
    );
}

#[test]
fn re_selecting_a_pr_does_not_refetch_its_files() {
    let mut model = model_with_prs(&[1, 2]);
    // Select PR 2 (fetches its files), then back to PR 1 (already fetched on
    // arrival), then forward to PR 2 again.
    update(&mut model, key_event(KeyCode::Char('j')));
    update(&mut model, key_event(KeyCode::Char('k')));

    let cmds = update(&mut model, key_event(KeyCode::Char('j')));

    assert!(
        file_fetch_numbers(&cmds).is_empty(),
        "PR 2's files were already requested; no refetch: {cmds:?}"
    );
    assert!(
        matches!(files_state(&model, 2), Some(FilesState::Requested)),
        "PR 2 stays Requested across the re-selection"
    );
}

#[test]
fn a_single_keypress_fetches_at_most_one_prs_files() {
    let mut model = model_with_prs(&[1, 2, 3, 4, 5]);

    let cmds = update(&mut model, key_event(KeyCode::Char('j')));

    assert!(
        file_fetch_numbers(&cmds).len() <= 1,
        "one j must not fan out a fetch per PR: {cmds:?}"
    );
}

#[test]
fn failed_files_fetch_retries_on_reselection() {
    // PR 1's files were requested when it arrived; the request fails.
    let mut model = model_with_prs(&[1, 2]);
    update(&mut model, Msg::FilesFetchFailed { pr: key(1) });
    assert!(
        files_state(&model, 1).is_none(),
        "a failed fetch removes the entry, returning PR 1 to never-requested"
    );

    // Move away and back: selecting PR 1 again must re-dispatch the fetch
    // instead of staying suppressed by the (now-cleared) Requested state.
    update(&mut model, key_event(KeyCode::Char('j')));
    let cmds = update(&mut model, key_event(KeyCode::Char('k')));

    assert_eq!(
        file_fetch_numbers(&cmds),
        [1],
        "a failed fetch must not permanently block a retry"
    );
}

#[test]
fn files_arrived_categorises_and_stores_for_the_pr() {
    let mut model = model_with_prs(&[1]);

    update(
        &mut model,
        Msg::FilesArrived {
            pr: key(1),
            files: vec![
                FileChange {
                    path: "src/app.rs".to_owned(),
                    additions: 10,
                    deletions: 2,
                },
                FileChange {
                    path: "README.md".to_owned(),
                    additions: 3,
                    deletions: 0,
                },
            ],
        },
    );

    let categorization = match files_state(&model, 1) {
        Some(FilesState::Loaded(categorization)) => categorization,
        other => panic!("files arrival must store a Loaded categorization, got {other:?}"),
    };
    assert_eq!(categorization.breakdown.total().files, 2);
    assert_eq!(categorization.breakdown.total().additions, 13);
    assert_eq!(
        categorization.breakdown.stats(FileCategory::Code).files,
        1,
        "src/app.rs is code"
    );
    assert_eq!(
        categorization.breakdown.stats(FileCategory::Docs).files,
        1,
        "README.md is docs"
    );
}

#[test]
fn files_fetch_waits_for_auth() {
    // No auth token: selecting a PR must not dispatch a files fetch.
    let (mut model, _) = Model::new();
    model.repo = RepoDetection::Detected(RepoInfo {
        owner: "mayfieldiv".to_owned(),
        repo: "legit".to_owned(),
    });
    model.list.begin_fetch("mayfieldiv/legit");

    let cmds = update(&mut model, Msg::PrArrived(sample_pr(1, "p")));

    assert!(
        file_fetch_numbers(&cmds).is_empty(),
        "no files fetch before auth resolves: {cmds:?}"
    );
}
