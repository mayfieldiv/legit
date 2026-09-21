//! Tests for the command layer's Effort discovery — the one place a unit's
//! outcome is turned into messages. Local probes read fixture Efforts built
//! in tempdirs, since the filesystem is exactly what a probe reads; a map
//! read settles from a hand-built `EffortReadBatch`, the transport's one
//! output.

use std::path::Path;

use tokio::sync::mpsc;

use super::{run_discover_cwd_efforts, run_discover_repo_efforts, settle_map_read};
use crate::{
    app::{msg::Msg, ticket_list::DiscoveryUnit},
    canonical_path::CanonicalPathBuf,
    config::{LegitConfig, RepoConfig, RepoIdentity},
    github::wayfinder::EffortReadBatch,
    repo_slug::RepoSlug,
    ticket::{Effort, EffortKey, EffortRead},
};

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// Two Efforts under the built-in `docs/wayfinder` root of `base`.
fn two_efforts_under(base: &Path) {
    write(&base.join("docs/wayfinder/alpha/map.md"), "# Alpha\n");
    write(&base.join("docs/wayfinder/alpha/issues/01-a.md"), "# A\n");
    write(&base.join("docs/wayfinder/beta/map.md"), "# Beta\n");
}

/// Drain every message the probe sent, in order. The sender is dropped by
/// the probe's completion, so the drain terminates.
fn drain(mut rx: mpsc::UnboundedReceiver<Msg>) -> Vec<Msg> {
    let mut msgs = Vec::new();
    while let Ok(msg) = rx.try_recv() {
        msgs.push(msg);
    }
    msgs
}

fn effort_title(read: &EffortRead) -> String {
    match read {
        EffortRead::Ready(effort) => effort.title.clone(),
        EffortRead::Degraded { title, reason, .. } => panic!("{title} degraded: {reason}"),
    }
}

#[tokio::test]
async fn a_repo_probe_streams_one_arrival_per_effort_then_finishes() {
    let dir = tempfile::tempdir().unwrap();
    two_efforts_under(dir.path());
    let repo = RepoConfig {
        main_worktree_path: Some(dir.path().to_str().unwrap().to_owned()),
        ..Default::default()
    };
    let unit = DiscoveryUnit::LocalRepo {
        name: "fixture".to_owned(),
        main_worktree_path: "/src/fixture".to_owned(),
    };
    let (tx, rx) = mpsc::unbounded_channel();

    run_discover_repo_efforts(unit.clone(), repo, tx).await;

    let msgs = drain(rx);
    let identity = RepoIdentity::Path(CanonicalPathBuf::canonicalize(dir.path()).unwrap());
    match msgs.as_slice() {
        [
            Msg::EffortArrived {
                repo: first_repo,
                read: first,
            },
            Msg::EffortArrived {
                repo: second_repo,
                read: second,
            },
            Msg::DiscoveryFinished {
                unit: finished,
                incomplete: None,
            },
        ] => {
            assert_eq!(effort_title(first), "Alpha");
            assert_eq!(effort_title(second), "Beta");
            assert_eq!(first_repo, &identity, "attributed by canonical identity");
            assert_eq!(second_repo, &identity);
            assert_eq!(finished, &unit);
        }
        other => panic!("expected two arrivals then a finish, got {other:?}"),
    }
}

#[tokio::test]
async fn a_missing_main_worktree_fails_the_unit_with_the_path() {
    let repo = RepoConfig {
        main_worktree_path: Some("/nonexistent/legit-fixture".to_owned()),
        ..Default::default()
    };
    let unit = DiscoveryUnit::LocalRepo {
        name: "legit-fixture".to_owned(),
        main_worktree_path: "/src/legit-fixture".to_owned(),
    };
    let (tx, rx) = mpsc::unbounded_channel();

    run_discover_repo_efforts(unit.clone(), repo, tx).await;

    match drain(rx).as_slice() {
        [
            Msg::DiscoveryFailed {
                unit: failed,
                error,
            },
        ] => {
            assert_eq!(failed, &unit);
            assert!(
                error.contains("/nonexistent/legit-fixture"),
                "the error names the missing worktree: {error}"
            );
        }
        other => panic!("expected one failure, got {other:?}"),
    }
}

#[tokio::test]
async fn the_cwd_walk_attributes_to_the_detected_repo_or_the_toplevel() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join(".wayfinder/map.md"), "# Local\n");
    let toplevel = RepoIdentity::Path(CanonicalPathBuf::canonicalize(dir.path()).unwrap());

    let (tx, rx) = mpsc::unbounded_channel();
    run_discover_cwd_efforts(dir.path().to_owned(), None, LegitConfig::default(), tx).await;
    match drain(rx).as_slice() {
        [
            Msg::EffortArrived { repo, read },
            Msg::DiscoveryFinished {
                unit: DiscoveryUnit::Cwd,
                incomplete: None,
            },
        ] => {
            assert_eq!(effort_title(read), "Local");
            assert_eq!(
                repo, &toplevel,
                "no detected repo: the toplevel is the identity"
            );
        }
        other => panic!("expected one arrival then a finish, got {other:?}"),
    }

    let (tx, rx) = mpsc::unbounded_channel();
    run_discover_cwd_efforts(
        dir.path().to_owned(),
        Some(RepoSlug::new("acme/web")),
        LegitConfig::default(),
        tx,
    )
    .await;
    match drain(rx).as_slice() {
        [
            Msg::EffortArrived { repo, .. },
            Msg::DiscoveryFinished { .. },
        ] => {
            assert_eq!(repo, &RepoIdentity::Slug(RepoSlug::new("acme/web")));
        }
        other => panic!("expected one arrival then a finish, got {other:?}"),
    }
}

#[tokio::test]
async fn the_cwd_walk_keeps_a_configured_slug_less_repo_keyed_by_path() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join(".wayfinder/map.md"), "# Local\n");
    let config = LegitConfig {
        repos: vec![RepoConfig {
            main_worktree_path: Some(dir.path().to_str().unwrap().to_owned()),
            ..Default::default()
        }],
        ..Default::default()
    };

    let (tx, rx) = mpsc::unbounded_channel();
    run_discover_cwd_efforts(
        dir.path().to_owned(),
        Some(RepoSlug::new("acme/web")),
        config,
        tx,
    )
    .await;

    match drain(rx).as_slice() {
        [
            Msg::EffortArrived { repo, .. },
            Msg::DiscoveryFinished { .. },
        ] => {
            assert_eq!(
                repo,
                &RepoIdentity::Path(CanonicalPathBuf::canonicalize(dir.path()).unwrap()),
                "the configured entry's identity wins over the detected slug, so \
                 the repo probe and the cwd walk attribute one Effort the same way"
            );
        }
        other => panic!("expected one arrival then a finish, got {other:?}"),
    }
}

// ── GitHub map reads ──────────────────────────────────────────────────────

/// A Ready GitHub Effort with no Tickets — settlement never looks inside.
fn github_effort(slug: &RepoSlug, map_number: u64, title: &str) -> EffortRead {
    EffortRead::Ready(
        Effort::new(
            EffortKey::GitHub {
                repo_slug: slug.clone(),
                map_number,
            },
            title.to_owned(),
            None,
            Vec::new(),
        )
        .unwrap(),
    )
}

#[test]
fn a_map_read_attributes_every_effort_to_the_slug_then_finishes_complete() {
    let slug = RepoSlug::new("acme/web");
    let batch = EffortReadBatch {
        efforts: vec![
            github_effort(&slug, 1, "Alpha"),
            github_effort(&slug, 2, "Beta"),
        ],
        has_more_maps: false,
    };
    let (tx, rx) = mpsc::unbounded_channel();

    settle_map_read(slug.clone(), Ok(batch), &tx);

    match drain(rx).as_slice() {
        [
            Msg::EffortArrived {
                repo: first_repo,
                read: first,
            },
            Msg::EffortArrived {
                repo: second_repo,
                read: second,
            },
            Msg::DiscoveryFinished {
                unit,
                incomplete: None,
            },
        ] => {
            assert_eq!(effort_title(first), "Alpha");
            assert_eq!(effort_title(second), "Beta");
            assert_eq!(
                first_repo,
                &RepoIdentity::Slug(slug.clone()),
                "the slug is the attribution outright"
            );
            assert_eq!(second_repo, &RepoIdentity::Slug(slug.clone()));
            assert_eq!(unit, &DiscoveryUnit::GitHubRepo { slug });
        }
        other => panic!("expected two arrivals then a complete finish, got {other:?}"),
    }
}

#[test]
fn a_map_read_past_the_window_keeps_what_it_read_and_finishes_incomplete() {
    let slug = RepoSlug::new("immense/immybot");
    let batch = EffortReadBatch {
        efforts: vec![github_effort(&slug, 1, "Alpha")],
        has_more_maps: true,
    };
    let (tx, rx) = mpsc::unbounded_channel();

    settle_map_read(slug.clone(), Ok(batch), &tx);

    match drain(rx).as_slice() {
        [
            Msg::EffortArrived { read, .. },
            Msg::DiscoveryFinished {
                unit,
                incomplete: Some(caveat),
            },
        ] => {
            assert_eq!(effort_title(read), "Alpha", "what was read still pools");
            assert_eq!(unit, &DiscoveryUnit::GitHubRepo { slug });
            assert!(caveat.contains("more than 10 open maps"), "{caveat}");
        }
        other => panic!("expected one arrival then an incomplete finish, got {other:?}"),
    }
}

#[test]
fn a_refused_map_read_fails_the_repo_unit() {
    let slug = RepoSlug::new("acme/api");
    let (tx, rx) = mpsc::unbounded_channel();

    settle_map_read(
        slug.clone(),
        Err(anyhow::anyhow!("GitHub GraphQL error: 404 Not Found")),
        &tx,
    );

    match drain(rx).as_slice() {
        [Msg::DiscoveryFailed { unit, error }] => {
            assert_eq!(unit, &DiscoveryUnit::GitHubRepo { slug });
            assert!(error.contains("404 Not Found"), "{error}");
        }
        other => panic!("expected one failure, got {other:?}"),
    }
}

#[test]
fn a_command_name_is_its_variant_without_the_payload() {
    assert_eq!(super::Cmd::LoadConfig.name(), "LoadConfig");
    let cmd = super::Cmd::FetchOpenPRs {
        repo: RepoSlug::new("acme/web"),
        token: crate::auth::AuthToken::parse("secret-token").unwrap(),
    };
    assert_eq!(cmd.name(), "FetchOpenPRs");
}
