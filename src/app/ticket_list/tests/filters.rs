use super::*;
use crate::app::list_cursor::Direction;

#[test]
fn completed_efforts_are_hidden_from_the_rail_and_effort_filter() {
    let mut list = TicketList::new();
    list.merge_effort(
        repo("web"),
        ready("done", "Completed effort", vec![closed("01-done")]),
        chrono::DateTime::UNIX_EPOCH,
    );

    assert!(rail_titles(&list).is_empty());
    list.step_effort(Direction::Down);
    assert!(list.all_efforts_selected());
}

#[test]
fn closing_the_selected_effort_returns_to_all_and_refresh_can_reopen_it() {
    let mut list = two_efforts();
    list.step_effort(Direction::Down);
    list.merge_effort(
        repo("web"),
        ready("alpha", "Alpha", vec![closed("01-a")]),
        chrono::DateTime::UNIX_EPOCH,
    );

    assert!(list.all_efforts_selected());
    assert_eq!(rail_titles(&list), ["Beta"]);
    assert_eq!(selected(&list).as_deref(), Some("01-d"));
    assert_eq!(
        dispatched(&mut list, RefreshScope::View),
        ["/w/alpha", "/w/beta"]
    );

    list.local_effort_read(
        CanonicalPathBuf::assume_canonical("/w/alpha"),
        repo("web"),
        Ok(ready("alpha", "Alpha", vec![open("01-a")])),
        chrono::DateTime::UNIX_EPOCH,
    );
    assert_eq!(rail_titles(&list), ["Alpha", "Beta"]);
}

#[test]
fn empty_maps_and_efforts_with_claimed_blocked_or_unreadable_tickets_stay_visible() {
    let mut list = TicketList::new();
    for (name, title, tickets) in [
        ("empty", "Empty map", vec![]),
        ("claimed", "Claimed work", vec![claimed("01-work", "alice")]),
        (
            "blocked",
            "Blocked work",
            vec![unknown_dep("01-work", "missing")],
        ),
    ] {
        list.merge_effort(
            repo("web"),
            ready(name, title, tickets),
            chrono::DateTime::UNIX_EPOCH,
        );
    }
    list.merge_effort(
        repo("web"),
        EffortRead::Degraded {
            key: effort_key("unreadable"),
            title: "Unreadable map".to_owned(),
            destination: None,
            reason: "cannot parse a ticket".to_owned(),
        },
        chrono::DateTime::UNIX_EPOCH,
    );

    assert_eq!(
        rail_titles(&list),
        [
            "Blocked work",
            "Claimed work",
            "Empty map",
            "Unreadable map"
        ]
    );
    for title in [
        "Blocked work",
        "Claimed work",
        "Empty map",
        "Unreadable map",
    ] {
        list.step_effort(Direction::Down);
        assert_eq!(list.effort_filter_label(), title);
    }
}

#[test]
fn repo_scope_resets_effort_filter_but_blocks_stay_pool_wide() {
    let mut list = two_efforts();
    let mut dependent = open("01-api");
    dependent
        .deps
        .push(Dependency::External(ExternalDependency {
            key: local_key("alpha", "01-a"),
            state: TicketState::Open,
            title: Some("Ticket 01-a".to_owned()),
        }));
    list.merge_effort(
        repo("api"),
        ready("api", "API", vec![dependent]),
        chrono::DateTime::UNIX_EPOCH,
    );
    list.step_effort(Direction::Down);
    list.set_repo_scope(Some(super::super::RepoScope {
        repo: RepoSlug::new("acme/web"),
        discoveries: vec![],
    }));
    assert_eq!(rail_titles(&list), ["Alpha", "Beta"]);
    assert!(rows(&list).contains(&"01-d".to_owned()));
    assert!(!rows(&list).contains(&"01-api".to_owned()));
    assert_eq!(ticket_row(&list, "01-a").downstream, 2);
    list.step_effort(Direction::Down);
    assert_eq!(dispatched(&mut list, RefreshScope::View), ["/w/alpha"]);
}

#[test]
fn mode_filter_cycles_and_either_types_remain_in_both_views() {
    let mut list = TicketList::new();
    let tickets = ["research", "prototype", "grilling", "task", "custom"]
        .into_iter()
        .enumerate()
        .map(|(i, ty)| Ticket {
            key: local_key("alpha", &format!("{i}")),
            title: ty.to_owned(),
            state: TicketState::Open,
            claim: None,
            ty: TicketType(ty.to_owned()),
            dependencies: vec![],
        })
        .collect();
    list.merge_effort(
        repo("web"),
        EffortRead::Ready(
            Effort::new(effort_key("alpha"), "Alpha".to_owned(), None, tickets).unwrap(),
        ),
        chrono::DateTime::UNIX_EPOCH,
    );
    list.cycle_mode();
    assert_eq!(rows(&list), ["── Frontier", "0", "3", "4"]);
    list.move_down();
    list.cycle_mode();
    assert_eq!(rows(&list), ["── Frontier", "1", "2", "3", "4"]);
    assert_eq!(selected(&list).as_deref(), Some("3"));
    list.cycle_mode();
    assert_eq!(rows(&list), ["── Frontier", "0", "1", "2", "3", "4"]);
}

#[test]
fn effort_filter_steps_in_rail_order_and_keeps_the_selected_effort_on_arrival() {
    let mut list = two_efforts();
    list.step_effort(Direction::Down);
    assert_eq!(
        rows(&list),
        [
            "── Frontier",
            "01-a",
            "── Claimed",
            "02-b",
            "── Blocked",
            "03-c"
        ]
    );
    list.step_effort(Direction::Down);
    assert_eq!(rows(&list), ["── Frontier", "01-d"]);
    assert_eq!(selected(&list).as_deref(), Some("01-d"));

    list.merge_effort(
        repo("web"),
        ready("aardvark", "Aardvark", vec![open("01-new")]),
        chrono::DateTime::UNIX_EPOCH,
    );
    assert_eq!(rows(&list), ["── Frontier", "01-d"]);
    list.step_effort(Direction::Up);
    assert_eq!(selected(&list).as_deref(), Some("01-a"));
    list.step_effort(Direction::Up);
    assert_eq!(selected(&list).as_deref(), Some("01-new"));
    list.step_effort(Direction::Up);
    assert_eq!(rail_titles(&list), ["Aardvark", "Alpha", "Beta"]);
    assert!(rows(&list).contains(&"01-d".to_owned()));
}
