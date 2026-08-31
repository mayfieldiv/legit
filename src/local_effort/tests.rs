//! Unit tests for local Effort discovery — root probing, worktree fan-out,
//! and the cwd walk. Fixture repos are built in tempdirs — the filesystem
//! (and real git) is exactly the I/O under test. Dialect parsing is
//! [`super::format`]'s concern, tested in its own child module.

use std::fs;
use std::path::Path;

use crate::ticket::EffortRead;

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().expect("fixture paths have parents")).unwrap();
    fs::write(path, content).unwrap();
}

/// Run git in a fixture repo, config-pinned so the environment can't break
/// the fixture (identity, signing).
fn git(dir: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_repo(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "--quiet"]);
    git(dir, &["commit", "--quiet", "--allow-empty", "-m", "root"]);
}

fn repo_config(main_worktree: &Path, roots: Option<&[&str]>) -> crate::config::RepoConfig {
    crate::config::RepoConfig {
        main_worktree_path: Some(main_worktree.to_str().unwrap().to_owned()),
        wayfinder_roots: roots.map(|roots| roots.iter().map(|r| (*r).to_owned()).collect()),
        ..Default::default()
    }
}

fn effort_titles(reads: &[EffortRead]) -> Vec<String> {
    reads
        .iter()
        .map(|read| match read {
            EffortRead::Ready(effort) => effort.title.clone(),
            EffortRead::Degraded { title, .. } => panic!("degraded: {title}"),
        })
        .collect()
}

#[test]
fn a_root_is_a_single_effort_or_contains_effort_subdirectories() {
    let dir = tempfile::tempdir().unwrap();

    // Single-effort root: a map directly inside, filename matched
    // case-insensitively (`MAP.md` exists in the wild).
    let single = dir.path().join(".wayfinder");
    write(&single.join("MAP.md"), "# Single\n");
    write(&single.join("issues/01-a.md"), "# A\n");
    assert_eq!(super::probe_root(&single).unwrap(), vec![single.clone()]);

    // Container root: each subdirectory holding a map is an Effort; other
    // subdirectories (assets and strays) are not.
    let container = dir.path().join("docs/wayfinder");
    write(&container.join("menu-redesign/map.md"), "# Menu\n");
    write(&container.join("approval-polling/map.md"), "# Approval\n");
    write(&container.join("assets/diagram.png"), "not a map");
    write(&container.join("stray.md"), "# Not a map file\n");
    assert_eq!(
        super::probe_root(&container).unwrap(),
        vec![
            container.join("approval-polling"),
            container.join("menu-redesign"),
        ],
        "effort subdirectories in name order; mapless subdirs skipped"
    );

    // A missing root or one with no efforts probes empty, not as an error.
    assert_eq!(
        super::probe_root(&dir.path().join("absent")).unwrap(),
        Vec::<std::path::PathBuf>::new()
    );
    let empty = dir.path().join(".scratch");
    fs::create_dir(&empty).unwrap();
    assert_eq!(
        super::probe_root(&empty).unwrap(),
        Vec::<std::path::PathBuf>::new()
    );
}

#[cfg(unix)]
#[test]
fn an_unreadable_root_errs_rather_than_probing_empty() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let locked = dir.path().join("locked");
    let root = locked.join("docs/wayfinder");
    write(&root.join("alpha/map.md"), "# Alpha\n");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

    let result = super::probe_root(&root);
    // Restore before asserting so the tempdir can clean up either way.
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        result.is_err(),
        "an unreadable root must err, never probe as absent"
    );
}

// ── per-repo discovery ───────────────────────────────────────────────────────

#[test]
fn discovery_fans_out_across_worktrees_and_skips_missing_ones() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main");
    init_repo(&main);
    write(&main.join("docs/wayfinder/alpha/map.md"), "# Alpha\n");

    let linked = dir.path().join("linked");
    git(
        &main,
        &["worktree", "add", "--quiet", linked.to_str().unwrap()],
    );
    write(&linked.join(".scratch/beta/map.md"), "# Beta\n");

    // A worktree whose directory is gone (prunable) is skipped, not an error.
    let vanished = dir.path().join("vanished");
    git(
        &main,
        &["worktree", "add", "--quiet", vanished.to_str().unwrap()],
    );
    fs::remove_dir_all(&vanished).unwrap();

    let reads = super::discover_repo_efforts(&repo_config(&main, None)).unwrap();
    let mut titles = effort_titles(&reads);
    titles.sort();
    assert_eq!(titles, vec!["Alpha".to_owned(), "Beta".to_owned()]);
}

// The linked-worktree analog isn't reachable in a fixture: git itself
// reports a permission-blocked worktree as prunable, so the spec'd
// prunable skip fires first. The probe in worktree_bases guards the
// residue — a listed, non-prunable path whose metadata read still fails.
#[cfg(unix)]
#[test]
fn an_unprobeable_main_worktree_reports_the_probe_error() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let holder = dir.path().join("holder");
    let main = holder.join("main");
    init_repo(&main);
    fs::set_permissions(&holder, fs::Permissions::from_mode(0o000)).unwrap();

    let result = super::discover_repo_efforts(&repo_config(&main, None));
    // Restore before asserting so the tempdir can clean up either way.
    fs::set_permissions(&holder, fs::Permissions::from_mode(0o755)).unwrap();
    let err = format!("{:#}", result.unwrap_err());
    assert!(
        err.contains("probing"),
        "an unprobeable Main Worktree reports the probe failure, not \
         'does not exist': {err}"
    );
}

#[test]
fn configured_roots_replace_the_built_in_probe_list() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main");
    init_repo(&main);
    write(&main.join("docs/wayfinder/builtin/map.md"), "# Built-in\n");
    write(&main.join("custom/maps/custom/map.md"), "# Custom\n");

    let reads = super::discover_repo_efforts(&repo_config(&main, Some(&["custom/maps"]))).unwrap();
    assert_eq!(
        effort_titles(&reads),
        vec!["Custom".to_owned()],
        "explicit wayfinderRoots replace the built-ins, never extend them"
    );
}

#[test]
fn a_non_git_base_is_probed_alone_and_absolute_roots_probe_once() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("plain");
    write(&base.join(".wayfinder/MAP.md"), "# Single\n");

    let shared = dir.path().join("shared-efforts");
    write(&shared.join("gamma/map.md"), "# Gamma\n");
    let roots = [".wayfinder".to_owned(), shared.to_str().unwrap().to_owned()];
    let roots: Vec<&str> = roots.iter().map(String::as_str).collect();

    let reads = super::discover_repo_efforts(&repo_config(&base, Some(&roots))).unwrap();
    let mut titles = effort_titles(&reads);
    titles.sort();
    assert_eq!(titles, vec!["Gamma".to_owned(), "Single".to_owned()]);
}

#[test]
fn a_missing_main_worktree_is_an_error_not_an_empty_discovery() {
    let dir = tempfile::tempdir().unwrap();
    let err =
        super::discover_repo_efforts(&repo_config(&dir.path().join("nope"), None)).unwrap_err();
    assert!(format!("{err:#}").contains("nope"), "{err:#}");
}

#[test]
fn a_repo_without_a_main_worktree_path_discovers_nothing() {
    let repo = crate::config::RepoConfig {
        slug: Some(crate::repo_slug::RepoSlug::new("acme/widgets")),
        ..Default::default()
    };
    assert!(super::discover_repo_efforts(&repo).unwrap().is_empty());
}

// ── cwd discovery ────────────────────────────────────────────────────────────

#[test]
fn cwd_walk_probes_each_level_up_to_the_git_toplevel() {
    let dir = tempfile::tempdir().unwrap();
    // A root above the repo must never be probed.
    write(&dir.path().join("docs/wayfinder/above/map.md"), "# Above\n");
    let repo = dir.path().join("monorepo");
    init_repo(&repo);
    write(&repo.join(".scratch/top/map.md"), "# Top\n");
    write(
        &repo.join("apps/mac-agent/docs/wayfinder/nested/map.md"),
        "# Nested\n",
    );
    let cwd = repo.join("apps/mac-agent/src");
    fs::create_dir_all(&cwd).unwrap();

    let reads = super::discover_cwd_efforts(&cwd, &crate::config::LegitConfig::default()).unwrap();
    let mut titles = effort_titles(&reads);
    titles.sort();
    assert_eq!(
        titles,
        vec!["Nested".to_owned(), "Top".to_owned()],
        "every level from cwd to the toplevel is probed; nothing above it"
    );
}

#[test]
fn a_non_git_cwd_is_probed_alone() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("plain/.wayfinder/map.md"), "# Plain\n");
    write(
        &dir.path().join("docs/wayfinder/parent/map.md"),
        "# Parent\n",
    );
    let cwd = dir.path().join("plain");

    let reads = super::discover_cwd_efforts(&cwd, &crate::config::LegitConfig::default()).unwrap();
    assert_eq!(effort_titles(&reads), vec!["Plain".to_owned()]);
}

#[test]
fn configured_roots_win_for_the_cwd_repo_on_a_path_match() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    init_repo(&repo);
    write(&repo.join("docs/wayfinder/builtin/map.md"), "# Built-in\n");
    write(&repo.join("custom/override/map.md"), "# Override\n");
    let cwd = repo.join("deep/inside");
    fs::create_dir_all(&cwd).unwrap();

    // The entry names the same directory through a dot-ridden spelling; the
    // match is on canonical identity, not string equality.
    let spelled = dir.path().join(".").join("repo");
    let config = crate::config::LegitConfig {
        repos: vec![repo_config(&spelled, Some(&["custom"]))],
        ..Default::default()
    };

    let reads = super::discover_cwd_efforts(&cwd, &config).unwrap();
    assert_eq!(
        effort_titles(&reads),
        vec!["Override".to_owned()],
        "the matched entry's wayfinderRoots replace the built-ins for the walk"
    );
}

#[test]
fn configured_roots_win_for_the_cwd_repo_on_a_slug_match() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("clone");
    init_repo(&repo);
    git(
        &repo,
        &["remote", "add", "origin", "git@github.com:acme/widgets.git"],
    );
    write(&repo.join("docs/wayfinder/builtin/map.md"), "# Built-in\n");
    write(&repo.join("maps/override/map.md"), "# Override\n");

    let config = crate::config::LegitConfig {
        repos: vec![crate::config::RepoConfig {
            // Different casing: slug matching is RepoSlug's, case-insensitive.
            slug: Some(crate::repo_slug::RepoSlug::new("ACME/Widgets")),
            wayfinder_roots: Some(vec!["maps".to_owned()]),
            ..Default::default()
        }],
        ..Default::default()
    };

    let reads = super::discover_cwd_efforts(&repo, &config).unwrap();
    assert_eq!(effort_titles(&reads), vec!["Override".to_owned()]);
}

#[test]
fn a_slugged_entry_still_path_matches_when_the_cwd_has_no_remote() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("clone");
    init_repo(&repo);
    write(&repo.join("docs/wayfinder/builtin/map.md"), "# Built-in\n");
    write(&repo.join("maps/override/map.md"), "# Override\n");

    let config = crate::config::LegitConfig {
        repos: vec![crate::config::RepoConfig {
            slug: Some(crate::repo_slug::RepoSlug::new("acme/widgets")),
            main_worktree_path: Some(repo.to_str().unwrap().to_owned()),
            wayfinder_roots: Some(vec!["maps".to_owned()]),
            ..Default::default()
        }],
        ..Default::default()
    };

    let reads = super::discover_cwd_efforts(&repo, &config).unwrap();
    assert_eq!(
        effort_titles(&reads),
        vec!["Override".to_owned()],
        "matching takes either evidence: no origin remote, but the entry's \
         Main Worktree names the toplevel"
    );
}
