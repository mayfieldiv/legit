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

use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use unicode_width::UnicodeWidthStr;

use crate::{
    app::list_cursor::{Direction, ListCursor, SelectableRow},
    canonical_path::CanonicalPathBuf,
    config::{RepoConfig, RepoIdentity},
    format::format_repo_short,
    repo_slug::RepoSlug,
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
    pub fetch: FetchState,
    /// The attributed Tracked Repo's short name (see `format_repo_short`).
    pub repo: String,
    /// The Map's title.
    pub title: String,
    pub source: EffortSource,
    /// The read's tallies and Destination, or why the Effort degraded (a
    /// degraded Effort has no Tickets).
    pub outcome: Result<EffortSummary, String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FetchState {
    pub fetched_at: Option<DateTime<Utc>>,
    pub refreshing: bool,
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
/// could attribute any Effort, a unit whose read settled but saw only a
/// window of it, or an Effort's card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RailCard<'a> {
    Failure {
        unit: &'a DiscoveryUnit,
        error: &'a str,
    },
    Incomplete {
        unit: &'a DiscoveryUnit,
        caveat: &'a str,
    },
    Effort(&'a EffortCard),
}

/// One pooled Effort: its card, plus the normalized Effort the queue rows are
/// derived from (`None` until a read succeeds).
#[derive(Clone, Debug, PartialEq, Eq)]
struct EffortEntry {
    key: EffortKey,
    repo: RepoIdentity,
    card: EffortCard,
    effort: Option<Effort>,
}

impl EffortEntry {
    fn new(repo: RepoIdentity, read: EffortRead, now: DateTime<Utc>) -> Self {
        let name = format_repo_short(&repo.display_name()).to_owned();
        match read {
            EffortRead::Ready(effort) => Self {
                key: effort.key.clone(),
                repo,
                card: EffortCard {
                    fetch: FetchState {
                        fetched_at: Some(now),
                        refreshing: false,
                    },
                    repo: name,
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
                repo,
                card: EffortCard {
                    fetch: FetchState::default(),
                    repo: name,
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

/// The title's state marker (spec §6.2), one per non-Frontier row. A row's
/// tier and queue rank are read off it (`tier`, `rank`).
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

    /// Queue order within the pool's rail-then-effort order: tier, with the
    /// Unknown-Dependency rows trailing Blocked.
    fn rank(marker: Option<&Self>) -> (QueueTier, bool) {
        (
            Self::tier(marker),
            matches!(marker, Some(RowMarker::UnknownDependency(_))),
        )
    }
}

/// One Ticket's queue row, self-contained: its identity plus everything the
/// row shows, resolved once per relayout so a redraw only formats.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketRow {
    pub fetch: FetchState,
    pub key: TicketKey,
    pub marker: Option<RowMarker>,
    pub display_ref: String,
    /// The attributed repo's short name.
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
    fn derive(repo: &str, ticket: &EffortTicket<'_>, downstream: usize, fetch: FetchState) -> Self {
        let open: Vec<&TicketKey> = ticket.open_dependencies().collect();
        Self {
            fetch,
            key: ticket.key.clone(),
            marker: RowMarker::of(ticket, open.first().copied()),
            display_ref: ticket.key.display_ref(),
            repo: repo.to_owned(),
            ty: ticket.ty.clone(),
            title: ticket.title.clone(),
            upstream: open.len(),
            downstream,
        }
    }

    pub fn tier(&self) -> QueueTier {
        RowMarker::tier(self.marker.as_ref())
    }

    fn rank(&self) -> (QueueTier, bool) {
        RowMarker::rank(self.marker.as_ref())
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
    pub repo: usize,
    pub ty: usize,
}

impl QueueContentWidths {
    fn fit(&mut self, row: &TicketRow) {
        self.display_ref = self.display_ref.max(row.display_ref.width());
        self.repo = self.repo.max(row.repo.width());
        self.ty = self.ty.max(row.ty.0.width());
    }
}

/// One unit of Effort discovery — a Tracked Repo's local worktree fan-out,
/// the cwd walk, or a PR-capable Tracked Repo's GitHub map read — whose phase
/// the queue tracks so the view can tell "still discovering" from "nothing
/// found" and surface a unit that failed outright (a missing Main Worktree, a
/// map read GitHub refused: neither has an Effort card to degrade).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DiscoveryUnit {
    LocalRepo {
        /// The repo's short name — its slug's repo half, or its Main
        /// Worktree basename.
        name: String,
        /// The configured path, verbatim: two slug-less repos can share a
        /// basename, so the name alone would merge their units.
        main_worktree_path: String,
    },
    Cwd,
    /// One repo's whole-map read: every open `wayfinder:map` issue it holds.
    GitHubRepo {
        slug: RepoSlug,
    },
}

impl DiscoveryUnit {
    /// The probe unit for one configured Tracked Repo, or `None` when it has
    /// no filesystem to probe: a slug-only repo has no Main Worktree. The
    /// display name can only be missing on a `RepoConfig` that `validate`
    /// would have rejected (neither slug nor path), so that reads as the same
    /// `None` rather than a panic in the reducer.
    pub fn for_repo(repo: &RepoConfig) -> Option<Self> {
        let main_worktree_path = repo.main_worktree_path.clone()?;
        let name = format_repo_short(&repo.display_name().ok()?).to_owned();
        Some(DiscoveryUnit::LocalRepo {
            name,
            main_worktree_path,
        })
    }

    /// The name the unit is shown under when it fails.
    pub fn label(&self) -> &str {
        match self {
            DiscoveryUnit::LocalRepo { name, .. } => name,
            DiscoveryUnit::Cwd => "cwd",
            DiscoveryUnit::GitHubRepo { slug } => format_repo_short(slug.as_str()),
        }
    }

    /// Where the unit's Efforts come from, so a failed unit can be worded by
    /// source the way an Effort card is.
    pub fn source(&self) -> EffortSource {
        match self {
            DiscoveryUnit::LocalRepo { .. } | DiscoveryUnit::Cwd => EffortSource::Local,
            DiscoveryUnit::GitHubRepo { .. } => EffortSource::GitHub,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReadOutcome {
    Ready,
    Degraded,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DiscoveryPhase {
    Loading {
        reads: HashMap<EffortKey, ReadOutcome>,
        caveat: Option<String>,
    },
    Loaded,
    /// Settled with everything the read could see pooled, but the unit holds
    /// more than the read's window. Terminal like `Loaded` — a re-read sees
    /// the same window — but the rail must say so (spec §5.5).
    Incomplete(String),
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FetchUnit {
    Discovery(DiscoveryUnit),
    Local { dir: CanonicalPathBuf },
}

impl FetchUnit {
    pub fn for_effort(key: &EffortKey) -> Self {
        match key {
            EffortKey::GitHub { repo_slug, .. } => Self::Discovery(DiscoveryUnit::GitHubRepo {
                slug: repo_slug.clone(),
            }),
            EffortKey::Local { dir } => Self::Local { dir: dir.clone() },
        }
    }
}

#[derive(Clone, Debug)]
pub enum RefreshTarget {
    Discovery(DiscoveryUnit),
    Local {
        dir: CanonicalPathBuf,
        repo: RepoIdentity,
    },
}

impl RefreshTarget {
    fn for_entry(entry: &EffortEntry) -> Self {
        match &entry.key {
            EffortKey::GitHub { repo_slug, .. } => Self::Discovery(DiscoveryUnit::GitHubRepo {
                slug: repo_slug.clone(),
            }),
            EffortKey::Local { dir } => Self::Local {
                dir: dir.clone(),
                repo: entry.repo.clone(),
            },
        }
    }

    pub fn unit(&self) -> FetchUnit {
        match self {
            Self::Discovery(unit) => FetchUnit::Discovery(unit.clone()),
            Self::Local { dir, .. } => FetchUnit::Local { dir: dir.clone() },
        }
    }
}

#[derive(Clone, Default)]
pub struct TicketList {
    /// Pooled Efforts in rail order (see `EffortEntry::order_key`).
    efforts: Vec<EffortEntry>,
    discoveries: BTreeMap<DiscoveryUnit, DiscoveryPhase>,
    refreshing: HashSet<FetchUnit>,
    refreshed_efforts: HashSet<EffortKey>,
    refresh_failed: bool,
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

    pub fn selected_fetch(&self) -> Option<RefreshTarget> {
        let selected = self.selected_ticket()?;
        self.efforts.iter().find_map(|entry| {
            entry
                .effort
                .as_ref()?
                .tickets()
                .any(|ticket| &ticket.key == selected)
                .then(|| RefreshTarget::for_entry(entry))
        })
    }

    pub fn all_fetches(&self) -> Vec<RefreshTarget> {
        self.efforts
            .iter()
            .map(RefreshTarget::for_entry)
            .chain(
                self.discoveries
                    .keys()
                    .filter(|unit| matches!(unit, DiscoveryUnit::GitHubRepo { .. }))
                    .cloned()
                    .map(RefreshTarget::Discovery),
            )
            .collect()
    }

    pub fn failed_fetches(&self) -> Vec<RefreshTarget> {
        self.efforts
            .iter()
            .filter(|entry| entry.card.outcome.is_err())
            .map(RefreshTarget::for_entry)
            .chain(
                self.discoveries
                    .iter()
                    .filter(|(_, phase)| matches!(phase, DiscoveryPhase::Failed(_)))
                    .map(|(unit, _)| RefreshTarget::Discovery(unit.clone())),
            )
            .collect()
    }

    pub fn begin_refresh(&mut self, unit: FetchUnit) -> bool {
        if self.refreshing.is_empty() {
            self.refreshed_efforts.clear();
            self.refresh_failed = false;
        }
        let inserted = self.refreshing.insert(unit);
        self.relayout();
        inserted
    }

    pub fn finish_refresh(&mut self, unit: &FetchUnit, succeeded: bool) -> Option<usize> {
        if !self.refreshing.remove(unit) {
            return None;
        }
        self.refresh_failed |= !succeeded;
        self.relayout();
        (self.refreshing.is_empty() && !self.refresh_failed).then_some(self.refreshed_efforts.len())
    }

    pub fn is_refreshing(&self, unit: &FetchUnit) -> bool {
        self.refreshing.contains(unit)
    }

    pub fn has_data(&self, key: &EffortKey) -> bool {
        self.efforts
            .iter()
            .any(|entry| entry.key == *key && entry.effort.is_some())
    }

    pub fn discovery_is_loading(&self, unit: &DiscoveryUnit) -> bool {
        matches!(
            self.discoveries.get(unit),
            Some(DiscoveryPhase::Loading { .. })
        )
    }

    /// Failed reads preserve previously loaded data and its Fetch Age.
    pub fn merge_effort(&mut self, repo: RepoIdentity, read: EffortRead, now: DateTime<Utc>) {
        let entry = EffortEntry::new(repo, read, now);
        self.record_refresh_read(
            &FetchUnit::for_effort(&entry.key),
            &entry.key,
            if entry.effort.is_some() {
                ReadOutcome::Ready
            } else {
                ReadOutcome::Degraded
            },
        );
        match self
            .efforts
            .iter_mut()
            .find(|existing| existing.key == entry.key)
        {
            Some(existing) if entry.effort.is_some() || existing.effort.is_none() => {
                *existing = entry
            }
            Some(_) => {}
            None => self.efforts.push(entry),
        }
        self.relayout();
    }

    pub fn record_discovery_read(&mut self, unit: &DiscoveryUnit, read: &EffortRead) {
        let (key, outcome) = match read {
            EffortRead::Ready(effort) => (&effort.key, ReadOutcome::Ready),
            EffortRead::Degraded { key, .. } => (key, ReadOutcome::Degraded),
        };
        self.record_refresh_read(&FetchUnit::Discovery(unit.clone()), key, outcome);
        if let Some(DiscoveryPhase::Loading { reads, .. }) = self.discoveries.get_mut(unit) {
            reads.insert(key.clone(), outcome);
        }
    }

    fn record_refresh_read(&mut self, unit: &FetchUnit, key: &EffortKey, outcome: ReadOutcome) {
        if !self.is_refreshing(unit) {
            return;
        }
        match outcome {
            ReadOutcome::Ready => {
                self.refreshed_efforts.insert(key.clone());
            }
            ReadOutcome::Degraded => self.refresh_failed = true,
        }
    }

    pub fn begin_discovery(&mut self, unit: DiscoveryUnit) {
        let caveat = match self.discoveries.get(&unit) {
            Some(DiscoveryPhase::Incomplete(caveat)) => Some(caveat.clone()),
            _ => None,
        };
        self.discoveries.insert(
            unit,
            DiscoveryPhase::Loading {
                reads: HashMap::new(),
                caveat,
            },
        );
    }

    /// Settle `unit`: complete, or with the caveat of a read that saw only a
    /// window of it (`Msg::DiscoveryFinished`).
    pub fn finish_discovery(
        &mut self,
        unit: DiscoveryUnit,
        incomplete: Option<String>,
        now: DateTime<Utc>,
    ) {
        if let Some(DiscoveryPhase::Loading { reads, .. }) = self.discoveries.get(&unit) {
            if incomplete.is_none()
                && let DiscoveryUnit::GitHubRepo { slug } = &unit
            {
                self.efforts.retain(|entry| {
                    !matches!(&entry.key, EffortKey::GitHub { repo_slug, .. } if repo_slug == slug)
                        || reads.contains_key(&entry.key)
                });
            }
            for entry in &mut self.efforts {
                if reads.get(&entry.key) == Some(&ReadOutcome::Ready) {
                    entry.card.fetch.fetched_at = Some(now);
                }
            }
            self.relayout();
        }
        let phase = match incomplete {
            None => DiscoveryPhase::Loaded,
            Some(caveat) => DiscoveryPhase::Incomplete(caveat),
        };
        self.discoveries.insert(unit, phase);
    }

    pub fn fail_discovery(&mut self, unit: DiscoveryUnit, error: String) {
        let has_stale_data = matches!(&unit, DiscoveryUnit::GitHubRepo { slug }
            if self.efforts.iter().any(|entry| entry.effort.is_some()
                && matches!(&entry.key, EffortKey::GitHub { repo_slug, .. } if repo_slug == slug)));
        let phase = if has_stale_data {
            match self.discoveries.get(&unit) {
                Some(DiscoveryPhase::Loading {
                    caveat: Some(caveat),
                    ..
                }) => DiscoveryPhase::Incomplete(caveat.clone()),
                _ => DiscoveryPhase::Loaded,
            }
        } else {
            DiscoveryPhase::Failed(error)
        };
        self.discoveries.insert(unit, phase);
    }

    /// Whether `unit` should have discovery dispatched: never run, or its
    /// last run failed. False while in flight or settled — re-running then
    /// would only redo work the pool already holds (an incomplete read would
    /// see the same window again).
    pub fn needs_discovery(&self, unit: &DiscoveryUnit) -> bool {
        match self.discoveries.get(unit) {
            None | Some(DiscoveryPhase::Failed(_)) => true,
            Some(
                DiscoveryPhase::Loading { .. }
                | DiscoveryPhase::Loaded
                | DiscoveryPhase::Incomplete(_),
            ) => false,
        }
    }

    /// Whether any discovery unit is still in flight.
    pub fn is_loading(&self) -> bool {
        self.discoveries
            .values()
            .any(|phase| matches!(phase, DiscoveryPhase::Loading { .. }))
    }

    /// The rail in display order: every unit that failed outright or settled
    /// incomplete, then the Effort cards in rail order. The unit cards lead
    /// because the rail doesn't scroll yet — below the Efforts, a full rail
    /// would push them offscreen with no way to reach them (spec §5.5, never
    /// silently missing).
    // TODO(#133): rail scrolling with the effort filter.
    pub fn rail(&self) -> impl Iterator<Item = RailCard<'_>> {
        let units = self
            .discoveries
            .iter()
            .filter_map(|(unit, phase)| match phase {
                DiscoveryPhase::Failed(error) => Some(RailCard::Failure { unit, error }),
                DiscoveryPhase::Incomplete(caveat)
                | DiscoveryPhase::Loading {
                    caveat: Some(caveat),
                    ..
                } => Some(RailCard::Incomplete { unit, caveat }),
                DiscoveryPhase::Loading { .. } | DiscoveryPhase::Loaded => None,
            });
        units.chain(
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
        for entry in &mut self.efforts {
            entry.card.fetch.refreshing =
                self.refreshing.contains(&FetchUnit::for_effort(&entry.key));
        }
        self.efforts.sort_by_cached_key(EffortEntry::order_key);

        // Blocks, pool-wide: how many open Tickets wait on each target.
        let mut dependents: HashMap<TicketKey, usize> = HashMap::new();
        for (_, ticket, _) in self.queued_tickets() {
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
            .map(|(repo, ticket, fetch)| {
                let downstream = dependents.get(&ticket.key).copied().unwrap_or(0);
                TicketRow::derive(repo, &ticket, downstream, fetch)
            })
            .collect();
        // Stable, so within a tier the pool's rail-then-effort order holds.
        tickets.sort_by_key(TicketRow::rank);

        let mut widths = QueueContentWidths::default();
        let mut rows = Vec::with_capacity(tickets.len() + 3);
        let mut open_tier = None;
        for row in tickets {
            widths.fit(&row);
            let tier = row.tier();
            if open_tier != Some(tier) {
                open_tier = Some(tier);
                rows.push(QueueRow::Header(tier));
            }
            rows.push(QueueRow::Ticket(row));
        }
        self.rows = rows;
        self.content_widths = widths;
        self.cursor.re_anchor(&self.rows);
    }

    /// Every open Ticket of every pooled Effort with its repo's display name,
    /// in rail then effort order.
    fn queued_tickets(&self) -> impl Iterator<Item = (&str, EffortTicket<'_>, FetchState)> {
        self.efforts
            .iter()
            .filter_map(|entry| {
                Some((
                    entry.card.repo.as_str(),
                    entry.effort.as_ref()?,
                    entry.card.fetch,
                ))
            })
            .flat_map(|(repo, effort, fetch)| {
                effort
                    .tickets()
                    .filter(|ticket| ticket.state == TicketState::Open)
                    .map(move |ticket| (repo, ticket, fetch))
            })
    }
}

#[cfg(test)]
mod tests;
