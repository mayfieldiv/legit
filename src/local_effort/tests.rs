//! Unit tests for local Effort discovery and dialect parsing. Fixture
//! Efforts are built in tempdirs from the research corpus's field inventory
//! and edge-case catalog (#106) — the filesystem is exactly the I/O under
//! test, matching `canonical_path`'s posture.

use std::fs;
use std::path::Path;

use super::format::read_effort;
use crate::canonical_path::CanonicalPathBuf;
use crate::ticket::{Claim, Dependency, Effort, EffortKey, EffortRead, TicketKey, TicketState};

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().expect("fixture paths have parents")).unwrap();
    fs::write(path, content).unwrap();
}

fn ready(read: EffortRead) -> Effort {
    match read {
        EffortRead::Ready(effort) => effort,
        EffortRead::Degraded { reason, .. } => panic!("expected Ready, degraded: {reason}"),
    }
}

fn local_ticket_key(path: &Path) -> TicketKey {
    TicketKey::Local {
        path: CanonicalPathBuf::canonicalize(path).expect("fixture file exists"),
    }
}

#[test]
fn parses_an_older_dialect_effort() {
    let dir = tempfile::tempdir().unwrap();
    let effort_dir = dir.path().join("menu-redesign");
    write(
        &effort_dir.join("map.md"),
        "<!-- wayfinder:map -->\n\n# Menu redesign\n\n## Destination\n\nA shipped, discoverable menu.\n\n## Decisions so far\n",
    );
    write(
        &effort_dir.join("tickets/01-inventory.md"),
        "---\nstatus: closed\ntype: research\nassignee: mreynolds\nblocked-by: []\n---\n\n# Inventory the menu items\n\nBody prose.\n\n## Resolution\n\nDone.\n",
    );
    write(
        &effort_dir.join("tickets/02-navigation.md"),
        "---\nstatus: open\ntype: grilling\nblocked-by: [1]\n---\n\n# Settle the navigation model\n",
    );

    let effort = ready(read_effort(&effort_dir).unwrap());

    assert_eq!(
        effort.key,
        EffortKey::Local {
            dir: CanonicalPathBuf::canonicalize(&effort_dir).unwrap()
        }
    );
    assert_eq!(effort.title, "Menu redesign");
    assert_eq!(
        effort.destination.as_deref(),
        Some("A shipped, discoverable menu.")
    );

    let tickets: Vec<_> = effort.tickets().collect();
    assert_eq!(tickets.len(), 2, "one Ticket per file, in filename order");

    let inventory = &tickets[0];
    assert_eq!(
        inventory.key,
        local_ticket_key(&effort_dir.join("tickets/01-inventory.md"))
    );
    assert_eq!(
        inventory.title, "Inventory the menu items",
        "title comes from the H1, not the filename slug"
    );
    assert_eq!(inventory.state, TicketState::Closed);
    assert_eq!(inventory.claim, Some(Claim::By("mreynolds".to_owned())));
    assert_eq!(inventory.ty.0, "research");
    assert_eq!(inventory.dependencies, vec![]);

    let navigation = &tickets[1];
    assert_eq!(navigation.title, "Settle the navigation model");
    assert_eq!(navigation.state, TicketState::Open);
    assert_eq!(navigation.claim, None, "no assignee key means unclaimed");
    assert_eq!(
        navigation.dependencies,
        vec![Dependency::SameEffort(local_ticket_key(
            &effort_dir.join("tickets/01-inventory.md")
        ))],
        "blocked-by numbers resolve to the member file with that numeric prefix"
    );

    let frontier: Vec<_> = effort.frontier().map(|t| t.title.clone()).collect();
    assert_eq!(
        frontier,
        vec!["Settle the navigation model".to_owned()],
        "the closed Dependency target leaves its dependent on the Frontier"
    );
}

#[test]
fn tolerates_the_corpus_edge_cases() {
    let dir = tempfile::tempdir().unwrap();
    let effort_dir = dir.path().join("swift-6-strict-safety");
    write(&effort_dir.join("map.md"), "# Swift 6 strict safety\n");
    // The one real present-but-empty assignee, which also closes under
    // `## Closure` instead of `## Resolution`; superseded is a blockquote
    // callout, never a status.
    write(
        &effort_dir.join("tickets/01-audit.md"),
        "---\nstatus: closed\ntype: research\nassignee:\nblocked-by: []\n---\n\n# Audit the targets\n\n> **Superseded** by the later rescope.\n\n## Closure\n\nFolded into 02.\n",
    );
    // A human-plus-agent assignee value passes through verbatim; an
    // unknown type does too. No H1 anywhere, so the slug is the title.
    write(
        &effort_dir.join("tickets/02-migrate.md"),
        "---\nstatus: open\ntype: spike\nassignee: mreynolds (claude)\nblocked-by: [9]\n---\n\nProse without a heading.\n",
    );

    let effort = ready(read_effort(&effort_dir).unwrap());
    let tickets: Vec<_> = effort.tickets().collect();

    let audit = &tickets[0];
    assert_eq!(
        audit.claim, None,
        "a present-but-empty assignee key is unclaimed"
    );
    assert_eq!(
        audit.state,
        TicketState::Closed,
        "Closure/superseded prose never overrides the status field"
    );

    let migrate = &tickets[1];
    assert_eq!(migrate.title, "02-migrate", "no H1 falls back to the slug");
    assert_eq!(
        migrate.claim,
        Some(Claim::By("mreynolds (claude)".to_owned()))
    );
    assert_eq!(migrate.ty.0, "spike", "unknown Types pass through verbatim");
    assert_eq!(migrate.ty.mode(), crate::ticket::Mode::Either);
    assert_eq!(
        migrate.dependencies,
        vec![Dependency::Unknown {
            raw: "9".to_owned()
        }],
        "a blocked-by ref naming no member file can't be found"
    );
    assert!(
        !effort
            .tickets()
            .nth(1)
            .expect("second ticket exists")
            .is_on_frontier(),
        "an Unknown Dependency keeps the ticket off the Frontier"
    );
}

#[test]
fn external_blocked_by_resolves_to_external_and_unknown_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    // A sibling effort holding the readable target, as the corpus spells it:
    // relative paths into another effort's tickets/ dir.
    let sibling = dir.path().join("menu-redesign");
    write(&sibling.join("map.md"), "# Menu redesign\n");
    write(
        &sibling.join("tickets/04-copy.md"),
        "---\nstatus: closed\ntype: task\n---\n\n# Settle the copy\n",
    );

    let effort_dir = dir.path().join("swift-6-strict-safety");
    write(&effort_dir.join("map.md"), "# Swift 6 strict safety\n");
    write(
        &effort_dir.join("tickets/01-adopt.md"),
        "---\nstatus: open\ntype: task\nexternal-blocked-by:\n  - ../../menu-redesign/tickets/04-copy.md\n  - ../../gone/tickets/02-missing.md\n---\n\n# Adopt strict concurrency\n",
    );

    let effort = ready(read_effort(&effort_dir).unwrap());
    let ticket = effort.tickets().next().expect("one ticket");

    assert_eq!(
        ticket.dependencies,
        vec![
            Dependency::External(crate::ticket::ExternalDependency {
                key: local_ticket_key(&sibling.join("tickets/04-copy.md")),
                state: TicketState::Closed,
                title: Some("Settle the copy".to_owned()),
            }),
            Dependency::Unknown {
                raw: "../../gone/tickets/02-missing.md".to_owned()
            },
        ],
        "a readable target outside the effort is External with its parsed \
         state and title; an unreadable one is Unknown with the raw ref"
    );
    assert!(
        !ticket.is_on_frontier(),
        "the Unknown Dependency keeps the ticket off the Frontier despite \
         the closed External target"
    );
}

#[test]
fn field_lines_after_a_section_heading_are_prose_not_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let effort_dir = dir.path().join("effort");
    write(&effort_dir.join("map.md"), "# Effort\n");
    write(
        &effort_dir.join("issues/01-thing.md"),
        "# The thing\n\nType: task\n\n## Progress\n\nStatus: resolved\nBlocked by: 02\n",
    );

    let effort = ready(read_effort(&effort_dir).unwrap());
    let ticket = effort.tickets().next().expect("one ticket");
    assert_eq!(ticket.state, TicketState::Open);
    assert_eq!(ticket.dependencies, vec![]);
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

// ── per-repo discovery ───────────────────────────────────────────────────────

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

#[test]
fn dialect_vocabularies_do_not_cross() {
    let dir = tempfile::tempdir().unwrap();
    let older = dir.path().join("older");
    write(&older.join("map.md"), "# Older\n");
    write(
        &older.join("tickets/01-a.md"),
        "---\nstatus: resolved\n---\n\n# A\n",
    );
    let (_, _, _, reason) = degraded(read_effort(&older).unwrap());
    assert!(
        reason.contains("resolved"),
        "the older dialect's lifecycle is open/closed only: {reason}"
    );

    let newer = dir.path().join("newer");
    write(&newer.join("map.md"), "# Newer\n");
    write(&newer.join("issues/01-a.md"), "# A\n\nStatus: closed\n");
    let (_, _, _, reason) = degraded(read_effort(&newer).unwrap());
    assert!(
        reason.contains("closed"),
        "the newer dialect's lifecycle is claimed/resolved only: {reason}"
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

fn degraded(read: EffortRead) -> (EffortKey, String, Option<String>, String) {
    match read {
        EffortRead::Ready(effort) => panic!("expected Degraded, parsed {:?}", effort.title),
        EffortRead::Degraded {
            key,
            title,
            destination,
            reason,
        } => (key, title, destination, reason),
    }
}

#[test]
fn an_unparseable_ticket_degrades_the_effort_keeping_map_context() {
    let dir = tempfile::tempdir().unwrap();
    let effort_dir = dir.path().join("effort");
    write(
        &effort_dir.join("map.md"),
        "# Real title\n\n## Destination\n\nSomewhere.\n",
    );
    write(&effort_dir.join("tickets/01-fine.md"), "# Fine\n");
    write(
        &effort_dir.join("tickets/02-broken.md"),
        "---\nstatus: open\nnot a yaml line\n---\n\n# Broken\n",
    );

    let (key, title, destination, reason) = degraded(read_effort(&effort_dir).unwrap());
    assert_eq!(
        key,
        EffortKey::Local {
            dir: CanonicalPathBuf::canonicalize(&effort_dir).unwrap()
        }
    );
    assert_eq!(title, "Real title");
    assert_eq!(destination.as_deref(), Some("Somewhere."));
    assert!(
        reason.contains("02-broken.md"),
        "the reason names the failing file: {reason}"
    );
}

#[test]
fn an_unrecognized_status_degrades_rather_than_guessing() {
    let dir = tempfile::tempdir().unwrap();
    let effort_dir = dir.path().join("effort");
    write(&effort_dir.join("map.md"), "# Effort\n");
    write(
        &effort_dir.join("tickets/01-a.md"),
        "---\nstatus: wontfix\n---\n\n# A\n",
    );

    let (_, _, _, reason) = degraded(read_effort(&effort_dir).unwrap());
    assert!(reason.contains("wontfix"), "{reason}");
}

#[test]
fn a_directory_without_a_map_degrades_under_its_dir_name() {
    let dir = tempfile::tempdir().unwrap();
    let effort_dir = dir.path().join("mapless");
    write(&effort_dir.join("tickets/01-a.md"), "# A\n");

    let (_, title, _, reason) = degraded(read_effort(&effort_dir).unwrap());
    assert_eq!(title, "mapless");
    assert!(reason.contains("map.md"), "{reason}");
}

#[test]
fn parses_a_newer_dialect_effort() {
    let dir = tempfile::tempdir().unwrap();
    let effort_dir = dir.path().join("approval-polling");
    write(
        &effort_dir.join("map.md"),
        "# Approval polling\n\n## Destination\n\nPolling shipped.\n",
    );
    write(
        &effort_dir.join("issues/01-schema.md"),
        "# Pick the schema\n\nStatus: resolved\nType: task\n\n## Question\n\nWhich schema?\n",
    );
    write(
        &effort_dir.join("issues/02-endpoint.md"),
        "# Shape the endpoint\n\nStatus: claimed\nType: grilling\nBlocked by: 01\n",
    );
    write(
        &effort_dir.join("issues/03-rollout.md"),
        "# Plan the rollout\n\nType: task\nBlocked by: 01, 02\n",
    );

    let effort = ready(read_effort(&effort_dir).unwrap());
    let tickets: Vec<_> = effort.tickets().collect();
    assert_eq!(tickets.len(), 3);

    let schema = &tickets[0];
    assert_eq!(schema.state, TicketState::Closed, "resolved maps to Closed");
    assert_eq!(schema.claim, None, "a resolved Ticket carries no Claim");
    assert_eq!(schema.ty.0, "task");

    let endpoint = &tickets[1];
    assert_eq!(endpoint.state, TicketState::Open);
    assert_eq!(
        endpoint.claim,
        Some(Claim::Anonymous),
        "the dialect records claimed-ness without a claimant name"
    );
    assert_eq!(
        endpoint.dependencies,
        vec![Dependency::SameEffort(local_ticket_key(
            &effort_dir.join("issues/01-schema.md")
        ))]
    );

    let rollout = &tickets[2];
    assert_eq!(
        (rollout.state, rollout.claim.clone()),
        (TicketState::Open, None),
        "no Status line means Open and unclaimed"
    );
    assert_eq!(rollout.dependencies.len(), 2);

    let frontier: Vec<_> = effort.frontier().map(|t| t.title.clone()).collect();
    assert_eq!(
        frontier,
        Vec::<String>::new(),
        "rollout waits on the open claimed endpoint; endpoint is claimed"
    );
}
