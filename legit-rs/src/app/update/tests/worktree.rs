use std::path::PathBuf;

use ratatui::crossterm::event::KeyCode;

use super::*;
use crate::{
    config::{LegitConfig, RepoConfig},
    worktree::WorktreeEntry,
};

fn config_with_source_clone() -> LegitConfig {
    LegitConfig {
        repos: vec![RepoConfig {
            slug: "mayfieldiv/legit".to_owned(),
            source_clone: Some("/src/legit".to_owned()),
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn model_with_selected_pr(config: LegitConfig) -> Model {
    let (mut model, _) = Model::new();
    model.config = config;
    model.repo = RepoDetection::Failed;
    model.list.begin_fetch("mayfieldiv/legit");
    update(&mut model, Msg::PrArrived(sample_pr(1, "p")));
    model
}

fn worktree_entry(path: &str, branch: Option<&str>) -> WorktreeEntry {
    WorktreeEntry {
        path: path.to_owned(),
        head: "a".repeat(40),
        branch_ref: branch.map(|branch| format!("refs/heads/{branch}")),
        branch_name: branch.map(str::to_owned),
        detached: branch.is_none(),
        bare: false,
        locked: None,
        prunable: None,
    }
}

#[test]
fn config_loaded_lists_worktrees_for_repos_with_source_clone() {
    let (mut model, _) = Model::new();

    let cmds = update(&mut model, Msg::ConfigLoaded(config_with_source_clone()));

    match cmds.as_slice() {
        [
            Cmd::ListWorktrees {
                repo_slug,
                source_clone,
            },
        ] => {
            assert_eq!(repo_slug, "mayfieldiv/legit");
            assert_eq!(source_clone, &PathBuf::from("/src/legit"));
        }
        other => panic!("expected one ListWorktrees cmd, got {other:?}"),
    }
}

#[test]
fn worktrees_arrived_stores_entries_and_matches_selected_pr_by_branch() {
    let mut model = model_with_selected_pr(config_with_source_clone());

    update(
        &mut model,
        Msg::WorktreesArrived {
            repo_slug: "mayfieldiv/legit".to_owned(),
            entries: vec![worktree_entry("/tmp/legit-1", Some("feature/1"))],
        },
    );

    let pr = model.list.selected_pr().expect("selected PR");
    let worktree = model.worktree_for_pr(pr).expect("matched worktree");
    assert_eq!(worktree.path, "/tmp/legit-1");
}

#[test]
fn w_in_list_creates_the_selected_pr_worktree() {
    let mut model = model_with_selected_pr(config_with_source_clone());

    let cmds = update(&mut model, key_event(KeyCode::Char('w')));

    let status = model.status.as_ref().expect("status set");
    assert_eq!(status.kind, StatusKind::Info);
    assert_eq!(status.text, "Creating worktree…");
    match cmds.as_slice() {
        [
            Cmd::CreateWorktree {
                pr,
                source_clone,
                target_path,
            },
        ] => {
            assert_eq!(pr, &key(1));
            assert_eq!(source_clone, &PathBuf::from("/src/legit"));
            assert!(
                target_path.ends_with(".legit/worktrees/mayfieldiv/legit/1-feature-1"),
                "target path should be deterministic, got {}",
                target_path.display()
            );
        }
        other => panic!("expected CreateWorktree, got {other:?}"),
    }
}

#[test]
fn w_in_detail_creates_the_current_detail_pr_worktree() {
    let mut model = model_with_selected_pr(config_with_source_clone());
    update(&mut model, key_event(KeyCode::Enter));

    let cmds = update(&mut model, key_event(KeyCode::Char('w')));

    assert!(
        cmds.iter()
            .any(|cmd| matches!(cmd, Cmd::CreateWorktree { pr, .. } if pr == &key(1))),
        "detail w should create the open PR worktree: {cmds:?}"
    );
}

#[test]
fn w_without_source_clone_sets_error_status() {
    let mut model = model_with_selected_pr(LegitConfig::default());

    let cmds = update(&mut model, key_event(KeyCode::Char('w')));

    assert!(
        !cmds
            .iter()
            .any(|cmd| matches!(cmd, Cmd::CreateWorktree { .. })),
        "missing sourceClone must not dispatch create: {cmds:?}"
    );
    let status = model.status.as_ref().expect("status set");
    assert_eq!(status.kind, StatusKind::Error);
    assert!(
        status
            .text
            .contains("No sourceClone configured for mayfieldiv/legit"),
        "status should name the repo: {status:?}"
    );
}

#[test]
fn w_when_worktree_already_matches_reports_existing_path() {
    let mut model = model_with_selected_pr(config_with_source_clone());
    model.worktrees_by_repo.insert(
        "mayfieldiv/legit".to_owned(),
        vec![worktree_entry("/tmp/legit-1", Some("feature/1"))],
    );

    let cmds = update(&mut model, key_event(KeyCode::Char('w')));

    assert!(
        !cmds
            .iter()
            .any(|cmd| matches!(cmd, Cmd::CreateWorktree { .. })),
        "existing worktree should suppress create: {cmds:?}"
    );
    let status = model.status.as_ref().expect("status set");
    assert_eq!(status.kind, StatusKind::Info);
    assert_eq!(status.text, "Copying /tmp/legit-1");
    assert_eq!(
        cmds,
        vec![Cmd::CopyToClipboard {
            text: "/tmp/legit-1".to_owned()
        }]
    );
}

#[test]
fn worktree_created_seeds_cache_and_re_lists_source_clones() {
    let mut model = model_with_selected_pr(config_with_source_clone());
    let path =
        crate::worktree::resolve_worktree_path(&model.config, "mayfieldiv/legit", 1, "feature/1")
            .expect("worktree path")
            .to_string_lossy()
            .to_string();

    let cmds = update(
        &mut model,
        Msg::WorktreeCreated {
            pr: key(1),
            path: path.clone(),
        },
    );

    let pr = model.list.selected_pr().expect("selected PR");
    let worktree = model
        .worktree_for_pr(pr)
        .expect("created path should match by deterministic path");
    assert_eq!(worktree.path, path);
    let status = model.status.as_ref().expect("status set");
    assert_eq!(status.kind, StatusKind::Info);
    assert!(status.text.contains("Copying "));
    assert!(
        cmds.iter()
            .any(|cmd| matches!(cmd, Cmd::CopyToClipboard { text } if text == &path)),
        "create success should copy the worktree path: {cmds:?}"
    );
    assert!(
        cmds.iter()
            .any(|cmd| matches!(cmd, Cmd::ListWorktrees { repo_slug, .. } if repo_slug == "mayfieldiv/legit")),
        "create success should refresh worktree detection: {cmds:?}"
    );
}

#[test]
fn clipboard_copied_sets_success_status() {
    let (mut model, _) = Model::new();

    let cmds = update(
        &mut model,
        Msg::ClipboardCopied {
            text: "/tmp/legit-1".to_owned(),
        },
    );

    assert!(
        cmds.iter()
            .any(|cmd| matches!(cmd, Cmd::ScheduleStatusClear { .. })),
        "success status should schedule a clear: {cmds:?}"
    );
    let status = model.status.as_ref().expect("status set");
    assert_eq!(status.kind, StatusKind::Success);
    assert_eq!(status.text, "Copied /tmp/legit-1");
}

#[test]
fn clipboard_failure_sets_error_status() {
    let (mut model, _) = Model::new();

    let cmds = update(
        &mut model,
        Msg::ClipboardCopyFailed {
            text: "/tmp/legit-1".to_owned(),
            error: "write failed".to_owned(),
        },
    );

    assert!(
        cmds.iter()
            .any(|cmd| matches!(cmd, Cmd::ScheduleStatusClear { .. })),
        "error status should schedule a clear: {cmds:?}"
    );
    let status = model.status.as_ref().expect("status set");
    assert_eq!(status.kind, StatusKind::Error);
    assert_eq!(status.text, "Failed to copy /tmp/legit-1: write failed");
}
