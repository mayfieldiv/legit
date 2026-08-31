//! Unit tests for the local Effort format — dialect parsing and per-Effort
//! degradation. Fixture Efforts are built in tempdirs from the research
//! corpus's field inventory and edge-case catalog (#106) — the filesystem is
//! exactly the I/O under test, matching `canonical_path`'s posture.

use std::fs;
use std::path::Path;

use super::read_effort;
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
    // unknown type does too.
    write(
        &effort_dir.join("tickets/02-migrate.md"),
        "---\nstatus: open\ntype: spike\nassignee: mreynolds (claude)\nblocked-by: [9]\n---\n\n# Migrate the targets\n",
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
fn an_external_ref_to_a_non_ticket_file_is_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let sibling = dir.path().join("menu-redesign");
    write(&sibling.join("map.md"), "# Menu redesign\n");
    write(
        &sibling.join("tickets/assets/notes.md"),
        "# Notes, not a ticket\n",
    );

    let effort_dir = dir.path().join("effort");
    write(&effort_dir.join("map.md"), "# Effort\n");
    write(
        &effort_dir.join("tickets/01-a.md"),
        "---\nstatus: open\nexternal-blocked-by:\n  - ../../menu-redesign/tickets/assets/notes.md\n---\n\n# A\n",
    );

    let effort = ready(read_effort(&effort_dir).unwrap());
    let ticket = effort.tickets().next().expect("one ticket");
    assert_eq!(
        ticket.dependencies,
        vec![Dependency::Unknown {
            raw: "../../menu-redesign/tickets/assets/notes.md".to_owned()
        }],
        "readable markdown outside a ticket directory is never a ticket"
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
fn an_older_dialect_ticket_missing_status_degrades() {
    let dir = tempfile::tempdir().unwrap();
    let effort_dir = dir.path().join("effort");
    write(&effort_dir.join("map.md"), "# Effort\n");
    write(
        &effort_dir.join("tickets/01-a.md"),
        "---\ntype: task\n---\n\n# A\n",
    );

    let (_, _, _, reason) = degraded(read_effort(&effort_dir).unwrap());
    assert!(
        reason.contains("status"),
        "absent-means-Open belongs to the newer dialect alone: {reason}"
    );
}

#[test]
fn a_ticket_without_an_h1_title_degrades() {
    let dir = tempfile::tempdir().unwrap();
    let effort_dir = dir.path().join("effort");
    write(&effort_dir.join("map.md"), "# Effort\n");
    write(
        &effort_dir.join("tickets/01-a.md"),
        "---\nstatus: open\n---\n\nProse without a heading.\n",
    );

    let (_, _, _, reason) = degraded(read_effort(&effort_dir).unwrap());
    assert!(
        reason.contains("no H1 title"),
        "the title is the H1, never the filename slug: {reason}"
    );
}

#[test]
fn duplicate_ticket_numbers_degrade_rather_than_binding_arbitrarily() {
    let dir = tempfile::tempdir().unwrap();
    let effort_dir = dir.path().join("effort");
    write(&effort_dir.join("map.md"), "# Effort\n");
    write(
        &effort_dir.join("tickets/01-first.md"),
        "---\nstatus: closed\n---\n\n# First\n",
    );
    write(
        &effort_dir.join("tickets/01-second.md"),
        "---\nstatus: open\n---\n\n# Second\n",
    );
    write(
        &effort_dir.join("tickets/02-dependent.md"),
        "---\nstatus: open\nblocked-by: [1]\n---\n\n# Dependent\n",
    );

    let (_, _, _, reason) = degraded(read_effort(&effort_dir).unwrap());
    assert!(
        reason.contains("duplicate ticket number 1")
            && reason.contains("01-first.md")
            && reason.contains("01-second.md"),
        "a blocked-by ref to a shared number would bind arbitrarily: {reason}"
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

#[cfg(unix)]
#[test]
fn an_unreadable_ticket_dir_degrades_the_effort() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let effort_dir = dir.path().join("effort");
    write(&effort_dir.join("map.md"), "# Effort\n");
    let ticket_dir = effort_dir.join("tickets");
    write(
        &ticket_dir.join("01-a.md"),
        "---\nstatus: open\n---\n\n# A\n",
    );
    fs::set_permissions(&ticket_dir, fs::Permissions::from_mode(0o000)).unwrap();

    let read = read_effort(&effort_dir).unwrap();
    // Restore before asserting so the tempdir can clean up either way.
    fs::set_permissions(&ticket_dir, fs::Permissions::from_mode(0o755)).unwrap();
    let (_, title, _, reason) = degraded(read);
    assert_eq!(title, "Effort");
    assert!(reason.contains("tickets"), "{reason}");
}
