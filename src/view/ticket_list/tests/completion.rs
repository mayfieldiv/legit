use super::*;

#[test]
fn local_ticket_updated_age_uses_file_modification_time_and_survives_refresh() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("map.md"), "# Local map\n").unwrap();
    std::fs::create_dir(dir.path().join("issues")).unwrap();
    let path = dir.path().join("issues/01-research.md");
    std::fs::write(&path, "# Research the protocol\n\nType: research\n").unwrap();
    std::fs::File::open(&path)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(2 * 86_400))
        .unwrap();
    let now = chrono::DateTime::UNIX_EPOCH + chrono::Duration::days(5);
    let (mut model, _) = Model::new();
    model.view_mode = ViewMode::TicketList;

    for (fetched_at, fetch_label) in [
        (now - chrono::Duration::minutes(2), "fetched 2m ago"),
        (now, "fetched just now"),
    ] {
        model.tickets.merge_effort(
            web(),
            crate::local_effort::read_effort_at(
                CanonicalPathBuf::canonicalize(dir.path()).unwrap(),
            ),
            fetched_at,
        );
        let mut terminal = Terminal::new(TestBackend::new(140, 24)).unwrap();
        terminal
            .draw(|frame| view::view(&model, frame, now))
            .unwrap();
        let rows = buffer_text(&terminal);
        assert!(rows[3].contains("Updated"), "{}", rows[3]);
        let ticket = rows
            .iter()
            .find(|row| row.contains("Research the protocol"))
            .unwrap();
        assert!(ticket.ends_with("3d     "), "{ticket}");
        assert!(rows.iter().any(|row| row.contains(fetch_label)));
    }
}

#[test]
fn a_local_map_with_no_blockers_keeps_its_tickets_visible() {
    for no_blockers in ["none", " None ", "NONE"] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("map.md"), "# Signing map\n").unwrap();
        std::fs::create_dir(dir.path().join("issues")).unwrap();
        std::fs::write(
            dir.path().join("issues/01-signing.md"),
            format!("# Investigate signing\n\nType: research\nBlocked by: {no_blockers}\n"),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("issues/02-publish.md"),
            "# Publish artifact\n\nType: task\nStatus: open\nBlocked by: 01\n",
        )
        .unwrap();
        let (mut model, _) = Model::new();
        model.tickets.merge_effort(
            web(),
            crate::local_effort::read_effort_at(
                CanonicalPathBuf::canonicalize(dir.path()).unwrap(),
            ),
            chrono::DateTime::UNIX_EPOCH,
        );
        model.view_mode = ViewMode::TicketList;

        let text = buffer_text(&render(&model, 140, 24)).join("\n");
        assert!(!text.contains("couldn't read"), "{no_blockers}:\n{text}");
        for expected in [
            "0/2 decided · 1 frontier",
            "Investigate signing",
            "Publish artifact",
            "after 01-signing",
        ] {
            assert!(text.contains(expected), "missing {expected}:\n{text}");
        }
    }
}

#[test]
fn an_unrecognized_local_dependency_blocks_only_its_ticket() {
    for dependent in [
        "# Dependent work\n\nType: task\nBlocked by: 01, mystery\n",
        "---\nstatus: open\ntype: task\nblocked-by: [1, mystery]\n---\n# Dependent work\n",
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("map.md"), "# Signing map\n").unwrap();
        std::fs::create_dir(dir.path().join("issues")).unwrap();
        std::fs::write(
            dir.path().join("issues/01-signing.md"),
            "# Investigate signing\n\nType: research\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("issues/02-publish.md"), dependent).unwrap();
        let (mut model, _) = Model::new();
        model.tickets.merge_effort(
            web(),
            crate::local_effort::read_effort_at(
                CanonicalPathBuf::canonicalize(dir.path()).unwrap(),
            ),
            chrono::DateTime::UNIX_EPOCH,
        );
        model.view_mode = ViewMode::TicketList;
        model.tickets.move_down();

        let text = buffer_text(&render(&model, 200, 30)).join("\n");
        assert!(!text.contains("couldn't read"), "{text}");
        for expected in [
            "0/2 decided · 1 frontier",
            "Investigate signing",
            "Dependent work",
            "dep? mystery",
            "↑1",
            "mystery — can't find or read",
        ] {
            assert!(text.contains(expected), "missing {expected}:\n{text}");
        }
    }
}

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
        tiny.lines()
            .any(|line| line.starts_with("  03-blocked Blocked  *")),
        "Either must stay visible without a Type column:\n{tiny}"
    );
}

#[test]
fn a_refreshing_either_ticket_keeps_both_glyphs_without_a_type_column() {
    let mut model = populated_model();
    let now = chrono::DateTime::UNIX_EPOCH + chrono::Duration::minutes(2);
    model.tickets.move_down();
    model.tickets.move_down();
    model.tickets.move_down();
    assert_eq!(
        model.tickets.selected_ticket().map(TicketKey::display_ref),
        Some("03-blocked".to_owned())
    );
    crate::app::update::update(
        &mut model,
        crate::app::msg::Msg::TerminalEvent(ratatui::crossterm::event::Event::Key(
            ratatui::crossterm::event::KeyEvent::new(
                ratatui::crossterm::event::KeyCode::Char('r'),
                ratatui::crossterm::event::KeyModifiers::NONE,
            ),
        )),
        now,
    );
    let mut terminal = Terminal::new(TestBackend::new(40, 24)).unwrap();
    terminal
        .draw(|frame| view::view(&model, frame, now))
        .unwrap();
    let rows = buffer_text(&terminal);
    let row = rows
        .iter()
        .find(|row| row.contains("03-blocked"))
        .unwrap_or_else(|| panic!("{}", rows.join("\n")));
    assert!(
        row.starts_with("↻ 03-blocked Blocked  *"),
        "refresh and Either must both show: {row:?}"
    );
    assert_eq!(fg_of(&terminal, "*…"), DARK.mode_either);
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
