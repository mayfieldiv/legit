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
//! The queue also owns the Effort refresh cycle: which Fetch Units `r`/`R`
//! cover, which are in flight, and when a run has settled enough to summarize.
//! The reducer only builds commands and posts what `RefreshNotice` tells it.
//!
//! Everything the surface shows is derived here, once per relayout, into
//! self-contained rail cards and queue rows (ADR 0002: derivations live in
//! `update`, never in the render path). The view formats them; it never
//! reaches back into the pool.

use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use unicode_width::UnicodeWidthStr;

mod summary;
pub use summary::{DependencySummary, TicketSummary};

use crate::{
    app::list_cursor::{Direction, ListCursor, SelectableRow},
    canonical_path::CanonicalPathBuf,
    config::{RepoConfig, RepoIdentity},
    format::format_repo_short,
    repo_slug::RepoSlug,
    ticket::{
        Claim, Effort, EffortKey, EffortRead, EffortSource, EffortTicket, Mode, TicketKey,
        TicketState, TicketType,
    },
};

/// One Effort's rail card: what the rail shows for it, whether the read
/// succeeded or degraded. Attribution is discovery-time data — where the
/// Effort was found decides the repo today (CONTEXT.md: a default, not a
/// definition) — so the repo rides here beside the read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffortCard {
    pub selected: bool,
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
                    selected: false,
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
                    selected: false,
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
        self.ty = self
            .ty
            .max(row.ty.0.width() + if row.ty.mode() == Mode::Either { 1 } else { 0 });
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

impl ReadOutcome {
    fn of(read: &EffortRead) -> (&EffortKey, Self) {
        match read {
            EffortRead::Ready(effort) => (&effort.key, Self::Ready),
            EffortRead::Degraded { key, .. } => (key, Self::Degraded),
        }
    }
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

/// The Fetch Unit (CONTEXT.md) an Effort's data rides on: its repo's map
/// read, or its own directory probe.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum FetchUnit {
    Discovery(DiscoveryUnit),
    Local { dir: CanonicalPathBuf },
}

impl FetchUnit {
    fn for_effort(key: &EffortKey) -> Self {
        match key {
            EffortKey::GitHub { repo_slug, .. } => Self::Discovery(DiscoveryUnit::GitHubRepo {
                slug: repo_slug.clone(),
            }),
            EffortKey::Local { dir } => Self::Local { dir: dir.clone() },
        }
    }
}

/// Which Fetch Units a refresh key covers (CONTEXT.md, Refresh).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshScope {
    /// `r`: the unit backing the selected Ticket. With nothing selected there
    /// is nothing to re-read — the queue has no Re-list.
    Selected,
    /// `R`: every unit backing the view.
    View,
}

/// One Fetch Unit to re-read, with what its command needs beyond the unit's
/// identity.
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

    fn unit(&self) -> FetchUnit {
        match self {
            Self::Discovery(unit) => FetchUnit::Discovery(unit.clone()),
            Self::Local { dir, .. } => FetchUnit::Local { dir: dir.clone() },
        }
    }
}

/// What the status line owes the user after one fetch arrival. At most one
/// per arrival: a failure marks its unit, which withholds the summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RefreshNotice {
    /// Every unit the user asked for settled clean: how many Efforts the run
    /// re-read.
    Refreshed(usize),
    /// A read the user asked for failed, or a degraded read arrived over data
    /// the queue already shows. Worded for the status line.
    Failed(String),
}

/// One Fetch Unit's place in the current refresh run. Absent means idle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UnitRefresh {
    /// Dispatched, not settled. `degraded` once a read under it came back
    /// degraded: the unit then settles Failed however its stream ends.
    InFlight { degraded: bool },
    /// Settled failed. Withholds the run's summary until re-dispatched, which
    /// puts it back in flight, or until the next run starts.
    Failed,
}

#[derive(Clone, Default)]
pub struct TicketList {
    /// Pooled Efforts in rail order (see `EffortEntry::order_key`).
    efforts: Vec<EffortEntry>,
    effort_filter: Option<EffortKey>,
    mode_filter: ModeFilter,
    repo_scope: Option<RepoScope>,
    summary: Option<TicketSummary>,
    discoveries: BTreeMap<DiscoveryUnit, DiscoveryPhase>,
    /// The current refresh run: every unit it dispatched that hasn't settled
    /// clean, and the Efforts re-read so far. A run starts on the first
    /// dispatch while no unit is in flight.
    refresh: HashMap<FetchUnit, UnitRefresh>,
    refreshed_efforts: HashSet<EffortKey>,
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

    pub fn set_repo_scope(&mut self, scope: Option<RepoScope>) {
        if self.repo_scope == scope {
            return;
        }
        self.repo_scope = scope;
        self.effort_filter = None;
        self.relayout();
        self.cursor.reset_to_top(&self.rows);
        self.refresh_summary();
    }

    fn repo_matches(&self, entry: &EffortEntry) -> bool {
        self.repo_scope
            .as_ref()
            .is_none_or(|scope| entry.repo == RepoIdentity::Slug(scope.repo.clone()))
    }

    fn rail_efforts(&self) -> impl Iterator<Item = &EffortEntry> {
        self.efforts
            .iter()
            .filter(|entry| self.repo_matches(entry))
            .filter(|entry| match &entry.card.outcome {
                Ok(summary) => {
                    summary.counts.total == 0 || summary.counts.decided < summary.counts.total
                }
                Err(_) => true,
            })
    }

    fn effort_matches(&self, entry: &EffortEntry) -> bool {
        self.repo_matches(entry)
            && self
                .effort_filter
                .as_ref()
                .is_none_or(|key| key == &entry.key)
    }

    fn discovery_matches(&self, unit: &DiscoveryUnit) -> bool {
        self.repo_scope.as_ref().is_none_or(|scope| match unit {
            DiscoveryUnit::GitHubRepo { slug } => slug == &scope.repo,
            _ => scope.discoveries.contains(unit),
        })
    }

    pub fn cycle_mode(&mut self) {
        self.mode_filter = match self.mode_filter {
            ModeFilter::All => ModeFilter::Afk,
            ModeFilter::Afk => ModeFilter::Hitl,
            ModeFilter::Hitl => ModeFilter::All,
        };
        self.relayout();
    }

    pub fn mode_filter(&self) -> ModeFilter {
        self.mode_filter
    }

    pub fn effort_filter_label(&self) -> &str {
        self.efforts
            .iter()
            .find(|entry| Some(&entry.key) == self.effort_filter.as_ref())
            .map_or("All efforts", |entry| entry.card.title.as_str())
    }

    pub fn all_efforts_selected(&self) -> bool {
        self.effort_filter.is_none()
    }

    pub fn step_effort(&mut self, direction: Direction) {
        let choices: Vec<_> = std::iter::once(None)
            .chain(self.rail_efforts().map(|entry| Some(entry.key.clone())))
            .collect();
        let current = choices
            .iter()
            .position(|key| key == &self.effort_filter)
            .unwrap_or(0);
        let next = match direction {
            Direction::Down => (current + 1).min(choices.len() - 1),
            Direction::Up => current.saturating_sub(1),
        };
        self.effort_filter = choices[next].clone();
        self.relayout();
        self.cursor.reset_to_top(&self.rows);
        self.refresh_summary();
    }

    // ── refresh ──────────────────────────────────────────────────────────────

    /// Dispatch a refresh: `scope`'s units plus every unit whose last read
    /// failed (a failed card has no selectable Ticket, so both keys retry
    /// it), minus any unit already in flight — a refresh, or a startup
    /// discovery still Loading. `dispatch` builds each survivor's command;
    /// a unit is marked in flight only when it returns `Some`, so the
    /// indicator never outlives a command that was never sent. Commands come
    /// back in rail order.
    pub fn begin_refresh<T>(
        &mut self,
        scope: RefreshScope,
        mut dispatch: impl FnMut(&RefreshTarget) -> Option<T>,
    ) -> Vec<T> {
        if !self
            .refresh
            .values()
            .any(|unit| matches!(unit, UnitRefresh::InFlight { .. }))
        {
            self.refresh.clear();
            self.refreshed_efforts.clear();
        }
        let scoped = match scope {
            RefreshScope::Selected => self.selected_fetch().into_iter().collect(),
            RefreshScope::View => self.all_fetches(),
        };
        let mut dispatched = Vec::new();
        for target in scoped.into_iter().chain(self.failed_fetches()) {
            let unit = target.unit();
            if self.in_flight(&unit) {
                continue;
            }
            let Some(cmd) = dispatch(&target) else {
                continue;
            };
            self.refresh
                .insert(unit, UnitRefresh::InFlight { degraded: false });
            if let RefreshTarget::Discovery(unit) = target {
                self.begin_discovery(unit);
            }
            dispatched.push(cmd);
        }
        if !dispatched.is_empty() {
            self.relayout();
        }
        dispatched
    }

    fn selected_fetch(&self) -> Option<RefreshTarget> {
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

    /// Every pooled Effort's unit, plus each repo's map read even when it
    /// pooled nothing — the repo may have gained a map since.
    fn all_fetches(&self) -> Vec<RefreshTarget> {
        self.efforts
            .iter()
            .filter(|entry| self.effort_matches(entry))
            .map(RefreshTarget::for_entry)
            .chain(
                self.discoveries
                    .keys()
                    .filter(|unit| self.effort_filter.is_none() && self.discovery_matches(unit))
                    .filter(|unit| matches!(unit, DiscoveryUnit::GitHubRepo { .. }))
                    .cloned()
                    .map(RefreshTarget::Discovery),
            )
            .collect()
    }

    fn failed_fetches(&self) -> Vec<RefreshTarget> {
        self.efforts
            .iter()
            .filter(|entry| self.effort_matches(entry))
            .filter(|entry| entry.card.outcome.is_err())
            .map(RefreshTarget::for_entry)
            .chain(
                self.discoveries
                    .iter()
                    .filter(|(unit, _)| {
                        self.effort_filter.is_none() && self.discovery_matches(unit)
                    })
                    .filter(|(_, phase)| matches!(phase, DiscoveryPhase::Failed(_)))
                    .map(|(unit, _)| RefreshTarget::Discovery(unit.clone())),
            )
            .collect()
    }

    fn refreshing(&self, unit: &FetchUnit) -> bool {
        matches!(self.refresh.get(unit), Some(UnitRefresh::InFlight { .. }))
    }

    fn in_flight(&self, unit: &FetchUnit) -> bool {
        self.refreshing(unit)
            || matches!(unit, FetchUnit::Discovery(discovery)
                if matches!(self.discoveries.get(discovery), Some(DiscoveryPhase::Loading { .. })))
    }

    fn has_data(&self, key: &EffortKey) -> bool {
        self.efforts
            .iter()
            .any(|entry| entry.key == *key && entry.effort.is_some())
    }

    /// Credit a read to the refresh of the unit that delivered it, if any.
    fn record_read(&mut self, unit: &FetchUnit, key: &EffortKey, outcome: ReadOutcome) {
        let Some(UnitRefresh::InFlight { degraded }) = self.refresh.get_mut(unit) else {
            return;
        };
        match outcome {
            ReadOutcome::Ready => {
                self.refreshed_efforts.insert(key.clone());
            }
            ReadOutcome::Degraded => *degraded = true,
        }
    }

    /// Settle `unit`'s refresh. The run's summary posts once its last unit
    /// settles with none left failed.
    fn settle(&mut self, unit: &FetchUnit, succeeded: bool) -> Option<RefreshNotice> {
        let Some(UnitRefresh::InFlight { degraded }) = self.refresh.get(unit).copied() else {
            return None;
        };
        if succeeded && !degraded {
            self.refresh.remove(unit);
        } else {
            self.refresh.insert(unit.clone(), UnitRefresh::Failed);
        }
        self.relayout();
        self.refresh
            .is_empty()
            .then_some(RefreshNotice::Refreshed(self.refreshed_efforts.len()))
    }

    /// A degraded read is status-worthy when the user asked for it, or when
    /// it arrives over data the queue already shows; a startup probe's
    /// degraded Effort just gets its card.
    fn degraded_notice(&self, unit: &FetchUnit, read: &EffortRead) -> Option<RefreshNotice> {
        let EffortRead::Degraded {
            key, title, reason, ..
        } = read
        else {
            return None;
        };
        (self.refreshing(unit) || self.has_data(key))
            .then(|| RefreshNotice::Failed(format!("{title}: {reason}")))
    }

    // ── arrivals ─────────────────────────────────────────────────────────────

    /// Pool an Effort a discovery unit streamed (`Msg::EffortArrived`),
    /// crediting the unit's refresh if one is in flight.
    pub fn effort_arrived(
        &mut self,
        unit: &DiscoveryUnit,
        repo: RepoIdentity,
        read: EffortRead,
        now: DateTime<Utc>,
    ) -> Option<RefreshNotice> {
        let fetch = FetchUnit::Discovery(unit.clone());
        let (key, outcome) = ReadOutcome::of(&read);
        self.record_read(&fetch, key, outcome);
        if let Some(DiscoveryPhase::Loading { reads, .. }) = self.discoveries.get_mut(unit) {
            reads.insert(key.clone(), outcome);
        }
        let notice = self.degraded_notice(&fetch, &read);
        self.merge_effort(repo, read, now);
        notice
    }

    /// Pool one local probe's result (`Msg::LocalEffortRead`). A local unit
    /// is one read, so the read settles it: Ready re-stamps its Fetch Age;
    /// degraded or `Err` keeps the stale data and age.
    pub fn local_effort_read(
        &mut self,
        dir: CanonicalPathBuf,
        repo: RepoIdentity,
        result: Result<EffortRead, String>,
        now: DateTime<Utc>,
    ) -> Option<RefreshNotice> {
        let unit = FetchUnit::Local { dir };
        let read = match result {
            Ok(read) => read,
            Err(error) => {
                self.settle(&unit, false);
                return Some(RefreshNotice::Failed(error));
            }
        };
        let (key, outcome) = ReadOutcome::of(&read);
        self.record_read(&unit, key, outcome);
        let notice = self.degraded_notice(&unit, &read);
        self.merge_effort(repo, read, now);
        let settled = self.settle(&unit, outcome == ReadOutcome::Ready);
        notice.or(settled)
    }

    /// Pool one read. A degraded read never displaces loaded data: the card
    /// keeps the last successful read's tallies and Fetch Age. The pool
    /// primitive — arrivals go through `effort_arrived` and
    /// `local_effort_read`, which also credit the unit's refresh.
    pub fn merge_effort(&mut self, repo: RepoIdentity, read: EffortRead, now: DateTime<Utc>) {
        let entry = EffortEntry::new(repo, read, now);
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

    // ── discovery ────────────────────────────────────────────────────────────

    /// Put `unit` in flight as a startup discovery: no indicator, no refresh
    /// accounting. A refresh of a discovery unit goes through `begin_refresh`.
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
    /// window of it (`Msg::DiscoveryFinished`). A complete map read is the
    /// repo's whole membership, so maps it didn't return are dropped; an
    /// incomplete one retains them.
    pub fn finish_discovery(
        &mut self,
        unit: DiscoveryUnit,
        incomplete: Option<String>,
        now: DateTime<Utc>,
    ) -> Option<RefreshNotice> {
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
        let fetch = FetchUnit::Discovery(unit.clone());
        self.discoveries.insert(unit, phase);
        self.settle(&fetch, true)
    }

    /// `unit` failed before it could settle (`Msg::DiscoveryFailed`). With
    /// stale data pooled the unit stays settled so the data keeps its cards
    /// and Fetch Age; without, the rail gets a failure card.
    pub fn fail_discovery(&mut self, unit: DiscoveryUnit, error: String) -> Option<RefreshNotice> {
        let fetch = FetchUnit::Discovery(unit.clone());
        let notice = self
            .refreshing(&fetch)
            .then(|| RefreshNotice::Failed(format!("{}: {error}", unit.label())));
        self.settle(&fetch, false);
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
        notice
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

    /// Failed and incomplete discovery units lead the Effort cards so the
    /// unfiltered rail exposes read failures before potentially many Efforts.
    pub fn rail(&self) -> impl Iterator<Item = RailCard<'_>> {
        let units = self
            .discoveries
            .iter()
            .filter(|(unit, _)| self.discovery_matches(unit))
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
            self.rail_efforts()
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
        self.refresh_summary();
    }

    pub fn move_up(&mut self) {
        self.cursor.step(&self.rows, Direction::Up);
        self.refresh_summary();
    }

    pub fn resize(&mut self, viewport_height: usize) {
        self.cursor.resize(&self.rows, viewport_height);
    }

    pub fn scroll(&mut self, direction: Direction, rows: usize) {
        match direction {
            Direction::Down => self.cursor.scroll_down(&self.rows, rows),
            Direction::Up => self.cursor.scroll_up(&self.rows, rows),
        }
    }

    /// Rebuild rail order and the tiered queue, then re-anchor the cursor
    /// (see `ListCursor::re_anchor`).
    fn relayout(&mut self) {
        let refresh = &self.refresh;
        for entry in &mut self.efforts {
            entry.card.fetch.refreshing = matches!(
                refresh.get(&FetchUnit::for_effort(&entry.key)),
                Some(UnitRefresh::InFlight { .. })
            );
        }
        self.efforts.sort_by_cached_key(EffortEntry::order_key);
        if self
            .effort_filter
            .as_ref()
            .is_some_and(|key| !self.rail_efforts().any(|entry| &entry.key == key))
        {
            self.effort_filter = None;
        }
        for entry in &mut self.efforts {
            entry.card.selected = Some(&entry.key) == self.effort_filter.as_ref();
        }

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
            .filter(|(entry, ticket)| {
                self.effort_matches(entry) && self.mode_filter.matches(ticket.ty.mode())
            })
            .map(|(entry, ticket)| {
                let downstream = dependents.get(&ticket.key).copied().unwrap_or(0);
                TicketRow::derive(&entry.card.repo, &ticket, downstream, entry.card.fetch)
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
        self.refresh_summary();
    }

    /// Open Tickets in rail then effort order, before any view filters.
    fn queued_tickets(&self) -> impl Iterator<Item = (&EffortEntry, EffortTicket<'_>)> {
        self.efforts
            .iter()
            .filter_map(|entry| Some((entry, entry.effort.as_ref()?)))
            .flat_map(|(entry, effort)| {
                effort
                    .tickets()
                    .filter(|ticket| ticket.state == TicketState::Open)
                    .map(move |ticket| (entry, ticket))
            })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ModeFilter {
    #[default]
    All,
    Afk,
    Hitl,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoScope {
    pub repo: RepoSlug,
    /// Local discovery units attributed to this Repo Tab, including failures
    /// that produced no Effort from which attribution could be recovered.
    pub discoveries: Vec<DiscoveryUnit>,
}

impl ModeFilter {
    fn matches(self, mode: Mode) -> bool {
        matches!(
            (self, mode),
            (Self::All, _) | (_, Mode::Either) | (Self::Afk, Mode::Afk) | (Self::Hitl, Mode::Hitl)
        )
    }
}

#[cfg(test)]
mod tests;
