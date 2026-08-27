//! Wayfinder ticket domain model.
//!
//! Pure types and derivations for the ticket surface — no I/O, no async.
//! Mirrors the `### Wayfinder tickets` glossary in CONTEXT.md exactly: an
//! Effort is one Map plus its Tickets, and Mode, blocked-ness, the Frontier,
//! and Blocks are always derived, never stored. The GitHub transport (#117)
//! and local dialect parser (#118) normalize their wire/file shapes into
//! these types; the fetch and view layers consume them.

// TODO(#117): remove once the transport/fetch layers consume this module.
#![allow(dead_code)]

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

/// Globally-unique Ticket identity across Efforts and sources.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TicketKey {
    /// A GitHub sub-issue: issue numbers are only unique within a repo, so
    /// the key pairs the repo slug with the number (the `PrKey` pattern).
    GitHub { repo_slug: String, number: u64 },
    /// A local ticket file, keyed by its canonical path.
    Local { path: std::path::PathBuf },
}

/// Globally-unique Effort identity across Tracked Repos and sources.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EffortKey {
    /// A GitHub map issue (labelled `wayfinder:map`), keyed like its tickets.
    GitHub { repo_slug: String, map_number: u64 },
    /// A local Effort, keyed by its canonical effort directory.
    Local { dir: std::path::PathBuf },
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
    /// The issue title / the file's H1 (never the filename slug — slugs
    /// drift after rescopes).
    pub title: String,
    pub state: TicketState,
    pub claim: Option<Claim>,
    pub ty: TicketType,
    pub dependencies: Vec<Dependency>,
}

/// A unit of wayfinding work: one Map plus its Tickets. The Map's own data
/// (title, Destination) lives directly on the Effort — the Map is the
/// artifact anchoring it, not a separate model type. Belongs to exactly one
/// Tracked Repo: a GitHub Effort names it in its key; a local Effort's repo
/// attribution is discovery-time data the fetch layer supplies (#118/#120).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effort {
    pub key: EffortKey,
    /// The Map's title.
    pub title: String,
    /// The Destination stated on the Map, when its body carries one.
    pub destination: Option<String>,
    pub tickets: Vec<Ticket>,
}

impl Effort {
    /// Where this Effort's data comes from, read off its key — one source of
    /// truth, so the attribute can never disagree with the identity.
    pub fn source(&self) -> EffortSource {
        match self.key {
            EffortKey::GitHub { .. } => EffortSource::GitHub,
            EffortKey::Local { .. } => EffortSource::Local,
        }
    }

    /// Look up one of this Effort's Tickets by key.
    pub fn ticket(&self, key: &TicketKey) -> Option<&Ticket> {
        self.tickets.iter().find(|t| &t.key == key)
    }

    /// Whether this Ticket waits on anything: any open or Unknown
    /// Dependency. Always derived, never stored. A same-effort target the
    /// lookup can't find counts as an Unknown Dependency, so it blocks.
    pub fn is_blocked(&self, ticket: &Ticket) -> bool {
        ticket.dependencies.iter().any(|dep| match dep {
            Dependency::SameEffort(key) => self
                .ticket(key)
                .is_none_or(|t| t.state == TicketState::Open),
            Dependency::External(external) => external.state == TicketState::Open,
            Dependency::Unknown { .. } => true,
        })
    }

    /// Whether this Ticket is on the Frontier: open, unclaimed, every
    /// Dependency target closed, and no Unknown Dependency.
    pub fn is_on_frontier(&self, ticket: &Ticket) -> bool {
        ticket.state == TicketState::Open && ticket.claim.is_none() && !self.is_blocked(ticket)
    }

    /// The Tickets a session can take right now, in effort order.
    pub fn frontier(&self) -> impl Iterator<Item = &Ticket> {
        self.tickets.iter().filter(|t| self.is_on_frontier(t))
    }

    /// Blocks — the reverse read of Dependency: the open Tickets of this
    /// Effort whose Dependencies include the given one, in effort order.
    pub fn blocks(&self, key: &TicketKey) -> Vec<&Ticket> {
        self.tickets
            .iter()
            .filter(|t| t.state == TicketState::Open)
            .filter(|t| {
                t.dependencies
                    .iter()
                    .any(|dep| dep.target_key() == Some(key))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
