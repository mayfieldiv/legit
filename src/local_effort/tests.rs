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
