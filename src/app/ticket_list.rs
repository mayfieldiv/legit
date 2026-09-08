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

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use crate::{
    app::list_scroll,
    config::RepoIdentity,
    ticket::{
        Claim, Effort, EffortKey, EffortRead, EffortSource, EffortTicket, TicketKey, TicketState,
    },
};

/// One pooled Effort and the Tracked Repo it belongs to. Attribution is
/// discovery-time data — where the Effort was found decides the repo today
/// (CONTEXT.md: a default, not a definition) — so it rides beside the read
/// rather than inside the domain type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffortEntry {
    pub repo: RepoIdentity,
    pub read: EffortRead,
    counts: TicketCounts,
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
    fn of(read: &EffortRead) -> Self {
        let EffortRead::Ready(effort) = read else {
            return Self::default();
        };
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

impl EffortEntry {
    fn new(repo: RepoIdentity, read: EffortRead) -> Self {
        let counts = TicketCounts::of(&read);
        Self { repo, read, counts }
    }

    pub fn key(&self) -> &EffortKey {
        match &self.read {
            EffortRead::Ready(effort) => &effort.key,
            EffortRead::Degraded { key, .. } => key,
        }
    }

    /// The Map's title.
    pub fn title(&self) -> String {
        match &self.read {
            EffortRead::Ready(effort) => effort.title.clone(),
            EffortRead::Degraded { title, .. } => title.clone(),
        }
    }

    pub fn destination(&self) -> Option<&str> {
        match &self.read {
            EffortRead::Ready(effort) => effort.destination.as_deref(),
            EffortRead::Degraded { destination, .. } => destination.as_deref(),
        }
    }

    pub fn source(&self) -> EffortSource {
        self.key().source()
    }

    /// Why the Effort degraded, or `None` when it read cleanly.
    pub fn error(&self) -> Option<&str> {
        match &self.read {
            EffortRead::Ready(_) => None,
            EffortRead::Degraded { reason, .. } => Some(reason),
        }
    }

    /// The normalized Effort; a degraded read has none (and no Tickets).
    pub fn effort(&self) -> Option<&Effort> {
        match &self.read {
            EffortRead::Ready(effort) => Some(effort),
            EffortRead::Degraded { .. } => None,
        }
    }

    /// Tallied once on arrival; the rail and header read them every frame.
    pub fn counts(&self) -> TicketCounts {
        self.counts
    }

    /// Rail order: repo, then Map title, then identity so the order is total
    /// (two Maps with one title in one repo can't swap between arrivals).
    fn order_key(&self) -> (String, String, String) {
        let identity = match self.key() {
            EffortKey::GitHub { map_number, .. } => map_number.to_string(),
            EffortKey::Local { dir } => dir.display().to_string(),
        };
        (self.repo.display_name(), self.title(), identity)
    }
}

/// The queue's tiers, in display order. Blocked also holds the
/// Unknown-Dependency Tickets (too rare for a tier of their own); they sort
/// last within it and carry a marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

    /// The tier an open Ticket lands in. An Unknown Dependency outranks
    /// everything: it is a data-integrity warning the queue must never hide
    /// (spec §6.1), so the `⟨dep? …⟩` row shows even for a claimed Ticket.
    /// Otherwise a claim outranks blocked-ness: someone already working on it
    /// is the more useful signal than what it still waits on.
    pub fn of(ticket: &EffortTicket<'_>) -> Self {
        if ticket.unknown_dependency_ref().is_some() {
            QueueTier::Blocked
        } else if ticket.claim.is_some() {
            QueueTier::Claimed
        } else if ticket.is_blocked() {
            QueueTier::Blocked
        } else {
            QueueTier::Frontier
        }
    }
}

/// The title's state marker (spec §6.2), one per non-Frontier row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowMarker {
    /// `⟨claimed X⟩`, or `⟨claimed⟩` for an anonymous claim.
    Claimed(Option<String>),
    /// `⟨after Y⟩`: the first open Dependency's display ref.
    After(String),
    /// `⟨dep? Z⟩`: the first Unknown Dependency's ref.
    UnknownDependency(String),
}

/// One Ticket's queue row: its identity plus everything the row shows that
/// is derived rather than stored on the Ticket, computed once per relayout so
/// a redraw only formats (ADR 0002: derivations live in `update`, never in
/// the render path).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketRow {
    pub key: TicketKey,
    pub tier: QueueTier,
    pub marker: Option<RowMarker>,
    /// Open upstream Dependencies — the `↑N` cell.
    pub upstream: usize,
    /// Open downstream dependents across every pooled Effort — the `↓N`
    /// cell. Pool-wide because Blocks (CONTEXT.md) is the reverse read of
    /// Dependency, and a Dependency can cross Efforts.
    pub downstream: usize,
}

impl TicketRow {
    fn derive(ticket: &EffortTicket<'_>, downstream: usize) -> Self {
        let tier = QueueTier::of(ticket);
        let marker = match tier {
            QueueTier::Frontier => None,
            QueueTier::Claimed => Some(RowMarker::Claimed(match &ticket.claim {
                Some(Claim::By(who)) => Some(who.clone()),
                Some(Claim::Anonymous) | None => None,
            })),
            QueueTier::Blocked => ticket
                .unknown_dependency_ref()
                .map(RowMarker::UnknownDependency)
                .or_else(|| {
                    ticket
                        .open_dependencies()
                        .next()
                        .map(|key| RowMarker::After(key.display_ref()))
                }),
        };
        Self {
            key: ticket.key.clone(),
            tier,
            marker,
            upstream: ticket.open_dependencies().count(),
            downstream,
        }
    }
}

/// One row in the rendered queue: a tier header or a Ticket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueueRow {
    Header(QueueTier),
    Ticket(TicketRow),
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
    /// The selected Ticket's identity; `None` only while the queue is empty.
    selected: Option<TicketKey>,
    /// Whether the user has moved the cursor. Until then the selection follows
    /// the top row as Efforts stream in and re-sort the queue; after, it
    /// sticks to its Ticket.
    pinned: bool,
    /// First visible display row (headers count toward the offset).
    scroll_offset: usize,
    viewport_height: usize,
}

impl fmt::Debug for TicketList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TicketList")
            .field("efforts", &self.efforts.len())
            .field("discoveries", &self.discoveries)
            .field("rows", &self.rows.len())
            .field("selected", &self.selected)
            .field("pinned", &self.pinned)
            .field("scroll_offset", &self.scroll_offset)
            .field("viewport_height", &self.viewport_height)
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
            .find(|existing| existing.key() == entry.key())
        {
            Some(existing) => *existing = entry,
            None => self.efforts.push(entry),
        }
        self.relayout();
    }

    pub fn begin_discovery(&mut self, unit: DiscoveryUnit) {
        self.discoveries.insert(unit, DiscoveryPhase::Loading);
    }

    pub fn finish_discovery(&mut self, unit: &DiscoveryUnit) {
        self.discoveries
            .insert(unit.clone(), DiscoveryPhase::Loaded);
    }

    pub fn fail_discovery(&mut self, unit: &DiscoveryUnit, error: String) {
        self.discoveries
            .insert(unit.clone(), DiscoveryPhase::Failed(error));
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

    /// Every unit that failed outright, as (unit label, error), in unit order.
    pub fn discovery_failures(&self) -> impl Iterator<Item = (&str, &str)> {
        self.discoveries
            .iter()
            .filter_map(|(unit, phase)| match phase {
                DiscoveryPhase::Failed(error) => Some((unit.label(), error.as_str())),
                _ => None,
            })
    }

    /// The pooled Efforts in rail order.
    pub fn efforts(&self) -> &[EffortEntry] {
        &self.efforts
    }

    /// Resolve a queue row's Ticket to its Effort entry and member handle.
    pub fn ticket(&self, key: &TicketKey) -> Option<(&EffortEntry, EffortTicket<'_>)> {
        self.efforts.iter().find_map(|entry| {
            let ticket = entry.effort()?.ticket(key)?;
            Some((entry, ticket))
        })
    }

    #[cfg(test)]
    pub fn rows(&self) -> &[QueueRow] {
        &self.rows
    }

    /// Whether the queue shows no Ticket rows — the placeholder state.
    pub fn visible_is_empty(&self) -> bool {
        !self
            .rows
            .iter()
            .any(|row| matches!(row, QueueRow::Ticket(_)))
    }

    pub fn selected_ticket(&self) -> Option<&TicketKey> {
        self.selected.as_ref()
    }

    #[cfg(test)]
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    #[cfg(test)]
    pub fn viewport_height(&self) -> usize {
        self.viewport_height
    }

    /// Iterate the display rows inside the scroll viewport, each with whether
    /// it is the selected Ticket. Headers are never selected.
    pub fn visible_rows(&self) -> impl Iterator<Item = (&QueueRow, bool)> {
        let start = self.scroll_offset.min(self.rows.len());
        let end = if self.viewport_height == 0 {
            self.rows.len()
        } else {
            (start + self.viewport_height).min(self.rows.len())
        };
        let selected = self.selected.as_ref();
        self.rows[start..end].iter().map(move |row| {
            let is_selected = matches!(row, QueueRow::Ticket(row) if Some(&row.key) == selected);
            (row, is_selected)
        })
    }

    pub fn move_down(&mut self) {
        self.step(1);
    }

    pub fn move_up(&mut self) {
        self.step(-1);
    }

    pub fn resize(&mut self, viewport_height: usize) {
        self.viewport_height = viewport_height;
        self.normalize_scroll();
    }

    /// Step the selection to the adjacent Ticket row in `delta`'s direction,
    /// skipping headers and clamping at the ends. Pins the cursor.
    fn step(&mut self, delta: isize) {
        self.pinned = true;
        let Some(current) = self.selected_display_row() else {
            return;
        };
        let candidates: Box<dyn Iterator<Item = usize>> = if delta > 0 {
            Box::new((current + 1)..self.rows.len())
        } else {
            Box::new((0..current).rev())
        };
        for row in candidates {
            if let QueueRow::Ticket(row) = &self.rows[row] {
                self.selected = Some(row.key.clone());
                break;
            }
        }
        self.normalize_scroll();
    }

    fn selected_display_row(&self) -> Option<usize> {
        let selected = self.selected.as_ref()?;
        self.rows
            .iter()
            .position(|row| matches!(row, QueueRow::Ticket(row) if row.key == *selected))
    }

    fn first_ticket(&self) -> Option<TicketKey> {
        self.rows.iter().find_map(|row| match row {
            QueueRow::Ticket(row) => Some(row.key.clone()),
            QueueRow::Header(_) => None,
        })
    }

    /// Rebuild rail order and the tiered queue, then re-anchor the cursor: an
    /// unpinned cursor follows the top row; a pinned one keeps its Ticket
    /// while that Ticket is still in the queue and snaps to the top otherwise.
    fn relayout(&mut self) {
        self.efforts.sort_by_cached_key(EffortEntry::order_key);

        let open_tickets = || {
            self.efforts
                .iter()
                .filter_map(EffortEntry::effort)
                .flat_map(Effort::tickets)
                .filter(|t| t.state == TicketState::Open)
        };
        // Blocks, pool-wide: how many open Tickets wait on each target.
        let mut dependents: HashMap<TicketKey, usize> = HashMap::new();
        for ticket in open_tickets() {
            for target in ticket
                .dependencies
                .iter()
                .filter_map(|dep| dep.target_key())
            {
                *dependents.entry(target.clone()).or_default() += 1;
            }
        }

        let mut frontier = Vec::new();
        let mut claimed = Vec::new();
        let mut blocked = Vec::new();
        let mut unknown = Vec::new();
        for ticket in open_tickets() {
            let downstream = dependents.get(&ticket.key).copied().unwrap_or(0);
            let row = TicketRow::derive(&ticket, downstream);
            match (row.tier, &row.marker) {
                (QueueTier::Frontier, _) => frontier.push(row),
                (QueueTier::Claimed, _) => claimed.push(row),
                (QueueTier::Blocked, Some(RowMarker::UnknownDependency(_))) => unknown.push(row),
                (QueueTier::Blocked, _) => blocked.push(row),
            }
        }
        blocked.append(&mut unknown);

        self.rows.clear();
        for (tier, members) in [
            (QueueTier::Frontier, frontier),
            (QueueTier::Claimed, claimed),
            (QueueTier::Blocked, blocked),
        ] {
            if members.is_empty() {
                continue;
            }
            self.rows.push(QueueRow::Header(tier));
            self.rows.extend(members.into_iter().map(QueueRow::Ticket));
        }

        let keep = self.pinned && self.selected_display_row().is_some();
        if !keep {
            self.selected = self.first_ticket();
        }
        self.normalize_scroll();
    }

    fn normalize_scroll(&mut self) {
        let Some(selected_row) = self.selected_display_row() else {
            self.scroll_offset =
                list_scroll::clamp(self.scroll_offset, self.rows.len(), self.viewport_height);
            return;
        };
        self.scroll_offset = list_scroll::follow_selection(
            self.scroll_offset,
            selected_row,
            self.rows.len(),
            self.viewport_height,
        );
    }
}

#[cfg(test)]
mod tests;
