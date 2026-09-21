use super::*;
use crate::app::list_cursor::Direction;

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
