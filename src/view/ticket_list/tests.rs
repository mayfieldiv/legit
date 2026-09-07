//! Snapshot tests for the ticket surface: the rail cards, the tiered queue,
//! and the placeholders. Text layout is asserted row by row; the few colours
//! that carry meaning (tier headers, Mode on the Type cell) are asserted on
//! their cells. Expected shapes come from spec §6.1–§6.2.

use ratatui::{Terminal, backend::TestBackend, style::Color};

use crate::{
    app::{
        model::{Model, ViewMode},
        ticket_list::DiscoveryUnit,
    },
    canonical_path::CanonicalPathBuf,
    config::RepoIdentity,
    palette::DARK,
    repo_slug::RepoSlug,
    ticket::{
        Claim, Dependency, Effort, EffortKey, EffortRead, Ticket, TicketKey, TicketState,
        TicketType,
    },
    view,
};

fn render(model: &Model, width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| view::view(model, frame, chrono::DateTime::UNIX_EPOCH))
        .expect("draw");
    terminal
}

fn buffer_text(terminal: &Terminal<TestBackend>) -> Vec<String> {
    let buf = terminal.backend().buffer();
    let area = *buf.area();
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buf[(x, y)].symbol().to_owned())
                .collect::<String>()
        })
        .collect()
}

/// The foreground colour of the first cell of `needle` on the row holding it.
fn fg_of(terminal: &Terminal<TestBackend>, needle: &str) -> Color {
    let rows = buffer_text(terminal);
    let (y, row) = rows
        .iter()
        .enumerate()
        .find(|(_, row)| row.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} not rendered:\n{}", rows.join("\n")));
    let start = row.find(needle).unwrap();
    let column = row[..start].chars().count();
    terminal.backend().buffer()[(column as u16, y as u16)].fg
}

fn local_key(effort: &str, slug: &str) -> TicketKey {
    TicketKey::Local {
        path: CanonicalPathBuf::assume_canonical(format!("/w/{effort}/tickets/{slug}.md")),
    }
}

fn ticket(effort: &str, slug: &str, title: &str, ty: &str) -> Ticket {
    Ticket {
        key: local_key(effort, slug),
        title: title.to_owned(),
        state: TicketState::Open,
        claim: None,
        ty: TicketType(ty.to_owned()),
        dependencies: Vec::new(),
    }
}

fn effort(name: &str, title: &str, destination: &str, tickets: Vec<Ticket>) -> EffortRead {
    EffortRead::Ready(
        Effort::new(
            EffortKey::Local {
                dir: CanonicalPathBuf::assume_canonical(format!("/w/{name}")),
            },
            title.to_owned(),
            Some(destination.to_owned()),
            tickets,
        )
        .unwrap(),
    )
}

fn web() -> RepoIdentity {
    RepoIdentity::Slug(RepoSlug::new("acme/web"))
}

/// The prototype's edge cases in one Effort: a Frontier ticket, a claimed
/// one, a blocked one (waiting on the Frontier ticket), a closed one, and an
/// Unknown-Dependency one — plus a second Effort with a research ticket.
fn populated_model() -> Model {
    let (mut model, _) = Model::new();
    let mut claimed = ticket("alpha", "02-claimed", "Prototype the rail", "prototype");
    claimed.claim = Some(Claim::By("mayfield".to_owned()));
    let mut blocked = ticket("alpha", "03-blocked", "Wire the queue", "task");
    blocked.dependencies = vec![Dependency::SameEffort(local_key("alpha", "01-free"))];
    let mut closed = ticket("alpha", "04-done", "Decide the palette", "grilling");
    closed.state = TicketState::Closed;
    let mut mystery = ticket("alpha", "05-mystery", "Ship it", "task");
    mystery.dependencies = vec![Dependency::Unknown {
        raw: "../gone/tickets/09-x.md".to_owned(),
    }];
    model.tickets.merge_effort(
        web(),
        effort(
            "alpha",
            "Map: ticket surface",
            "A queue toggled from the PR view",
            vec![
                ticket("alpha", "01-free", "Name the destination", "grilling"),
                claimed,
                blocked,
                closed,
                mystery,
            ],
        ),
    );
    model.tickets.merge_effort(
        RepoIdentity::Path(CanonicalPathBuf::assume_canonical("/src/notes")),
        effort(
            "beta",
            "Map: docs",
            "Docs done",
            vec![ticket("beta", "01-read", "Read the RFC", "research")],
        ),
    );
    model.view_mode = ViewMode::TicketList;
    model.terminal_width = 140;
    model.terminal_height = 24;
    model.sync_viewport();
    model
}

#[test]
fn the_ticket_surface_renders_the_rail_and_the_tiered_queue() {
    let model = populated_model();

    let terminal = render(&model, 140, 24);

    // Rail order is repo (`acme/web` before `notes`) then Map title; the queue
    // groups by tier, then rail order, then each Effort's own order. Closed
    // `04-done` is hidden but counted (`1/5 decided`). `01-free` blocks
    // `03-blocked`, hence `↓1` on one and `↑1 ⟨after 01-free⟩` on the other;
    // the Unknown-Dependency ticket trails Blocked with its raw ref.
    let mut expected = vec![
        "legit — Tickets — 2 efforts · 2 frontier                                                                                                    ",
        "All efforts                           │  Ticket     Repo           Type      Title                                           Block   Age    ",
        "                                      │  ── Frontier                                                                                        ",
        "web · Map: ticket surface             │  01-free    web            grilling  Name the destination                            ↓1             ",
        "local · 1/5 decided · 1 frontier      │  01-read    notes          research  Read the RFC                                                   ",
        "A queue toggled from the PR view      │  ── Claimed                                                                                         ",
        "                                      │  02-claimed web            prototype Prototype the rail ⟨claimed mayfield⟩                          ",
        "notes · Map: docs                     │  ── Blocked                                                                                         ",
        "local · 0/1 decided · 1 frontier      │  03-blocked web            task      Wire the queue ⟨after 01-free⟩                  ↑1             ",
        "Docs done                             │  05-mystery web            task      Ship it ⟨dep? ../gone/tickets/09-x.md⟩                         ",
    ];
    let blank = "                                      │                                                                                                     ";
    expected.extend(std::iter::repeat_n(blank, 13));
    expected.push(
        "j/k nav  t PRs  q quit                                                                                               0 in-flight · 0 waiting",
    );
    assert_eq!(buffer_text(&terminal), expected);
}

#[test]
fn tier_headers_and_type_cells_carry_their_semantic_colours() {
    let model = populated_model();

    let terminal = render(&model, 140, 24);

    assert_eq!(fg_of(&terminal, "── Frontier"), DARK.frontier);
    assert_eq!(fg_of(&terminal, "── Claimed"), DARK.claimed);
    assert_eq!(fg_of(&terminal, "── Blocked"), DARK.blocked);
    assert_eq!(fg_of(&terminal, "research"), DARK.mode_afk);
    assert_eq!(fg_of(&terminal, "prototype"), DARK.mode_hitl);
    assert_eq!(fg_of(&terminal, "task "), DARK.mode_either);
    assert_eq!(fg_of(&terminal, "↑1"), DARK.blocked);
    assert_eq!(fg_of(&terminal, "↓1"), DARK.blocks);
    assert_eq!(fg_of(&terminal, "⟨dep?"), DARK.blocked);
    assert_eq!(fg_of(&terminal, "⟨claimed"), DARK.claimed);
}

#[test]
fn the_selected_ticket_row_is_filled_and_its_title_brightened() {
    let mut model = populated_model();
    model.tickets.move_down();
    assert_eq!(
        model.tickets.selected_ticket().map(TicketKey::display_ref),
        Some("01-read".to_owned())
    );

    let terminal = render(&model, 140, 24);

    let rows = buffer_text(&terminal);
    let buf = terminal.backend().buffer();
    let row_of = |needle: &str| rows.iter().position(|row| row.contains(needle)).unwrap() as u16;
    let selected_y = row_of("01-read");
    // Column 40 is the queue's left pad — plain filler, so a fill there is
    // the row band, not a cell's own style.
    assert_eq!(
        buf[(40, selected_y)].bg,
        DARK.selected_bg,
        "the row is filled"
    );
    let title_x = rows[selected_y as usize].find("Read the RFC").unwrap() as u16;
    assert_eq!(buf[(title_x, selected_y)].fg, DARK.selected_fg);
    let other_y = row_of("01-free");
    assert_ne!(buf[(40, other_y)].bg, DARK.selected_bg);
}

#[test]
fn a_degraded_effort_and_a_failed_probe_each_get_a_card_with_the_error() {
    let (mut model, _) = Model::new();
    model.tickets.merge_effort(
        web(),
        EffortRead::Degraded {
            key: EffortKey::Local {
                dir: CanonicalPathBuf::assume_canonical("/w/broken"),
            },
            title: "Map: broken".to_owned(),
            destination: Some("Somewhere".to_owned()),
            reason: "tickets/01-a.md: missing status".to_owned(),
        },
    );
    model.tickets.fail_discovery(
        &DiscoveryUnit::LocalRepo {
            name: "immybot".to_owned(),
            main_worktree_path: "/src/immybot".to_owned(),
        },
        "main worktree /src/immybot does not exist".to_owned(),
    );
    model.view_mode = ViewMode::TicketList;

    let terminal = render(&model, 100, 12);

    assert_eq!(
        buffer_text(&terminal),
        vec![
            "legit — Tickets — 1 effort · 0 frontier                                                             ",
            "All efforts                           │  Ticket Repo           Type Title            Block   Age    ",
            "                                      │                       No open tickets                       ",
            "web · Map: broken                     │                                                             ",
            "local · couldn't read                 │                                                             ",
            "tickets/01-a.md: missing status       │                                                             ",
            "                                      │                                                             ",
            "immybot · couldn't probe              │                                                             ",
            "main worktree /src/immybot does not e…│                                                             ",
            "                                      │                                                             ",
            "                                      │                                                             ",
            "j/k nav  t PRs  q quit                                                       0 in-flight · 0 waiting",
        ]
    );
    assert_eq!(fg_of(&terminal, "couldn't read"), DARK.error);
    assert_eq!(fg_of(&terminal, "couldn't probe"), DARK.error);
}

#[test]
fn an_empty_surface_says_loading_while_a_probe_is_in_flight_then_no_efforts() {
    let (mut model, _) = Model::new();
    model.view_mode = ViewMode::TicketList;
    model.tickets.begin_discovery(DiscoveryUnit::Cwd);

    let terminal = render(&model, 60, 5);
    assert_eq!(
        buffer_text(&terminal),
        vec![
            "legit — Tickets — 0 efforts · 0 frontier                    ",
            "                      Loading efforts…                      ",
            "                                                            ",
            "                                                            ",
            "j/k nav  t PRs  q quit               0 in-flight · 0 waiting",
        ]
    );

    model.tickets.finish_discovery(&DiscoveryUnit::Cwd);
    let terminal = render(&model, 60, 5);
    assert_eq!(
        buffer_text(&terminal)[1],
        "                      No efforts found                      "
    );
}

#[test]
fn long_refs_are_middle_truncated_at_fourteen_columns() {
    let (mut model, _) = Model::new();
    model.tickets.merge_effort(
        web(),
        effort(
            "alpha",
            "Map",
            "D",
            vec![ticket(
                "alpha",
                "01-a-very-long-ticket-slug-indeed",
                "Long",
                "task",
            )],
        ),
    );
    model.view_mode = ViewMode::TicketList;

    let terminal = render(&model, 120, 6);

    let row = &buffer_text(&terminal)[3];
    assert!(
        row.contains("│  01-a-ve…indeed web"),
        "capped at 14 with a middle ellipsis: {row:?}"
    );
}
