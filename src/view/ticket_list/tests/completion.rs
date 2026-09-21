use super::*;

#[test]
fn an_overflowing_summary_can_be_scrolled_to_the_prompt_without_moving_the_ticket() {
    use ratatui::crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind,
    };
    let (mut model, _) = Model::new();
    let mut dependencies: Vec<_> = (1..=20)
        .map(|i| {
            let mut ticket = ticket(
                "alpha",
                &format!("{i:02}-done"),
                &format!("Settled decision {i}"),
                "research",
            );
            ticket.state = TicketState::Closed;
            ticket
        })
        .collect();
    let mut selected = ticket("alpha", "21-next", "Long summary", "task");
    selected.dependencies = dependencies
        .iter()
        .map(|ticket| Dependency::SameEffort(ticket.key.clone()))
        .collect();
    dependencies.push(selected);
    dependencies.push(ticket("alpha", "22-other", "Other ticket", "task"));
    model.tickets.merge_effort(
        web(),
        effort("alpha", "Map", "Destination", dependencies),
        chrono::DateTime::UNIX_EPOCH,
    );
    model.view_mode = ViewMode::TicketList;
    let event = |model: &mut Model, event| {
        crate::app::update::update(
            model,
            crate::app::msg::Msg::TerminalEvent(event),
            chrono::DateTime::UNIX_EPOCH,
        )
    };
    event(&mut model, Event::Resize(200, 24));
    let initial = buffer_text(&render(&model, 200, 24)).join("\n");
    assert!(initial.contains("more ↓"), "{initial}");
    assert!(!initial.contains("p  Long summary"));
    let selected = model.tickets.selected_ticket().cloned();
    for _ in 0..20 {
        assert!(
            event(
                &mut model,
                Event::Key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE))
            )
            .is_empty()
        );
    }
    let bottom = buffer_text(&render(&model, 200, 24)).join("\n");
    assert!(bottom.contains("p  Long summary"), "{bottom}");
    assert!(!bottom.contains("more ↓"));
    assert_eq!(model.tickets.selected_ticket(), selected.as_ref());
    event(
        &mut model,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 190,
            row: 10,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert!(
        buffer_text(&render(&model, 200, 24))
            .join("\n")
            .contains("more ↓")
    );
    event(
        &mut model,
        Event::Key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
    );
    assert!(buffer_text(&render(&model, 200, 24))[3].contains("22-other Other ticket"));
}

#[test]
fn shrinking_drops_summary_then_rail_and_keeps_a_state_column_and_titles() {
    let model = populated_model();
    let wide = buffer_text(&render(&model, 200, 24)).join("\n");
    assert!(wide.contains("waits on (↑)"));
    let medium = buffer_text(&render(&model, 140, 24)).join("\n");
    assert!(!medium.contains("waits on (↑)"));
    assert!(medium.contains("local · 1/5 decided"));
    let narrow = buffer_text(&render(&model, 70, 24)).join("\n");
    assert!(!narrow.contains("local · 1/5 decided"));
    assert!(narrow.contains("State"), "{narrow}");
    assert!(narrow.contains("Frontier"));
    assert!(narrow.contains("Read the RFC"), "{narrow}");
    let tiny = buffer_text(&render(&model, 40, 24)).join("\n");
    assert!(tiny.contains("State"));
    assert!(tiny.contains("Read the RFC"), "{tiny}");
    assert!(
        tiny.lines().any(|line| line.starts_with("* 03-blocked")),
        "Either must stay visible without a Type column:\n{tiny}"
    );
}

#[test]
fn tabs_mode_chips_and_effort_filter_are_visible_and_the_rail_follows_its_filter() {
    let mut model = populated_model();
    model.config.repos.push(crate::config::RepoConfig {
        slug: Some(RepoSlug::new("acme/web")),
        ..Default::default()
    });
    model.tickets.cycle_mode();
    model
        .tickets
        .step_effort(crate::app::list_cursor::Direction::Down);
    model
        .tickets
        .step_effort(crate::app::list_cursor::Direction::Down);
    let terminal = render(&model, 140, 12);
    let text = buffer_text(&terminal).join("\n");
    for expected in [
        "[All]",
        "acme/web",
        "[AFK]",
        "Map: ticket surface",
        "Either",
    ] {
        assert!(text.contains(expected), "missing {expected}:\n{text}");
    }
    let rows = buffer_text(&terminal);
    let y = rows
        .iter()
        .position(|row| row.starts_with("web · Map: ticket surface"))
        .expect("selected rail card must be visible");
    assert_eq!(
        terminal.backend().buffer()[(0, y as u16)].bg,
        DARK.selected_bg
    );
}

#[test]
fn summary_resolves_dependencies_and_open_dependents_from_loaded_efforts() {
    let (mut model, _) = Model::new();
    let mut done = ticket("alpha", "01-done", "Choose the contract", "grilling");
    done.state = TicketState::Closed;
    let mut selected = ticket("alpha", "02-build", "Build the queue", "task");
    selected.dependencies = vec![
        Dependency::SameEffort(done.key.clone()),
        Dependency::Unknown {
            raw: "missing.md".to_owned(),
        },
    ];
    let mut next = ticket("alpha", "03-use", "Use the queue", "research");
    next.dependencies = vec![Dependency::SameEffort(selected.key.clone())];
    model.tickets.merge_effort(
        web(),
        effort(
            "alpha",
            "Queue map",
            "Browse every decision",
            vec![done, selected, next],
        ),
        chrono::DateTime::UNIX_EPOCH,
    );
    model.view_mode = ViewMode::TicketList;
    model.tickets.move_down();
    let terminal = render(&model, 200, 35);
    let text = buffer_text(&terminal).join("\n");
    for expected in [
        "task · Either",
        "waits on (↑)",
        "Choose the contract",
        "missing.md — can't find or read",
        "blocks (↓)",
        "Use the queue",
        "p  Build the queue | /wayfinder",
    ] {
        assert!(text.contains(expected), "missing {expected}:\n{text}");
    }
    assert_eq!(
        fg_of(&terminal, "missing.md — can't find or read"),
        DARK.blocked
    );
    assert_eq!(fg_of(&terminal, "Choose the contract"), DARK.muted);
}
