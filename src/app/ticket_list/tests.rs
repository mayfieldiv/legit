//! Unit tests for the ticket queue: pooling Efforts, tier grouping, the
//! cursor, and the probe phases. Expected values come from spec §6.1–§6.3
//! (issue #112's resolution comment). Pure — Efforts are built in memory.

use super::{DiscoveryUnit, EffortEntry, QueueRow, QueueTier, TicketList};
use crate::{
    canonical_path::CanonicalPathBuf,
    config::RepoIdentity,
    repo_slug::RepoSlug,
    ticket::{
        Claim, Dependency, Effort, EffortKey, EffortRead, Ticket, TicketKey, TicketState,
        TicketType,
    },
};

// ── builders ─────────────────────────────────────────────────────────────────

fn repo(name: &str) -> RepoIdentity {
    RepoIdentity::Slug(RepoSlug::new(format!("acme/{name}")))
}

fn local_key(effort: &str, slug: &str) -> TicketKey {
    TicketKey::Local {
        path: CanonicalPathBuf::assume_canonical(format!("/w/{effort}/tickets/{slug}.md")),
    }
}

fn effort_key(effort: &str) -> EffortKey {
    EffortKey::Local {
        dir: CanonicalPathBuf::assume_canonical(format!("/w/{effort}")),
    }
}

struct TicketSpec {
    slug: &'static str,
    state: TicketState,
    claim: Option<Claim>,
    deps: Vec<Dependency>,
}

fn open(slug: &'static str) -> TicketSpec {
    TicketSpec {
        slug,
        state: TicketState::Open,
        claim: None,
        deps: Vec::new(),
    }
}

fn closed(slug: &'static str) -> TicketSpec {
    TicketSpec {
        state: TicketState::Closed,
        ..open(slug)
    }
}

fn claimed(slug: &'static str, by: &str) -> TicketSpec {
    TicketSpec {
        claim: Some(Claim::By(by.to_owned())),
        ..open(slug)
    }
}

fn after(slug: &'static str, effort: &str, target: &str) -> TicketSpec {
    TicketSpec {
        deps: vec![Dependency::SameEffort(local_key(effort, target))],
        ..open(slug)
    }
}

fn unknown_dep(slug: &'static str, raw: &str) -> TicketSpec {
    TicketSpec {
        deps: vec![Dependency::Unknown {
            raw: raw.to_owned(),
        }],
        ..open(slug)
    }
}

fn effort(name: &str, title: &str, tickets: Vec<TicketSpec>) -> Effort {
    Effort::new(
        effort_key(name),
        title.to_owned(),
        Some(format!("{title} destination")),
        tickets
            .into_iter()
            .map(|spec| Ticket {
                key: local_key(name, spec.slug),
                title: format!("Ticket {}", spec.slug),
                state: spec.state,
                claim: spec.claim,
                ty: TicketType("task".to_owned()),
                dependencies: spec.deps,
            })
            .collect(),
    )
    .unwrap()
}

fn ready(name: &str, title: &str, tickets: Vec<TicketSpec>) -> EffortRead {
    EffortRead::Ready(effort(name, title, tickets))
}

/// The queue as `── <tier>` headers and ticket display refs, in display order.
fn rows(list: &TicketList) -> Vec<String> {
    list.rows()
        .iter()
        .map(|row| match row {
            QueueRow::Header(tier) => format!("── {}", tier.label()),
            QueueRow::Ticket(key) => key.display_ref(),
        })
        .collect()
}

fn selected(list: &TicketList) -> Option<String> {
    list.selected_ticket().map(|key| key.display_ref())
}

fn rail_titles(list: &TicketList) -> Vec<String> {
    list.efforts().iter().map(EffortEntry::title).collect()
}

// ── tiers ────────────────────────────────────────────────────────────────────

#[test]
fn tickets_group_into_frontier_claimed_blocked_in_effort_then_ticket_order() {
    let mut list = TicketList::new();
    list.merge_effort(
        repo("web"),
        ready(
            "alpha",
            "Alpha",
            vec![
                after("01-blocked", "alpha", "02-free"),
                open("02-free"),
                claimed("03-taken", "mayfield"),
            ],
        ),
    );
    list.merge_effort(
        repo("web"),
        ready(
            "beta",
            "Beta",
            vec![open("01-first"), after("02-later", "beta", "01-first")],
        ),
    );

    assert_eq!(
        rows(&list),
        [
            "── Frontier",
            "02-free",
            "01-first",
            "── Claimed",
            "03-taken",
            "── Blocked",
            "01-blocked",
            "02-later",
        ]
    );
}

#[test]
fn a_claimed_ticket_that_is_also_blocked_sits_in_claimed() {
    let mut list = TicketList::new();
    let mut spec = claimed("01-taken", "mayfield");
    spec.deps = vec![Dependency::SameEffort(local_key("alpha", "02-open"))];
    list.merge_effort(
        repo("web"),
        ready("alpha", "Alpha", vec![spec, open("02-open")]),
    );

    assert_eq!(
        rows(&list),
        ["── Frontier", "02-open", "── Claimed", "01-taken"],
        "someone is already working on it — the claim is the more useful signal"
    );
}

#[test]
fn closed_tickets_are_hidden_from_the_queue_but_counted_as_decided() {
    let mut list = TicketList::new();
    list.merge_effort(
        repo("web"),
        ready(
            "alpha",
            "Alpha",
            vec![
                closed("01-done"),
                after("02-next", "alpha", "01-done"),
                closed("03-done"),
            ],
        ),
    );

    assert_eq!(rows(&list), ["── Frontier", "02-next"]);
    let counts = list.efforts()[0].counts();
    assert_eq!((counts.decided, counts.total, counts.frontier), (2, 3, 1));
}

#[test]
fn unknown_dependency_tickets_fold_into_blocked_and_sort_last() {
    let mut list = TicketList::new();
    list.merge_effort(
        repo("web"),
        ready(
            "alpha",
            "Alpha",
            vec![
                unknown_dep("01-mystery", "../gone/tickets/09-x.md"),
                after("02-waiting", "alpha", "03-open"),
                open("03-open"),
            ],
        ),
    );
    list.merge_effort(
        repo("web"),
        ready(
            "beta",
            "Beta",
            vec![after("01-waiting", "beta", "02-open"), open("02-open")],
        ),
    );

    assert_eq!(
        rows(&list),
        [
            "── Frontier",
            "03-open",
            "02-open",
            "── Blocked",
            "02-waiting",
            "01-waiting",
            "01-mystery",
        ],
        "unknown-dependency tickets have no tier of their own; they trail Blocked"
    );
}

#[test]
fn empty_tiers_emit_no_header() {
    let mut list = TicketList::new();
    list.merge_effort(repo("web"), ready("alpha", "Alpha", vec![open("01-a")]));

    assert_eq!(rows(&list), ["── Frontier", "01-a"]);
    assert_eq!(QueueTier::Frontier.label(), "Frontier");
    assert_eq!(QueueTier::Claimed.label(), "Claimed");
    assert_eq!(QueueTier::Blocked.label(), "Blocked");
}

// ── rail ─────────────────────────────────────────────────────────────────────

#[test]
fn efforts_sort_by_repo_then_title_regardless_of_arrival_order() {
    let mut list = TicketList::new();
    list.merge_effort(repo("web"), ready("zeta", "Zeta", vec![open("01-a")]));
    list.merge_effort(repo("api"), ready("mid", "Mid", vec![open("01-b")]));
    list.merge_effort(repo("web"), ready("alpha", "Alpha", vec![open("01-c")]));

    assert_eq!(rail_titles(&list), ["Mid", "Alpha", "Zeta"]);
    assert_eq!(
        rows(&list),
        ["── Frontier", "01-b", "01-c", "01-a"],
        "queue order within a tier is rail order"
    );
}

#[test]
fn a_degraded_effort_keeps_its_card_with_the_error_and_contributes_no_tickets() {
    let mut list = TicketList::new();
    list.merge_effort(
        repo("web"),
        EffortRead::Degraded {
            key: effort_key("broken"),
            title: "Broken".to_owned(),
            destination: Some("Somewhere".to_owned()),
            reason: "tickets/01-a.md: missing status".to_owned(),
        },
    );

    assert_eq!(rail_titles(&list), ["Broken"]);
    assert_eq!(
        list.efforts()[0].error(),
        Some("tickets/01-a.md: missing status")
    );
    assert!(rows(&list).is_empty());
    assert!(list.visible_is_empty());
}

#[test]
fn a_re_arriving_effort_replaces_its_pooled_read() {
    let mut list = TicketList::new();
    list.merge_effort(repo("web"), ready("alpha", "Alpha", vec![open("01-a")]));
    list.merge_effort(
        repo("web"),
        ready("alpha", "Alpha renamed", vec![open("01-a"), open("02-b")]),
    );

    assert_eq!(rail_titles(&list), ["Alpha renamed"]);
    assert_eq!(rows(&list), ["── Frontier", "01-a", "02-b"]);
}

// ── cursor ───────────────────────────────────────────────────────────────────

fn two_efforts() -> TicketList {
    let mut list = TicketList::new();
    list.merge_effort(
        repo("web"),
        ready(
            "alpha",
            "Alpha",
            vec![
                open("01-a"),
                claimed("02-b", "x"),
                after("03-c", "alpha", "01-a"),
            ],
        ),
    );
    list.merge_effort(repo("web"), ready("beta", "Beta", vec![open("01-d")]));
    list
}

#[test]
fn selection_follows_the_top_until_navigated_then_sticks_to_its_ticket() {
    let mut list = TicketList::new();
    assert_eq!(selected(&list), None);

    list.merge_effort(repo("web"), ready("mid", "Mid", vec![open("01-m")]));
    assert_eq!(selected(&list), Some("01-m".to_owned()));
    // An Effort sorting above the selection re-tops the still-default cursor…
    list.merge_effort(repo("web"), ready("alpha", "Alpha", vec![open("01-a")]));
    assert_eq!(selected(&list), Some("01-a".to_owned()));

    // …but once the user has moved, the cursor sticks to its ticket.
    list.move_down();
    assert_eq!(selected(&list), Some("01-m".to_owned()));
    list.merge_effort(
        repo("web"),
        ready("aardvark", "Aardvark", vec![open("01-z")]),
    );
    assert_eq!(selected(&list), Some("01-m".to_owned()));
}

#[test]
fn j_and_k_step_tickets_and_skip_tier_headers() {
    let mut list = two_efforts();
    assert_eq!(rows(&list).len(), 7, "3 headers + 4 tickets");
    assert_eq!(selected(&list), Some("01-a".to_owned()));

    list.move_down();
    assert_eq!(selected(&list), Some("01-d".to_owned()));
    list.move_down();
    assert_eq!(
        selected(&list),
        Some("02-b".to_owned()),
        "skips the Claimed header"
    );
    list.move_down();
    assert_eq!(selected(&list), Some("03-c".to_owned()));
    list.move_down();
    assert_eq!(
        selected(&list),
        Some("03-c".to_owned()),
        "clamps at the last ticket"
    );

    list.move_up();
    list.move_up();
    list.move_up();
    assert_eq!(selected(&list), Some("01-a".to_owned()));
    list.move_up();
    assert_eq!(
        selected(&list),
        Some("01-a".to_owned()),
        "clamps at the first ticket"
    );
}

#[test]
fn a_vanished_selection_snaps_to_the_top_ticket() {
    let mut list = two_efforts();
    list.move_down();
    list.move_down();
    assert_eq!(selected(&list), Some("02-b".to_owned()));

    list.merge_effort(repo("web"), ready("alpha", "Alpha", vec![closed("02-b")]));

    assert_eq!(selected(&list), Some("01-d".to_owned()));
}

#[test]
fn the_viewport_follows_the_cursor() {
    let mut list = two_efforts();
    list.resize(3);
    assert_eq!(list.scroll_offset(), 0);

    for _ in 0..3 {
        list.move_down();
    }
    let visible: Vec<&QueueRow> = list.visible_rows().map(|(row, _)| row).collect();
    assert_eq!(visible.len(), 3);
    assert!(
        list.visible_rows().any(|(row, selected)| {
            selected && *row == QueueRow::Ticket(local_key("alpha", "03-c"))
        }),
        "the selected ticket's row is inside the window"
    );
}

// ── probe phases ─────────────────────────────────────────────────────────────

#[test]
fn probe_phases_report_loading_until_every_unit_settles() {
    let mut list = TicketList::new();
    assert!(!list.is_loading());

    list.begin_discovery(DiscoveryUnit::Cwd);
    list.begin_discovery(DiscoveryUnit::LocalRepo {
        name: "acme/web".to_owned(),
        main_worktree_path: "/src/web".to_owned(),
    });
    assert!(list.is_loading());

    list.finish_discovery(&DiscoveryUnit::Cwd);
    assert!(list.is_loading(), "one unit still in flight");
    list.fail_discovery(
        &DiscoveryUnit::LocalRepo {
            name: "acme/web".to_owned(),
            main_worktree_path: "/src/web".to_owned(),
        },
        "main worktree /x does not exist".to_owned(),
    );
    assert!(!list.is_loading());
    assert_eq!(
        list.discovery_failures().collect::<Vec<_>>(),
        [("acme/web", "main worktree /x does not exist")]
    );
}
