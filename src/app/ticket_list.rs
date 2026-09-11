//! Ticket Queue Module: the pooled Efforts of every Tracked Repo, each with
//! the repo it is attributed to, plus the queue's tier-grouped display rows,
//! the selection cursor and scroll viewport, and the per-unit phases of
//! Effort discovery. The sibling of `pr_list` for the ticket surface.
//!
//! Efforts pool in rail order (repo, then Map title) whatever order their
//! reads arrive in. The queue flattens every pooled Effort's open Tickets into
//! tiers — Frontier, Claimed, Blocked — with a header row per
//! non-empty tier; within a tier, rail order then effort order. Closed Tickets
//! never appear (only the rail's `N/M decided` counts them). Selection tracks a
//! Ticket's identity, so arrivals that re-sort the queue move its row, never
//! which Ticket is selected.
//!
//! Everything the surface shows is derived here, once per relayout, into
//! self-contained rail cards and queue rows (ADR 0002: derivations live in
//! `update`, never in the render path). The view formats them; it never
//! reaches back into the pool.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use unicode_width::UnicodeWidthStr;

use crate::{
    app::list_cursor::{Direction, ListCursor, SelectableRow},
    config::RepoIdentity,
    format::format_repo_short,
    ticket::{
        Claim, Effort, EffortKey, EffortRead, EffortSource, EffortTicket, TicketKey, TicketState,
        TicketType,
    },
};

/// One Effort's rail card: what the rail shows for it, whether the read
/// succeeded or degraded. Attribution is discovery-time data — where the
/// Effort was found decides the repo today (CONTEXT.md: a default, not a
/// definition) — so the repo rides here beside the read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffortCard {
    /// The attributed Tracked Repo's display name.
    pub repo: String,
    /// The Map's title.
    pub title: String,
    pub source: EffortSource,
    /// The read's tallies and Destination, or why the Effort degraded (a
    /// degraded Effort has no Tickets).
    pub outcome: Result<EffortSummary, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffortSummary {
    pub counts: TicketCounts,
    pub destination: Option<String>,
}

/// The rail card's ticket tallies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TicketCounts {
    /// Closed Tickets — resolved or ruled out of scope, indistinguishably.
    pub decided: usize,
    pub total: usize,
    /// Tickets on the Frontier.
    pub frontier: usize,
}

impl TicketCounts {
    fn of(effort: &Effort) -> Self {
        Self {
            decided: effort
                .tickets()
                .filter(|t| t.state == TicketState::Closed)
                .count(),
            total: effort.tickets().count(),
            frontier: effort.frontier().count(),
        }
    }
}

/// One rail entry in display order: a discovery unit that failed before it
/// could attribute any Effort, or an Effort's card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RailCard<'a> {
    Failure { unit: &'a str, error: &'a str },
    Effort(&'a EffortCard),
}

/// One pooled Effort: its card, plus the normalized Effort the queue rows are
/// derived from (`None` once the read degraded).
#[derive(Clone, Debug, PartialEq, Eq)]
struct EffortEntry {
    key: EffortKey,
    card: EffortCard,
    effort: Option<Effort>,
}

impl EffortEntry {
    fn new(repo: RepoIdentity, read: EffortRead) -> Self {
        let repo = repo.display_name();
        match read {
            EffortRead::Ready(effort) => Self {
                key: effort.key.clone(),
                card: EffortCard {
                    repo,
                    title: effort.title.clone(),
                    source: effort.key.source(),
                    outcome: Ok(EffortSummary {
                        counts: TicketCounts::of(&effort),
                        destination: effort.destination.clone(),
                    }),
                },
                effort: Some(effort),
            },
            EffortRead::Degraded {
                key,
                title,
                destination: _,
                reason,
            } => Self {
                card: EffortCard {
                    repo,
                    title,
                    source: key.source(),
                    outcome: Err(reason),
                },
                key,
                effort: None,
            },
        }
    }

    /// Rail order: repo, then Map title, then identity so the order is total
    /// (two Maps with one title in one repo can't swap between arrivals).
    fn order_key(&self) -> (String, String, String) {
        let identity = match &self.key {
            EffortKey::GitHub { map_number, .. } => map_number.to_string(),
            EffortKey::Local { dir } => dir.display().to_string(),
        };
        (self.card.repo.clone(), self.card.title.clone(), identity)
    }
}

/// The queue's tiers, in display order. Blocked also holds the
/// Unknown-Dependency Tickets (too rare for a tier of their own); they sort
/// last within it and carry a marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum QueueTier {
    Frontier,
    Claimed,
    Blocked,
}

impl QueueTier {
    pub fn label(self) -> &'static str {
        match self {
            QueueTier::Frontier => "Frontier",
            QueueTier::Claimed => "Claimed",
            QueueTier::Blocked => "Blocked",
        }
    }
}

/// The title's state marker (spec §6.2), one per non-Frontier row. The
/// marker decides the tier: a row's tier is a function of its marker, so
/// the two can't disagree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowMarker {
    /// `⟨claimed X⟩`, or `⟨claimed⟩` for an anonymous claim.
    Claimed(Option<String>),
    /// `⟨after Y⟩`: the first open Dependency's display ref.
    After(String),
    /// `⟨dep? Z⟩`: the first Unknown Dependency's ref.
    UnknownDependency(String),
}

impl RowMarker {
    /// An Unknown Dependency outranks everything: it is a data-integrity
    /// warning the queue must never hide (spec §6.1), so the `⟨dep? …⟩` row
    /// shows even for a claimed Ticket. Otherwise a claim outranks
    /// blocked-ness: someone already working on it is the more useful signal
    /// than what it still waits on.
    fn of(ticket: &EffortTicket<'_>, first_open_dependency: Option<&TicketKey>) -> Option<Self> {
        ticket
            .unknown_dependency_ref()
            .map(RowMarker::UnknownDependency)
            .or_else(|| {
                ticket.claim.as_ref().map(|claim| {
                    RowMarker::Claimed(match claim {
                        Claim::By(who) => Some(who.clone()),
                        Claim::Anonymous => None,
                    })
                })
            })
            .or_else(|| first_open_dependency.map(|key| RowMarker::After(key.display_ref())))
    }

    fn tier(marker: Option<&Self>) -> QueueTier {
        match marker {
            None => QueueTier::Frontier,
            Some(RowMarker::Claimed(_)) => QueueTier::Claimed,
            Some(RowMarker::After(_) | RowMarker::UnknownDependency(_)) => QueueTier::Blocked,
        }
    }
}

/// One Ticket's queue row, self-contained: its identity plus everything the
/// row shows, resolved once per relayout so a redraw only formats.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketRow {
    pub key: TicketKey,
    pub tier: QueueTier,
    pub marker: Option<RowMarker>,
    pub display_ref: String,
    /// The attributed repo's display name.
    pub repo: String,
    pub ty: TicketType,
    pub title: String,
    /// Open upstream Dependencies — the `↑N` cell.
    pub upstream: usize,
    /// Open downstream dependents across every pooled Effort — the `↓N`
    /// cell. Pool-wide because Blocks (CONTEXT.md) is the reverse read of
    /// Dependency, and a Dependency can cross Efforts.
    pub downstream: usize,
}

impl TicketRow {
    fn derive(repo: &str, ticket: &EffortTicket<'_>, downstream: usize) -> Self {
        let open: Vec<&TicketKey> = ticket.open_dependencies().collect();
        let marker = RowMarker::of(ticket, open.first().copied());
        Self {
            key: ticket.key.clone(),
            tier: RowMarker::tier(marker.as_ref()),
            marker,
            display_ref: ticket.key.display_ref(),
            repo: repo.to_owned(),
            ty: ticket.ty.clone(),
            title: ticket.title.clone(),
            upstream: open.len(),
            downstream,
        }
    }

    /// Queue order within the pool's rail-then-effort order: tier, with the
    /// Unknown-Dependency rows trailing Blocked.
    fn order(&self) -> (QueueTier, bool) {
        (
            self.tier,
            matches!(self.marker, Some(RowMarker::UnknownDependency(_))),
        )
    }
}

/// One row in the rendered queue: a tier header or a Ticket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueueRow {
    Header(QueueTier),
    Ticket(TicketRow),
}

impl SelectableRow for QueueRow {
    type Id = TicketKey;
    fn id(&self) -> Option<&TicketKey> {
        match self {
            QueueRow::Ticket(row) => Some(&row.key),
            QueueRow::Header(_) => None,
        }
    }
}

/// The widest content each fixed queue column has to fit, over the queued
/// Tickets — measured once per relayout so a redraw only sizes columns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueueContentWidths {
    pub display_ref: usize,
    /// Of the repo's short name, as the cell shows it.
    pub repo: usize,
    pub ty: usize,
}

impl QueueContentWidths {
    fn fit(&mut self, row: &TicketRow) {
        self.display_ref = self.display_ref.max(row.display_ref.width());
        self.repo = self.repo.max(format_repo_short(&row.repo).width());
        self.ty = self.ty.max(row.ty.0.width());
    }
}

/// One unit of Effort discovery — a Tracked Repo's local worktree fan-out, or
/// the cwd walk — whose phase the queue tracks so the view can tell "still
/// discovering" from "nothing found" and surface a unit that failed outright
/// (a missing Main Worktree has no Effort card to degrade). A GitHub repo's
/// map read is the same kind of unit and joins this enum with its slice.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiscoveryUnit {
    LocalRepo {
        /// The repo's display name — its slug or Main Worktree basename.
        name: String,
        /// The configured path, verbatim: two slug-less repos can share a
        /// basename, so the name alone would merge their units.
        main_worktree_path: String,
    },
    Cwd,
}

impl DiscoveryUnit {
    /// The name the unit is shown under when it fails.
    pub fn label(&self) -> &str {
        match self {
            DiscoveryUnit::LocalRepo { name, .. } => name,
            DiscoveryUnit::Cwd => "cwd",
        }
    }
}

/// Lifecycle of one discovery unit; at most one variant holds per unit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscoveryPhase {
    Loading,
    Loaded,
    Failed(String),
}

#[derive(Clone, Default)]
pub struct TicketList {
    /// Pooled Efforts in rail order (see `EffortEntry::order_key`).
    efforts: Vec<EffortEntry>,
    discoveries: BTreeMap<DiscoveryUnit, DiscoveryPhase>,
    /// Flattened display layout (tier headers + Ticket rows), rebuilt by
    /// `relayout` whenever the pool changes.
    rows: Vec<QueueRow>,
    content_widths: QueueContentWidths,
    /// The selection (a Ticket's identity) and scroll viewport over `rows`.
    cursor: ListCursor<TicketKey>,
}

impl fmt::Debug for TicketList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TicketList")
            .field("efforts", &self.efforts.len())
            .field("discoveries", &self.discoveries)
            .field("rows", &self.rows.len())
            .field("cursor", &self.cursor)
            .finish()
    }
}

impl TicketList {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pool one Effort read, attributed to `repo`. A re-read of an already
    /// pooled Effort (same key) replaces it wholesale — one map read or probe
    /// reconciles membership and enrichment together, so there is nothing to
    /// graft back.
    pub fn merge_effort(&mut self, repo: RepoIdentity, read: EffortRead) {
        let entry = EffortEntry::new(repo, read);
        match self
            .efforts
            .iter_mut()
            .find(|existing| existing.key == entry.key)
        {
            Some(existing) => *existing = entry,
            None => self.efforts.push(entry),
        }
        self.relayout();
    }

    pub fn begin_discovery(&mut self, unit: DiscoveryUnit) {
        self.discoveries.insert(unit, DiscoveryPhase::Loading);
    }

    pub fn finish_discovery(&mut self, unit: DiscoveryUnit) {
        self.discoveries.insert(unit, DiscoveryPhase::Loaded);
    }

    pub fn fail_discovery(&mut self, unit: DiscoveryUnit, error: String) {
        self.discoveries.insert(unit, DiscoveryPhase::Failed(error));
    }

    /// Whether `unit` should have discovery dispatched: never run, or its
    /// last run failed. False while in flight or loaded — re-running then
    /// would only redo work the pool already holds.
    pub fn needs_discovery(&self, unit: &DiscoveryUnit) -> bool {
        match self.discoveries.get(unit) {
            None | Some(DiscoveryPhase::Failed(_)) => true,
            Some(DiscoveryPhase::Loading | DiscoveryPhase::Loaded) => false,
        }
    }

    /// Whether any discovery unit is still in flight.
    pub fn is_loading(&self) -> bool {
        self.discoveries
            .values()
            .any(|phase| *phase == DiscoveryPhase::Loading)
    }

    /// The rail in display order: every unit that failed outright, then the
    /// Effort cards in rail order. Failures lead because the rail doesn't
    /// scroll yet — below the Efforts, a full rail would push them offscreen
    /// with no way to reach them (spec §5.5, never silently missing).
    // TODO(#133): rail scrolling with the effort filter.
    pub fn rail(&self) -> impl Iterator<Item = RailCard<'_>> {
        let failures = self
            .discoveries
            .iter()
            .filter_map(|(unit, phase)| match phase {
                DiscoveryPhase::Failed(error) => Some(RailCard::Failure {
                    unit: unit.label(),
                    error,
                }),
                DiscoveryPhase::Loading | DiscoveryPhase::Loaded => None,
            });
        failures.chain(
            self.efforts
                .iter()
                .map(|entry| RailCard::Effort(&entry.card)),
        )
    }

    pub fn content_widths(&self) -> QueueContentWidths {
        self.content_widths
    }

    /// Whether the queue shows no Ticket rows — the placeholder state.
    pub fn visible_is_empty(&self) -> bool {
        !self
            .rows
            .iter()
            .any(|row| matches!(row, QueueRow::Ticket(_)))
    }

    /// The selected Ticket; `None` only while the queue is empty.
    pub fn selected_ticket(&self) -> Option<&TicketKey> {
        self.cursor.selected()
    }

    #[cfg(test)]
    pub fn scroll_offset(&self) -> usize {
        self.cursor.offset()
    }

    #[cfg(test)]
    pub fn viewport_height(&self) -> usize {
        self.cursor.height()
    }

    /// The display rows inside the scroll viewport, each flagged when it is
    /// the selected Ticket.
    pub fn visible_rows(&self) -> impl Iterator<Item = (&QueueRow, bool)> {
        self.cursor.visible_rows(&self.rows)
    }

    pub fn move_down(&mut self) {
        self.cursor.step(&self.rows, Direction::Down);
    }

    pub fn move_up(&mut self) {
        self.cursor.step(&self.rows, Direction::Up);
    }

    pub fn resize(&mut self, viewport_height: usize) {
        self.cursor.resize(&self.rows, viewport_height);
    }

    /// Rebuild rail order and the tiered queue, then re-anchor the cursor
    /// (see `ListCursor::re_anchor`).
    fn relayout(&mut self) {
        self.efforts.sort_by_cached_key(EffortEntry::order_key);

        // Blocks, pool-wide: how many open Tickets wait on each target.
        let mut dependents: HashMap<TicketKey, usize> = HashMap::new();
        for (_, ticket) in self.queued_tickets() {
            for target in ticket
                .dependencies
                .iter()
                .filter_map(|dep| dep.target_key())
            {
                *dependents.entry(target.clone()).or_default() += 1;
            }
        }
        let mut tickets: Vec<TicketRow> = self
            .queued_tickets()
            .map(|(repo, ticket)| {
                let downstream = dependents.get(&ticket.key).copied().unwrap_or(0);
                TicketRow::derive(repo, &ticket, downstream)
            })
            .collect();
        // Stable, so within a tier the pool's rail-then-effort order holds.
        tickets.sort_by_key(TicketRow::order);

        let mut widths = QueueContentWidths::default();
        let mut rows = Vec::with_capacity(tickets.len() + 3);
        let mut open_tier = None;
        for row in tickets {
            widths.fit(&row);
            if open_tier != Some(row.tier) {
                open_tier = Some(row.tier);
                rows.push(QueueRow::Header(row.tier));
            }
            rows.push(QueueRow::Ticket(row));
        }
        self.rows = rows;
        self.content_widths = widths;
        self.cursor.re_anchor(&self.rows);
    }

    /// Every open Ticket of every pooled Effort with its repo's display name,
    /// in rail then effort order.
    fn queued_tickets(&self) -> impl Iterator<Item = (&str, EffortTicket<'_>)> {
        self.efforts
            .iter()
            .filter_map(|entry| Some((entry.card.repo.as_str(), entry.effort.as_ref()?)))
            .flat_map(|(repo, effort)| {
                effort
                    .tickets()
                    .filter(|ticket| ticket.state == TicketState::Open)
                    .map(move |ticket| (repo, ticket))
            })
    }
}

#[cfg(test)]
mod tests;
