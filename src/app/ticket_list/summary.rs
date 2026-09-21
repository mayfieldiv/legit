use super::{EffortEntry, QueueRow, TicketList, TicketRow};
use crate::app::list_cursor::Direction;
use crate::ticket::{Dependency, EffortKey, TicketKey, TicketState};
use chrono::{DateTime, Utc};

#[derive(Clone, Debug)]
pub struct TicketSummary {
    scroll: usize,
    pub row: TicketRow,
    pub fetched_at: Option<DateTime<Utc>>,
    pub effort: String,
    pub destination: Option<String>,
    pub waits_on: Vec<DependencySummary>,
    pub blocks: Vec<DependencySummary>,
    pub handoff_prompt: String,
    pub location: String,
}

impl TicketSummary {
    pub fn scroll(&self) -> usize {
        self.scroll
    }
}

#[derive(Clone, Debug)]
pub enum DependencySummary {
    Known {
        display_ref: String,
        title: String,
        state: TicketState,
        qualifier: Option<DependencyQualifier>,
    },
    Unknown(String),
}

#[derive(Clone, Debug)]
pub struct DependencyQualifier {
    pub text: String,
    pub repo: Option<String>,
}

impl DependencyQualifier {
    fn for_effort(entry: &EffortEntry) -> Self {
        Self {
            text: format!("{} · {}", entry.card.repo, entry.card.title),
            repo: Some(entry.card.repo.clone()),
        }
    }

    fn for_key(key: &TicketKey) -> Self {
        match key {
            TicketKey::GitHub { repo_slug, .. } => Self {
                text: repo_slug.to_string(),
                repo: Some(repo_slug.to_string()),
            },
            TicketKey::Local { path } => Self {
                text: path.display().to_string(),
                repo: None,
            },
        }
    }
}

impl TicketList {
    pub fn selected_summary(&self) -> Option<&TicketSummary> {
        self.summary.as_ref()
    }

    pub(super) fn refresh_summary(&mut self) {
        let mut summary = self.derive_summary();
        if let (Some(previous), Some(next)) = (&self.summary, &mut summary)
            && previous.row.key == next.row.key
        {
            next.scroll = previous.scroll;
        }
        self.summary = summary;
    }

    pub fn scroll_summary(&mut self, direction: Direction, lines: usize) {
        if let Some(summary) = &mut self.summary {
            summary.scroll = match direction {
                Direction::Down => summary.scroll.saturating_add(lines),
                Direction::Up => summary.scroll.saturating_sub(lines),
            };
        }
    }

    pub fn clamp_summary(&mut self, max_scroll: usize) {
        if let Some(summary) = &mut self.summary {
            summary.scroll = summary.scroll.min(max_scroll);
        }
    }

    fn derive_summary(&self) -> Option<TicketSummary> {
        let key = self.selected_ticket()?;
        let row = self.rows.iter().find_map(|row| match row {
            QueueRow::Ticket(row) if &row.key == key => Some(row),
            _ => None,
        })?;
        let (entry, effort) = self
            .efforts
            .iter()
            .filter_map(|entry| Some((entry, entry.effort.as_ref()?)))
            .find(|(_, effort)| effort.ticket(key).is_some())?;
        let map_ref = match &effort.key {
            EffortKey::GitHub {
                repo_slug,
                map_number,
            } => format!("https://github.com/{repo_slug}/issues/{map_number}"),
            EffortKey::Local { dir } => dir.display().to_string(),
        };
        let location = location(key);
        Some(TicketSummary {
            scroll: 0,
            row: row.clone(),
            fetched_at: entry.card.fetch.fetched_at,
            effort: effort.title.clone(),
            destination: effort.destination.clone(),
            waits_on: effort
                .ticket(key)?
                .dependencies
                .iter()
                .map(|dependency| match dependency {
                    Dependency::SameEffort(key) => match effort.ticket(key) {
                        Some(target) => DependencySummary::Known {
                            display_ref: key.display_ref(),
                            title: target.title.clone(),
                            state: target.state,
                            qualifier: None,
                        },
                        None => DependencySummary::Unknown(key.display_ref()),
                    },
                    Dependency::External(target) => DependencySummary::Known {
                        display_ref: target.key.display_ref(),
                        title: target
                            .title
                            .clone()
                            .unwrap_or_else(|| "title unavailable".to_owned()),
                        state: target.state,
                        qualifier: self
                            .efforts
                            .iter()
                            .find(|entry| {
                                entry
                                    .effort
                                    .as_ref()
                                    .is_some_and(|effort| effort.ticket(&target.key).is_some())
                            })
                            .map(DependencyQualifier::for_effort)
                            .or_else(|| Some(DependencyQualifier::for_key(&target.key))),
                    },
                    Dependency::Unknown { raw } => DependencySummary::Unknown(raw.clone()),
                })
                .collect(),
            blocks: self
                .efforts
                .iter()
                .filter_map(|entry| Some((entry, entry.effort.as_ref()?)))
                .flat_map(|(entry, source)| {
                    source
                        .tickets()
                        .filter(move |ticket| {
                            ticket.state == TicketState::Open
                                && ticket
                                    .dependencies
                                    .iter()
                                    .any(|dep| dep.target_key() == Some(key))
                        })
                        .map(move |ticket| DependencySummary::Known {
                            display_ref: ticket.key.display_ref(),
                            title: ticket.title.clone(),
                            state: ticket.state,
                            qualifier: (source.key != effort.key)
                                .then(|| DependencyQualifier::for_effort(entry)),
                        })
                })
                .collect(),
            handoff_prompt: format!("{} | /wayfinder {map_ref} - resolve {location}", row.title),
            location,
        })
    }
}

fn location(key: &TicketKey) -> String {
    match key {
        TicketKey::GitHub { repo_slug, number } => {
            format!("https://github.com/{repo_slug}/issues/{number}")
        }
        TicketKey::Local { path } => path.display().to_string(),
    }
}
