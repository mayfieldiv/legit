//! Wayfinder ticket domain model.
//!
//! Pure types and derivations for the ticket surface — no I/O, no async.
//! Mirrors the `### Wayfinder tickets` glossary in CONTEXT.md exactly: an
//! Effort is one Map plus its Tickets, and Mode, blocked-ness, the Frontier,
//! and Blocks are always derived, never stored. The GitHub transport (#117)
//! and local dialect parser (#118) normalize their wire/file shapes into
//! these types; the fetch and view layers consume them.

use crate::canonical_path::CanonicalPathBuf;
use crate::repo_slug::RepoSlug;

/// A Ticket's kind — the `wayfinder:<type>` label or the dialect's type
/// field. Deliberately an open string: unknown Types are shown verbatim,
/// never hidden, and only feed [`Mode`] derivation as "unknown".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketType(pub String);

/// Which kind of session can take a Ticket. Derived from [`TicketType`],
/// never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Agent alone — `research`.
    Afk,
    /// Human in the loop — `prototype`, `grilling`.
    Hitl,
    /// Matches both filtered views — `task` and unknown Types.
    Either,
}

impl TicketType {
    /// The Mode this Type derives (see [`Mode`]'s variants for the mapping).
    pub fn mode(&self) -> Mode {
        match self.0.as_str() {
            "research" => Mode::Afk,
            "prototype" | "grilling" => Mode::Hitl,
            _ => Mode::Either,
        }
    }
}

/// A Ticket's lifecycle axis. Closed covers both resolved and
/// closed-as-out-of-scope — no dialect encodes the distinction structurally,
/// so the model doesn't either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketState {
    Open,
    Closed,
}

/// The in-progress marker on an open Ticket — the GitHub assignee, or the
/// dialect's claim field. Orthogonal to [`TicketState`]. A claimed Ticket is
/// off the Frontier; legit only renders claims, never takes or releases them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    /// Claimed by a known claimant (GitHub assignee login, or the older
    /// dialect's populated `assignee` field).
    By(String),
    /// Claimed, claimant unknown — the newer local dialect's
    /// `Status: claimed` records claimed-ness without a name; render as
    /// claimed without one.
    Anonymous,
}

/// Globally-unique Ticket identity across Efforts and sources. Both
/// variants carry identity-safe parts: [`RepoSlug`] unifies casings, and
/// [`CanonicalPathBuf`] is constructible only by canonicalizing.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TicketKey {
    /// A GitHub sub-issue: issue numbers are only unique within a repo, so
    /// the key pairs the repo slug with the number (the `PrKey` pattern).
    GitHub { repo_slug: RepoSlug, number: u64 },
    /// A local ticket file, keyed by its canonical path.
    Local { path: CanonicalPathBuf },
}

impl TicketKey {
    /// The ref a Ticket is shown under (spec §6.2): GitHub `#NNN`; local,
    /// the file slug — the filename without its extension. Slugs drift after
    /// rescopes, which is why they are the ref and never the title.
    pub fn display_ref(&self) -> String {
        match self {
            TicketKey::GitHub { number, .. } => format!("#{number}"),
            TicketKey::Local { path } => path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
        }
    }
}

/// Globally-unique Effort identity across Tracked Repos and sources; the
/// same identity-safe parts as [`TicketKey`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EffortKey {
    /// A GitHub map issue (labelled `wayfinder:map`), keyed like its tickets.
    GitHub {
        repo_slug: RepoSlug,
        map_number: u64,
    },
    /// A local Effort, keyed by its canonical effort directory.
    Local { dir: CanonicalPathBuf },
}

impl EffortKey {
    /// Where the Effort's data comes from, read off its identity — one source
    /// of truth, so the attribute can never disagree with the key.
    pub fn source(&self) -> EffortSource {
        match self {
            EffortKey::GitHub { .. } => EffortSource::GitHub,
            EffortKey::Local { .. } => EffortSource::Local,
        }
    }
}

/// Where an Effort's data comes from — an attribute of the Effort, not a
/// different kind of container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffortSource {
    GitHub,
    Local,
}

/// A directed edge between Tickets: this Ticket waits on that one. A Ticket
/// with any open or Unknown Dependency is off the Frontier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dependency {
    /// The target is a Ticket of the same Effort; its state and title are
    /// resolved by lookup in the Effort's tickets, never stored on the edge.
    /// A target the lookup can't find degrades to an Unknown Dependency.
    SameEffort(TicketKey),
    /// The target lives in another Effort, so lookup can't reach it; the
    /// reader captures what it saw instead.
    External(ExternalDependency),
    /// The target can't be found or read; the raw ref is kept for display
    /// ("<raw ref> — can't find or read").
    Unknown { raw: String },
}

impl Dependency {
    /// The target's key, when one is known — an Unknown Dependency has none.
    pub fn target_key(&self) -> Option<&TicketKey> {
        match self {
            Dependency::SameEffort(key) => Some(key),
            Dependency::External(external) => Some(&external.key),
            Dependency::Unknown { .. } => None,
        }
    }
}

/// What the reader captured about an External Dependency's target: the
/// GitHub `blockedBy` node's fields, or the local target file's parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalDependency {
    pub key: TicketKey,
    pub state: TicketState,
    /// The target's title when the reader could see it (GitHub payloads
    /// always carry it; a local target file may be title-less).
    pub title: Option<String>,
}

/// A single decision or investigation belonging to an Effort — a GitHub
/// sub-issue of the Map, or a local ticket file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    pub key: TicketKey,
    /// The issue title, or the file's H1 with its frontmatter title as a
    /// fallback. Never the filename slug, which can drift after rescopes.
    pub title: String,
    pub state: TicketState,
    pub claim: Option<Claim>,
    pub ty: TicketType,
    pub dependencies: Vec<Dependency>,
}

/// One Effort's outcome from a source read — the per-Effort degradation
/// boundary (spec §5.5) made structural: an Effort is either fully
/// normalized or visibly degraded, never silently partial. Both transports
/// (the GitHub map read and the local Effort parse) produce it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffortRead {
    Ready(Effort),
    /// The source couldn't be represented as a complete Effort — a
    /// normalization failure, an unparseable ticket or map file, or a
    /// detected-but-unparsed fallback dialect. Identity and map context
    /// survive the failure.
    Degraded {
        key: EffortKey,
        title: String,
        destination: Option<String>,
        /// Human-readable cause.
        reason: String,
    },
}

/// A unit of wayfinding work: one Map plus its Tickets. The Map's own data
/// (title, Destination) lives directly on the Effort — the Map is the
/// artifact anchoring it, not a separate model type. Belongs to exactly one
/// Tracked Repo: a GitHub Effort names it in its key; a local Effort's repo
/// attribution is discovery-time data the fetch layer supplies beside the
/// read (`app::ticket_list::EffortEntry`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effort {
    pub key: EffortKey,
    /// The Map's title.
    pub title: String,
    /// The Destination stated on the Map, when its body carries one.
    pub destination: Option<String>,
    /// Private so [`Effort::new`]'s unique-key guarantee can't be bypassed;
    /// read through [`Effort::tickets`] / [`Effort::ticket`], which hand out
    /// member handles.
    tickets: Vec<Ticket>,
}

impl Effort {
    /// Construct an Effort, rejecting duplicate Ticket keys — two Tickets
    /// with one identity would make every keyed lookup silently pick a
    /// winner. Neither real source can produce one (GitHub sub-issue
    /// numbers and canonical paths are unique), so a duplicate is malformed
    /// input, surfaced per-Effort like any other parse failure.
    ///
    /// A Ticket's repeated Dependency edges collapse to one: Dependency is a
    /// relation, so `blocked-by: [1, 1]` names one target, and every count
    /// over the edges (open upstream, Blocks) reads distinct targets.
    pub fn new(
        key: EffortKey,
        title: String,
        destination: Option<String>,
        mut tickets: Vec<Ticket>,
    ) -> anyhow::Result<Self> {
        for (i, ticket) in tickets.iter().enumerate() {
            anyhow::ensure!(
                !tickets[..i].iter().any(|prev| prev.key == ticket.key),
                "duplicate ticket key {:?}",
                ticket.key
            );
        }
        for ticket in &mut tickets {
            let mut distinct = Vec::with_capacity(ticket.dependencies.len());
            for dependency in ticket.dependencies.drain(..) {
                if !distinct.contains(&dependency) {
                    distinct.push(dependency);
                }
            }
            ticket.dependencies = distinct;
        }
        Ok(Self {
            key,
            title,
            destination,
            tickets,
        })
    }

    /// This Effort's Tickets as member handles, in effort order — the
    /// position each holds is what [`Effort::ticket_at`] takes.
    pub fn tickets(&self) -> impl Iterator<Item = EffortTicket<'_>> {
        self.tickets.iter().map(|ticket| EffortTicket {
            effort: self,
            ticket,
        })
    }

    /// The member at `index` in effort order, as [`Effort::tickets`]
    /// enumerates them.
    pub fn ticket_at(&self, index: usize) -> Option<EffortTicket<'_>> {
        self.tickets.get(index).map(|ticket| EffortTicket {
            effort: self,
            ticket,
        })
    }

    /// Look up one of this Effort's Tickets by key.
    pub fn ticket(&self, key: &TicketKey) -> Option<EffortTicket<'_>> {
        self.tickets().find(|t| &t.key == key)
    }

    /// The Tickets a session can take right now, in effort order.
    pub fn frontier(&self) -> impl Iterator<Item = EffortTicket<'_>> {
        self.tickets().filter(|t| t.is_on_frontier())
    }
}

/// A Dependency as the owning Effort can see it: a target with a known state,
/// or one that can't be found or read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyStatus<'a> {
    Known {
        key: &'a TicketKey,
        state: TicketState,
    },
    /// The ref to show — the raw ref, or a missing same-effort target's
    /// display ref.
    Unknown(String),
}

/// One of an Effort's Tickets, resolved against the Effort that owns it —
/// the only thing the derivations hang off, so a Ticket can never be asked
/// about an Effort it doesn't belong to. Handed out exclusively by
/// [`Effort::tickets`] / [`Effort::ticket`] / [`Effort::frontier`]; derefs
/// to the underlying [`Ticket`] for its data.
#[derive(Clone, Copy)]
pub struct EffortTicket<'a> {
    effort: &'a Effort,
    ticket: &'a Ticket,
}

impl std::ops::Deref for EffortTicket<'_> {
    type Target = Ticket;

    fn deref(&self) -> &Ticket {
        self.ticket
    }
}

impl std::fmt::Debug for EffortTicket<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.ticket.fmt(f)
    }
}

impl EffortTicket<'_> {
    /// Each Dependency resolved against what this Effort can see, in
    /// declaration order. A same-effort target the lookup can't find is an
    /// Unknown Dependency (its display ref stands in for the raw ref).
    pub fn dependency_statuses(&self) -> impl Iterator<Item = DependencyStatus<'_>> {
        self.ticket.dependencies.iter().map(|dep| match dep {
            Dependency::SameEffort(key) => match self.effort.ticket(key) {
                Some(target) => DependencyStatus::Known {
                    key,
                    state: target.state,
                },
                None => DependencyStatus::Unknown(key.display_ref()),
            },
            Dependency::External(external) => DependencyStatus::Known {
                key: &external.key,
                state: external.state,
            },
            Dependency::Unknown { raw } => DependencyStatus::Unknown(raw.clone()),
        })
    }

    /// The targets this Ticket still waits on — every known Dependency whose
    /// target is open, in declaration order.
    pub fn open_dependencies(&self) -> impl Iterator<Item = &TicketKey> {
        self.dependency_statuses()
            .filter_map(|status| match status {
                DependencyStatus::Known {
                    key,
                    state: TicketState::Open,
                } => Some(key),
                _ => None,
            })
    }

    /// The first Unknown Dependency's ref, for the `⟨dep? <ref>⟩` marker.
    pub fn unknown_dependency_ref(&self) -> Option<String> {
        self.dependency_statuses().find_map(|status| match status {
            DependencyStatus::Unknown(raw) => Some(raw),
            DependencyStatus::Known { .. } => None,
        })
    }

    /// Whether this Ticket waits on anything: any open or Unknown
    /// Dependency. Always derived, never stored.
    pub fn is_blocked(&self) -> bool {
        self.dependency_statuses().any(|status| match status {
            DependencyStatus::Known { state, .. } => state == TicketState::Open,
            DependencyStatus::Unknown(_) => true,
        })
    }

    /// Whether this Ticket is on the Frontier: open, unclaimed, every
    /// Dependency target closed, and no Unknown Dependency.
    pub fn is_on_frontier(&self) -> bool {
        self.ticket.state == TicketState::Open && self.ticket.claim.is_none() && !self.is_blocked()
    }
}

#[cfg(test)]
mod tests;
