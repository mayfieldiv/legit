//! Unit tests for local Effort discovery and dialect parsing. Fixture
//! Efforts are built in tempdirs from the research corpus's field inventory
//! and edge-case catalog (#106) — the filesystem is exactly the I/O under
//! test, matching `canonical_path`'s posture.

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
        "the closed blocker leaves its dependent on the Frontier"
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
