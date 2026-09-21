//! Snapshot tests for the ticket surface: the rail cards, the tiered queue,
//! and the placeholders. Text layout is asserted row by row; the few colours
//! that carry meaning (tier headers, Mode on the Type cell) are asserted on
//! their cells. Expected shapes come from spec §6.1–§6.2.

use ratatui::{Terminal, backend::TestBackend, style::Color};

use super::super::row::render_cells;
use crate::{
    app::{
        model::{Model, ViewMode},
        ticket_list::{DiscoveryUnit, RowMarker},
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

    // Rail order is the repo as shown (`notes` before `web` — the short name,
    // not `acme/web`) then Map title; the queue groups by tier, then rail
    // order, then each Effort's own order. Closed `04-done` is hidden but
    // counted (`1/5 decided`). `01-free` blocks `03-blocked`, hence `↓1` on
    // one and `↑1 ⟨after 01-free⟩` on the other; the Unknown-Dependency
    // ticket trails Blocked with its raw ref.
    let mut expected = vec![
        "legit — Tickets — 2 efforts · 2 frontier                                                                                                    ",
        "All efforts                           │  Ticket     Repo  Type      Title                                                    Block   Age    ",
        "                                      │  ── Frontier                                                                                        ",
        "notes · Map: docs                     │  01-read    notes research  Read the RFC                                                            ",
        "local · 0/1 decided · 1 frontier      │  01-free    web   grilling  Name the destination                                     ↓1             ",
        "Docs done                             │  ── Claimed                                                                                         ",
        "                                      │  02-claimed web   prototype Prototype the rail ⟨claimed mayfield⟩                                   ",
        "web · Map: ticket surface             │  ── Blocked                                                                                         ",
        "local · 1/5 decided · 1 frontier      │  03-blocked web   task      Wire the queue ⟨after 01-free⟩                           ↑1             ",
        "A queue toggled from the PR view      │  05-mystery web   task      Ship it ⟨dep? ../gone/tickets/09-x.md⟩                                  ",
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
        Some("01-free".to_owned())
    );

    let terminal = render(&model, 140, 24);

    let rows = buffer_text(&terminal);
    let buf = terminal.backend().buffer();
    let row_of = |needle: &str| rows.iter().position(|row| row.contains(needle)).unwrap() as u16;
    let selected_y = row_of("01-free");
    // Column 40 is the queue's left pad — plain filler, so a fill there is
    // the row band, not a cell's own style.
    assert_eq!(
        buf[(40, selected_y)].bg,
        DARK.selected_bg,
        "the row is filled"
    );
    let title_x = rows[selected_y as usize]
        .find("Name the destination")
        .unwrap() as u16;
    assert_eq!(buf[(title_x, selected_y)].fg, DARK.selected_fg);
    let other_y = row_of("01-read");
    assert_ne!(buf[(40, other_y)].bg, DARK.selected_bg);
}

#[test]
fn a_failed_probe_leads_the_rail_and_a_degraded_effort_keeps_its_card_with_the_error() {
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
        DiscoveryUnit::LocalRepo {
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
            "All efforts                           │  Ticket Repo Type Title                      Block   Age    ",
            "                                      │                       No open tickets                       ",
            "immybot · couldn't probe              │                                                             ",
            "main worktree /src/immybot does not e…│                                                             ",
            "                                      │                                                             ",
            "web · Map: broken                     │                                                             ",
            "local · couldn't read                 │                                                             ",
            "tickets/01-a.md: missing status       │                                                             ",
            "                                      │                                                             ",
            "                                      │                                                             ",
            "j/k nav  t PRs  q quit                                                       0 in-flight · 0 waiting",
        ]
    );
    assert_eq!(fg_of(&terminal, "couldn't read"), DARK.error);
    assert_eq!(fg_of(&terminal, "couldn't probe"), DARK.error);
}

#[test]
fn a_github_effort_reads_github_with_issue_refs_and_a_failed_map_read_says_couldnt_read() {
    let (mut model, _) = Model::new();
    let legit = RepoSlug::new("mayfieldiv/legit");
    let issue = |number: u64| TicketKey::GitHub {
        repo_slug: legit.clone(),
        number,
    };
    let github_ticket = |number, title: &str, dependencies| Ticket {
        key: issue(number),
        title: title.to_owned(),
        state: TicketState::Open,
        claim: None,
        ty: TicketType("task".to_owned()),
        dependencies,
    };
    let mut decided = github_ticket(116, "Domain types", Vec::new());
    decided.state = TicketState::Closed;
    decided.claim = Some(Claim::By("mayfieldiv".to_owned()));
    let effort = Effort::new(
        EffortKey::GitHub {
            repo_slug: legit.clone(),
            map_number: 123,
        },
        "Map: ticket surface".to_owned(),
        Some("All eight issues merged".to_owned()),
        vec![
            decided,
            github_ticket(
                117,
                "GitHub transport",
                vec![Dependency::SameEffort(issue(116))],
            ),
            github_ticket(
                120,
                "Fetch integration",
                vec![Dependency::SameEffort(issue(117))],
            ),
        ],
    )
    .unwrap();
    model
        .tickets
        .merge_effort(RepoIdentity::Slug(legit), EffortRead::Ready(effort));
    model.tickets.fail_discovery(
        DiscoveryUnit::GitHubRepo {
            slug: RepoSlug::new("acme/api"),
        },
        "GitHub GraphQL error: 404 Not Found".to_owned(),
    );
    model.view_mode = ViewMode::TicketList;

    let terminal = render(&model, 100, 12);

    // The card's source line reads `github`; refs are issue numbers; a closed
    // Dependency (#116) doesn't block #117, an open one (#117) blocks #120.
    // The failed map read leads the rail worded as a read, not a probe.
    assert_eq!(
        buffer_text(&terminal),
        vec![
            "legit — Tickets — 1 effort · 1 frontier                                                             ",
            "All efforts                           │  Ticket Repo  Type Title                     Block   Age    ",
            "                                      │  ── Frontier                                                ",
            "api · couldn't read                   │  #117   legit task GitHub transport          ↓1             ",
            "GitHub GraphQL error: 404 Not Found   │  ── Blocked                                                 ",
            "                                      │  #120   legit task Fetch integ… ⟨after #117⟩ ↑1             ",
            "legit · Map: ticket surface           │                                                             ",
            "github · 1/3 decided · 1 frontier     │                                                             ",
            "All eight issues merged               │                                                             ",
            "                                      │                                                             ",
            "                                      │                                                             ",
            "j/k nav  t PRs  q quit                                                       0 in-flight · 0 waiting",
        ]
    );
    assert_eq!(fg_of(&terminal, "couldn't read"), DARK.error);
}

#[test]
fn an_incomplete_map_read_leads_the_rail_as_a_warning_and_keeps_its_efforts() {
    let (mut model, _) = Model::new();
    let immybot = RepoSlug::new("immense/immybot");
    let effort = Effort::new(
        EffortKey::GitHub {
            repo_slug: immybot.clone(),
            map_number: 900,
        },
        "Map: memory image".to_owned(),
        Some("Owned tables in memory".to_owned()),
        vec![Ticket {
            key: TicketKey::GitHub {
                repo_slug: immybot.clone(),
                number: 901,
            },
            title: "Run the pilot".to_owned(),
            state: TicketState::Open,
            claim: None,
            ty: TicketType("task".to_owned()),
            dependencies: Vec::new(),
        }],
    )
    .unwrap();
    model.tickets.merge_effort(
        RepoIdentity::Slug(immybot.clone()),
        EffortRead::Ready(effort),
    );
    model.tickets.finish_discovery(
        DiscoveryUnit::GitHubRepo { slug: immybot },
        Some("more than 10 open maps; showing the first 10".to_owned()),
    );
    model.view_mode = ViewMode::TicketList;

    let terminal = render(&model, 100, 12);

    // The unit card says the read was short and why; the map it did read
    // keeps its own card and its tickets stay queued.
    assert_eq!(
        buffer_text(&terminal),
        vec![
            "legit — Tickets — 1 effort · 1 frontier                                                             ",
            "All efforts                           │  Ticket Repo    Type Title                   Block   Age    ",
            "                                      │  ── Frontier                                                ",
            "immybot · incomplete                  │  #901   immybot task Run the pilot                          ",
            "more than 10 open maps; showing the f…│                                                             ",
            "                                      │                                                             ",
            "immybot · Map: memory image           │                                                             ",
            "github · 0/1 decided · 1 frontier     │                                                             ",
            "Owned tables in memory                │                                                             ",
            "                                      │                                                             ",
            "                                      │                                                             ",
            "j/k nav  t PRs  q quit                                                       0 in-flight · 0 waiting",
        ]
    );
    assert_eq!(fg_of(&terminal, "incomplete"), DARK.warning);
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

    model.tickets.finish_discovery(DiscoveryUnit::Cwd, None);
    let terminal = render(&model, 60, 5);
    assert_eq!(
        buffer_text(&terminal)[1],
        "                      No efforts found                      "
    );
}

#[test]
fn long_refs_still_truncate_when_the_terminal_is_narrow() {
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

/// `title_cell` for a Blocked ticket whose marker is `⟨dep? gone.md⟩` (14
/// columns), rendered alone at `width` — the joined cell text.
fn dep_marker_cell(width: usize) -> String {
    let marker = RowMarker::UnknownDependency("gone.md".to_owned());
    let cell = super::title_cell(
        "Ship it",
        Some(&marker),
        width,
        ratatui::style::Style::default(),
        &DARK,
    );
    render_cells(vec![cell], None)
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

#[test]
fn the_state_marker_outranks_the_title_when_the_cell_is_tight() {
    assert_eq!(dep_marker_cell(30), "Ship it ⟨dep? gone.md⟩        ");
    assert_eq!(
        dep_marker_cell(16),
        "… ⟨dep? gone.md⟩",
        "one title column fits beside the marker: its ellipsis"
    );
    assert_eq!(
        dep_marker_cell(15),
        "⟨dep? gone.md⟩ ",
        "no room for a title glyph: the marker alone"
    );
    assert_eq!(
        dep_marker_cell(10),
        "⟨dep? gon…",
        "a marker wider than the cell truncates rather than vanishing"
    );
}

#[test]
fn wide_columns_fit_ticket_names_and_stay_stable_while_scrolling() {
    let (mut model, _) = Model::new();
    let short_ref = "019-pilot-soak-gate";
    let long_ref = "026-funnel-chunk-identification-procs";
    model.tickets.merge_effort(
        RepoIdentity::Slug(RepoSlug::new("immense/immybot-manager")),
        effort(
            "memory-image",
            "Memory-image system of record",
            "Keep the owned tables in memory",
            vec![
                ticket("memory-image", short_ref, "Run the pilot", "task"),
                ticket("memory-image", long_ref, "Plan the funnel", "research"),
            ],
        ),
    );
    model.view_mode = ViewMode::TicketList;
    model.tickets.resize(2);

    let before = buffer_text(&render(&model, 320, 8));
    assert!(before[3].contains("immybot-manager · Memory-image system of record"));
    assert!(before[3].contains(short_ref));
    assert!(before[3].contains("Run the pilot"));
    assert!(!before.iter().any(|row| row.contains(long_ref)));

    model.tickets.move_down();
    let after = buffer_text(&render(&model, 320, 8));
    assert!(after.iter().any(|row| row.contains(long_ref)));
    assert_eq!(before[1], after[1], "scrolling must not move the columns");

    let narrow = buffer_text(&render(&model, 140, 8));
    assert!(narrow[1].contains("Title"));
    assert!(narrow[1].contains("Block"));
    assert!(narrow[1].contains("Age"));
    assert!(narrow.iter().any(|row| row.contains("Plan the funnel")));
}
