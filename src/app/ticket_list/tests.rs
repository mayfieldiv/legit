//! Unit tests for the ticket queue: pooling Efforts, tier grouping, the
//! cursor, and the probe phases. Expected values come from spec §6.1–§6.3
//! (issue #112's resolution comment). Pure — Efforts are built in memory.

use super::{DiscoveryUnit, QueueRow, QueueTier, RailCard, RowMarker, TicketList, TicketRow};
use crate::{
    canonical_path::CanonicalPathBuf,
    config::RepoIdentity,
    repo_slug::RepoSlug,
    ticket::{
        Claim, Dependency, Effort, EffortKey, EffortRead, ExternalDependency, Ticket, TicketKey,
        TicketState, TicketType,
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

/// The queue as `── <tier>` headers and ticket display refs, in display
/// order. An unsized viewport shows every row.
fn rows(list: &TicketList) -> Vec<String> {
    list.visible_rows()
        .map(|(row, _)| match row {
            QueueRow::Header(tier) => format!("── {}", tier.label()),
            QueueRow::Ticket(row) => row.display_ref.clone(),
        })
        .collect()
}

/// The queue row for the Ticket shown as `display_ref`, which must be queued.
fn ticket_row<'a>(list: &'a TicketList, display_ref: &str) -> &'a TicketRow {
    list.visible_rows()
        .find_map(|(row, _)| match row {
            QueueRow::Ticket(row) if row.display_ref == display_ref => Some(row),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{display_ref} is not queued: {:?}", rows(list)))
}

fn selected(list: &TicketList) -> Option<String> {
    list.selected_ticket().map(|key| key.display_ref())
}

/// The rail as `repo · title` for Effort cards, `unit ✗ error` for failed
/// units, and `unit ⚠ caveat` for incomplete ones, in display order.
fn rail(list: &TicketList) -> Vec<String> {
    list.rail()
        .map(|card| match card {
            RailCard::Effort(card) => format!("{} · {}", card.repo, card.title),
            RailCard::Failure { unit, error } => format!("{} ✗ {error}", unit.label()),
            RailCard::Incomplete { unit, caveat } => format!("{} ⚠ {caveat}", unit.label()),
        })
        .collect()
}

fn rail_titles(list: &TicketList) -> Vec<String> {
    list.rail()
        .filter_map(|card| match card {
            RailCard::Effort(card) => Some(card.title.clone()),
            RailCard::Failure { .. } | RailCard::Incomplete { .. } => None,
        })
        .collect()
}

/// The one Effort card in the rail.
fn only_card(list: &TicketList) -> &super::EffortCard {
    let mut cards = list.rail().filter_map(|card| match card {
        RailCard::Effort(card) => Some(card),
        RailCard::Failure { .. } | RailCard::Incomplete { .. } => None,
    });
    let card = cards.next().expect("one effort card");
    assert!(cards.next().is_none(), "one effort card: {:?}", rail(list));
    card
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
        chrono::DateTime::UNIX_EPOCH,
    );
    list.merge_effort(
        repo("web"),
        ready(
            "beta",
            "Beta",
            vec![open("01-first"), after("02-later", "beta", "01-first")],
        ),
        chrono::DateTime::UNIX_EPOCH,
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
        chrono::DateTime::UNIX_EPOCH,
    );

    assert_eq!(
        rows(&list),
        ["── Frontier", "02-open", "── Claimed", "01-taken"],
        "someone is already working on it — the claim is the more useful signal"
    );
}

#[test]
fn a_claimed_ticket_with_an_unknown_dependency_is_still_flagged_in_blocked() {
    let mut list = TicketList::new();
    let mut spec = claimed("01-taken", "mayfield");
    spec.deps = vec![Dependency::Unknown {
        raw: "../gone/tickets/09-x.md".to_owned(),
    }];
    list.merge_effort(
        repo("web"),
        ready("alpha", "Alpha", vec![spec, open("02-open")]),
        chrono::DateTime::UNIX_EPOCH,
    );

    assert_eq!(
        rows(&list),
        ["── Frontier", "02-open", "── Blocked", "01-taken"],
        "an unknown dependency is a warning the claim must not hide"
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
        chrono::DateTime::UNIX_EPOCH,
    );

    assert_eq!(rows(&list), ["── Frontier", "02-next"]);
    let summary = only_card(&list).outcome.as_ref().unwrap();
    let counts = summary.counts;
    assert_eq!((counts.decided, counts.total, counts.frontier), (2, 3, 1));
    assert_eq!(summary.destination.as_deref(), Some("Alpha destination"));
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
        chrono::DateTime::UNIX_EPOCH,
    );
    list.merge_effort(
        repo("web"),
        ready(
            "beta",
            "Beta",
            vec![after("01-waiting", "beta", "02-open"), open("02-open")],
        ),
        chrono::DateTime::UNIX_EPOCH,
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
fn rows_carry_their_marker_and_pool_wide_block_counts() {
    let mut list = TicketList::new();
    list.merge_effort(
        repo("web"),
        ready(
            "alpha",
            "Alpha",
            vec![
                open("01-a"),
                after("02-b", "alpha", "01-a"),
                claimed("03-c", "mayfield"),
                unknown_dep("04-d", "gone.md"),
            ],
        ),
        chrono::DateTime::UNIX_EPOCH,
    );
    let mut external = open("01-x");
    external.deps = vec![Dependency::External(ExternalDependency {
        key: local_key("alpha", "01-a"),
        state: TicketState::Open,
        title: None,
    })];
    list.merge_effort(
        repo("web"),
        ready("beta", "Beta", vec![external]),
        chrono::DateTime::UNIX_EPOCH,
    );

    let summary = |display_ref: &str| {
        let row = ticket_row(&list, display_ref);
        (row.tier(), row.marker.clone(), row.upstream, row.downstream)
    };
    assert_eq!(
        summary("01-a"),
        (QueueTier::Frontier, None, 0, 2),
        "02-b in its own Effort and 01-x in Beta both wait on it"
    );
    assert_eq!(
        summary("02-b"),
        (
            QueueTier::Blocked,
            Some(RowMarker::After("01-a".to_owned())),
            1,
            0
        )
    );
    assert_eq!(
        summary("03-c"),
        (
            QueueTier::Claimed,
            Some(RowMarker::Claimed(Some("mayfield".to_owned()))),
            0,
            0
        )
    );
    assert_eq!(
        summary("04-d"),
        (
            QueueTier::Blocked,
            Some(RowMarker::UnknownDependency("gone.md".to_owned())),
            0,
            0
        )
    );
    assert_eq!(
        summary("01-x"),
        (
            QueueTier::Blocked,
            Some(RowMarker::After("01-a".to_owned())),
            1,
            0
        ),
        "an External Dependency counts upstream like a same-effort one"
    );
}

#[test]
fn block_counts_read_distinct_targets_not_declared_edges() {
    let mut list = TicketList::new();
    let mut twice = open("02-b");
    twice.deps = vec![
        Dependency::SameEffort(local_key("alpha", "01-a")),
        Dependency::SameEffort(local_key("alpha", "01-a")),
    ];
    list.merge_effort(
        repo("web"),
        ready("alpha", "Alpha", vec![open("01-a"), twice]),
        chrono::DateTime::UNIX_EPOCH,
    );

    assert_eq!(
        ticket_row(&list, "01-a").downstream,
        1,
        "`blocked-by: [1, 1]` is one dependent ticket"
    );
    assert_eq!(ticket_row(&list, "02-b").upstream, 1);
}

#[test]
fn empty_tiers_emit_no_header() {
    let mut list = TicketList::new();
    list.merge_effort(
        repo("web"),
        ready("alpha", "Alpha", vec![open("01-a")]),
        chrono::DateTime::UNIX_EPOCH,
    );

    assert_eq!(rows(&list), ["── Frontier", "01-a"]);
    assert_eq!(QueueTier::Frontier.label(), "Frontier");
    assert_eq!(QueueTier::Claimed.label(), "Claimed");
    assert_eq!(QueueTier::Blocked.label(), "Blocked");
}

// ── rail ─────────────────────────────────────────────────────────────────────

#[test]
fn efforts_sort_by_repo_then_title_regardless_of_arrival_order() {
    let mut list = TicketList::new();
    list.merge_effort(
        repo("web"),
        ready("zeta", "Zeta", vec![open("01-a")]),
        chrono::DateTime::UNIX_EPOCH,
    );
    list.merge_effort(
        repo("api"),
        ready("mid", "Mid", vec![open("01-b")]),
        chrono::DateTime::UNIX_EPOCH,
    );
    list.merge_effort(
        repo("web"),
        ready("alpha", "Alpha", vec![open("01-c")]),
        chrono::DateTime::UNIX_EPOCH,
    );

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
        chrono::DateTime::UNIX_EPOCH,
    );

    let card = only_card(&list);
    assert_eq!((card.repo.as_str(), card.title.as_str()), ("web", "Broken"));
    assert_eq!(
        card.outcome,
        Err("tickets/01-a.md: missing status".to_owned())
    );
    assert!(rows(&list).is_empty());
    assert!(list.visible_is_empty());
}

#[test]
fn a_re_arriving_effort_replaces_its_pooled_read() {
    let mut list = TicketList::new();
    list.merge_effort(
        repo("web"),
        ready("alpha", "Alpha", vec![open("01-a")]),
        chrono::DateTime::UNIX_EPOCH,
    );
    list.merge_effort(
        repo("web"),
        ready("alpha", "Alpha renamed", vec![open("01-a"), open("02-b")]),
        chrono::DateTime::UNIX_EPOCH,
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
        chrono::DateTime::UNIX_EPOCH,
    );
    list.merge_effort(
        repo("web"),
        ready("beta", "Beta", vec![open("01-d")]),
        chrono::DateTime::UNIX_EPOCH,
    );
    list
}

#[test]
fn selection_follows_the_top_until_navigated_then_sticks_to_its_ticket() {
    let mut list = TicketList::new();
    assert_eq!(selected(&list), None);

    list.merge_effort(
        repo("web"),
        ready("mid", "Mid", vec![open("01-m")]),
        chrono::DateTime::UNIX_EPOCH,
    );
    assert_eq!(selected(&list), Some("01-m".to_owned()));
    // An Effort sorting above the selection re-tops the still-default cursor…
    list.merge_effort(
        repo("web"),
        ready("alpha", "Alpha", vec![open("01-a")]),
        chrono::DateTime::UNIX_EPOCH,
    );
    assert_eq!(selected(&list), Some("01-a".to_owned()));

    // …but once the user has moved, the cursor sticks to its ticket.
    list.move_down();
    assert_eq!(selected(&list), Some("01-m".to_owned()));
    list.merge_effort(
        repo("web"),
        ready("aardvark", "Aardvark", vec![open("01-z")]),
        chrono::DateTime::UNIX_EPOCH,
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

    list.merge_effort(
        repo("web"),
        ready("alpha", "Alpha", vec![closed("02-b")]),
        chrono::DateTime::UNIX_EPOCH,
    );

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
    assert_eq!(list.visible_rows().count(), 3);
    assert!(
        list.visible_rows().any(|(row, selected)| {
            selected
                && matches!(row, QueueRow::Ticket(row) if row.key == local_key("alpha", "03-c"))
        }),
        "the selected ticket's row is inside the window"
    );
}

#[test]
fn rows_carry_what_they_show_resolved() {
    let mut list = TicketList::new();
    list.merge_effort(
        repo("web"),
        ready("alpha", "Alpha", vec![claimed("01-a", "mayfield")]),
        chrono::DateTime::UNIX_EPOCH,
    );

    let row = ticket_row(&list, "01-a");
    assert_eq!(row.key, local_key("alpha", "01-a"));
    assert_eq!(row.repo, "web", "the repo's short name");
    assert_eq!(row.title, "Ticket 01-a");
    assert_eq!(row.ty.0, "task");
}

#[test]
fn content_widths_measure_the_queued_tickets() {
    let mut list = TicketList::new();
    list.merge_effort(
        repo("web"),
        ready(
            "alpha",
            "Alpha",
            vec![open("01-a"), closed("02-a-long-decided-slug")],
        ),
        chrono::DateTime::UNIX_EPOCH,
    );

    let widths = list.content_widths();
    assert_eq!(
        widths.display_ref,
        "01-a".len(),
        "closed tickets aren't queued, so they don't size the column"
    );
    assert_eq!(widths.repo, "web".len());
    assert_eq!(widths.ty, "task".len());
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

    list.finish_discovery(DiscoveryUnit::Cwd, None, chrono::DateTime::UNIX_EPOCH);
    assert!(list.is_loading(), "one unit still in flight");
    list.fail_discovery(
        DiscoveryUnit::LocalRepo {
            name: "acme/web".to_owned(),
            main_worktree_path: "/src/web".to_owned(),
        },
        "main worktree /x does not exist".to_owned(),
    );
    assert!(!list.is_loading());
    assert_eq!(rail(&list), ["acme/web ✗ main worktree /x does not exist"]);
}

#[test]
fn failed_units_lead_the_rail_ahead_of_every_effort() {
    let mut list = TicketList::new();
    list.merge_effort(
        repo("api"),
        ready("alpha", "Alpha", vec![open("01-a")]),
        chrono::DateTime::UNIX_EPOCH,
    );
    list.fail_discovery(DiscoveryUnit::Cwd, "not a directory".to_owned());
    list.begin_discovery(DiscoveryUnit::LocalRepo {
        name: "acme/web".to_owned(),
        main_worktree_path: "/src/web".to_owned(),
    });

    assert_eq!(
        rail(&list),
        ["cwd ✗ not a directory", "api · Alpha"],
        "a unit still in flight has no card"
    );
}

#[test]
fn an_incomplete_unit_is_settled_but_leads_the_rail_with_its_caveat() {
    let mut list = TicketList::new();
    let unit = DiscoveryUnit::GitHubRepo {
        slug: RepoSlug::new("acme/api"),
    };
    list.begin_discovery(unit.clone());
    list.merge_effort(
        repo("api"),
        ready("alpha", "Alpha", vec![open("01-a")]),
        chrono::DateTime::UNIX_EPOCH,
    );

    list.finish_discovery(
        unit.clone(),
        Some("more than 10 open maps".to_owned()),
        chrono::DateTime::UNIX_EPOCH,
    );

    assert!(!list.is_loading());
    assert!(
        !list.needs_discovery(&unit),
        "settled: a re-read would see the same window"
    );
    assert_eq!(
        rail(&list),
        ["api ⚠ more than 10 open maps", "api · Alpha"],
        "the pooled Effort keeps its card; the caveat leads it"
    );
    assert_eq!(rows(&list), ["── Frontier", "01-a"]);
}
