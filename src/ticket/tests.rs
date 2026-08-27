//! Unit tests for the wayfinder ticket domain model. Expected values come
//! from the spec (issue #112's resolution comment, §1) and CONTEXT.md's
//! `### Wayfinder tickets` glossary. Pure and synchronous — no tokio.

use super::{
    Claim, Dependency, Effort, EffortKey, EffortSource, ExternalDependency, Mode, Ticket,
    TicketKey, TicketState, TicketType,
};

fn ty(s: &str) -> TicketType {
    TicketType(s.to_owned())
}

// ── builders ─────────────────────────────────────────────────────────────────

const SLUG: &str = "mayfieldiv/legit";

fn key(number: u64) -> TicketKey {
    TicketKey::GitHub {
        repo_slug: SLUG.to_owned(),
        number,
    }
}

fn ticket(number: u64, state: TicketState) -> Ticket {
    Ticket {
        key: key(number),
        title: format!("Ticket {number}"),
        state,
        claim: None,
        ty: ty("task"),
        dependencies: Vec::new(),
    }
}

fn open_ticket(number: u64) -> Ticket {
    ticket(number, TicketState::Open)
}

fn closed_ticket(number: u64) -> Ticket {
    ticket(number, TicketState::Closed)
}

fn dep_on(number: u64) -> Dependency {
    Dependency::SameEffort(key(number))
}

fn effort(tickets: Vec<Ticket>) -> Effort {
    Effort {
        key: EffortKey::GitHub {
            repo_slug: SLUG.to_owned(),
            map_number: 100,
        },
        title: "Map: test effort".to_owned(),
        destination: Some("A test destination".to_owned()),
        tickets,
    }
}

// ── Mode from Type ───────────────────────────────────────────────────────────

#[test]
fn research_is_afk() {
    assert_eq!(ty("research").mode(), Mode::Afk);
}

#[test]
fn prototype_and_grilling_are_hitl() {
    assert_eq!(ty("prototype").mode(), Mode::Hitl);
    assert_eq!(ty("grilling").mode(), Mode::Hitl);
}

#[test]
fn task_is_either() {
    assert_eq!(ty("task").mode(), Mode::Either);
}

#[test]
fn unknown_types_are_either() {
    assert_eq!(ty("spike").mode(), Mode::Either);
    assert_eq!(ty("").mode(), Mode::Either);
    // Types match exactly; a cased variant is an unknown Type, shown verbatim.
    assert_eq!(ty("Research").mode(), Mode::Either);
}

// ── EffortSource ─────────────────────────────────────────────────────────────

#[test]
fn effort_source_follows_the_key_variant() {
    let github = effort(Vec::new());
    assert_eq!(github.source(), EffortSource::GitHub);

    let local = Effort {
        key: EffortKey::Local {
            dir: "/home/mayfield/dev/legit/docs/wayfinder/ticket-surface".into(),
        },
        ..effort(Vec::new())
    };
    assert_eq!(local.source(), EffortSource::Local);
}

// ── blocked-ness ─────────────────────────────────────────────────────────────

#[test]
fn no_dependencies_is_not_blocked() {
    let e = effort(vec![open_ticket(1)]);
    assert!(!e.is_blocked(&e.tickets[0]));
}

#[test]
fn dependency_on_closed_same_effort_ticket_is_not_blocked() {
    let mut t = open_ticket(2);
    t.dependencies.push(dep_on(1));
    let e = effort(vec![closed_ticket(1), t]);
    assert!(!e.is_blocked(&e.tickets[1]));
}

#[test]
fn dependency_on_open_same_effort_ticket_is_blocked() {
    let mut t = open_ticket(2);
    t.dependencies.push(dep_on(1));
    let e = effort(vec![open_ticket(1), t]);
    assert!(e.is_blocked(&e.tickets[1]));
}

#[test]
fn one_open_dependency_among_closed_ones_still_blocks() {
    let mut t = open_ticket(3);
    t.dependencies.push(dep_on(1));
    t.dependencies.push(dep_on(2));
    let e = effort(vec![closed_ticket(1), open_ticket(2), t]);
    assert!(e.is_blocked(&e.tickets[2]));
}

#[test]
fn same_effort_dependency_target_missing_counts_as_unknown_and_blocks() {
    let mut t = open_ticket(2);
    t.dependencies.push(dep_on(99));
    let e = effort(vec![t]);
    assert!(e.is_blocked(&e.tickets[0]));
}

#[test]
fn open_external_dependency_blocks_closed_does_not() {
    let external = |state| {
        Dependency::External(ExternalDependency {
            key: TicketKey::GitHub {
                repo_slug: "other/repo".to_owned(),
                number: 7,
            },
            state,
            title: Some("External ticket".to_owned()),
        })
    };

    let mut blocked = open_ticket(1);
    blocked.dependencies.push(external(TicketState::Open));
    let mut free = open_ticket(2);
    free.dependencies.push(external(TicketState::Closed));
    let e = effort(vec![blocked, free]);
    assert!(e.is_blocked(&e.tickets[0]));
    assert!(!e.is_blocked(&e.tickets[1]));
}

// ── Frontier ─────────────────────────────────────────────────────────────────

#[test]
fn open_unclaimed_unblocked_ticket_is_on_the_frontier() {
    let e = effort(vec![open_ticket(1)]);
    assert!(e.is_on_frontier(&e.tickets[0]));
}

#[test]
fn claimed_tickets_are_off_the_frontier() {
    let mut named = open_ticket(1);
    named.claim = Some(Claim::By("mayfieldiv".to_owned()));
    // The newer local dialect claims without a name; still off the Frontier.
    let mut anonymous = open_ticket(2);
    anonymous.claim = Some(Claim::Anonymous);
    let e = effort(vec![named, anonymous]);
    assert!(!e.is_on_frontier(&e.tickets[0]));
    assert!(!e.is_on_frontier(&e.tickets[1]));
}

#[test]
fn closed_tickets_are_off_the_frontier() {
    let e = effort(vec![closed_ticket(1)]);
    assert!(!e.is_on_frontier(&e.tickets[0]));
}

#[test]
fn blocked_tickets_are_off_the_frontier() {
    let mut blocked = open_ticket(2);
    blocked.dependencies.push(dep_on(1));
    let mut unknown = open_ticket(3);
    unknown.dependencies.push(Dependency::Unknown {
        raw: "#999".to_owned(),
    });
    let e = effort(vec![open_ticket(1), blocked, unknown]);
    assert!(!e.is_on_frontier(&e.tickets[1]));
    assert!(!e.is_on_frontier(&e.tickets[2]));
}

#[test]
fn frontier_lists_only_frontier_tickets_in_effort_order() {
    let mut claimed = open_ticket(2);
    claimed.claim = Some(Claim::By("mayfieldiv".to_owned()));
    let mut blocked = open_ticket(4);
    blocked.dependencies.push(dep_on(2));
    let mut unblocked = open_ticket(5);
    unblocked.dependencies.push(dep_on(3));
    let e = effort(vec![
        open_ticket(1),
        claimed,
        closed_ticket(3),
        blocked,
        unblocked,
    ]);
    let frontier: Vec<&TicketKey> = e.frontier().map(|t| &t.key).collect();
    assert_eq!(frontier, vec![&key(1), &key(5)]);
}

// ── Blocks (reverse read) ────────────────────────────────────────────────────

#[test]
fn blocks_lists_open_tickets_that_depend_on_the_given_one() {
    let mut dependent_a = open_ticket(2);
    dependent_a.dependencies.push(dep_on(1));
    let mut dependent_b = open_ticket(3);
    dependent_b.dependencies.push(dep_on(1));
    dependent_b.dependencies.push(dep_on(2));
    let e = effort(vec![open_ticket(1), dependent_a, dependent_b]);
    let blocks: Vec<&TicketKey> = e.blocks(&key(1)).iter().map(|t| &t.key).collect();
    assert_eq!(blocks, vec![&key(2), &key(3)]);
}

#[test]
fn blocks_excludes_closed_dependents() {
    let mut resolved = closed_ticket(2);
    resolved.dependencies.push(dep_on(1));
    let e = effort(vec![open_ticket(1), resolved]);
    assert!(e.blocks(&key(1)).is_empty());
}

#[test]
fn blocks_is_empty_without_dependents() {
    let e = effort(vec![open_ticket(1), open_ticket(2)]);
    assert!(e.blocks(&key(1)).is_empty());
}

#[test]
fn unknown_dependency_always_blocks() {
    let mut t = open_ticket(1);
    t.dependencies.push(Dependency::Unknown {
        raw: "../../other/tickets/03-gone.md".to_owned(),
    });
    let e = effort(vec![t]);
    assert!(e.is_blocked(&e.tickets[0]));
}
