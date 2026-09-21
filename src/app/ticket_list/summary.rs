use super::{QueueRow, TicketList, TicketRow};
use crate::ticket::{Dependency, EffortKey, TicketKey, TicketState};

#[derive(Clone, Debug)]
pub struct TicketSummary {
    pub row: TicketRow,
    pub effort: String,
    pub destination: Option<String>,
    pub waits_on: Vec<DependencySummary>,
    pub blocks: Vec<DependencySummary>,
    pub handoff_prompt: String,
    pub location: String,
}

#[derive(Clone, Debug)]
pub enum DependencySummary {
    Known {
        display_ref: String,
        title: String,
        state: TicketState,
        qualifier: Option<String>,
    },
    Unknown(String),
}

impl TicketList {
    pub fn selected_summary(&self) -> Option<&TicketSummary> {
        self.summary.as_ref()
    }

    pub(super) fn refresh_summary(&mut self) {
        self.summary = self.derive_summary();
    }

    fn derive_summary(&self) -> Option<TicketSummary> {
        let key = self.selected_ticket()?;
        let row = self.rows.iter().find_map(|row| match row {
            QueueRow::Ticket(row) if &row.key == key => Some(row),
            _ => None,
        })?;
        let effort = self
            .efforts
            .iter()
            .filter_map(|entry| entry.effort.as_ref())
            .find(|effort| effort.ticket(key).is_some())?;
        let map_ref = match &effort.key {
            EffortKey::GitHub {
                repo_slug,
                map_number,
            } => format!("https://github.com/{repo_slug}/issues/{map_number}"),
            EffortKey::Local { dir } => dir.display().to_string(),
        };
        let location = location(key);
        Some(TicketSummary {
            row: row.clone(),
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
                            .map(|entry| format!("{} · {}", entry.card.repo, entry.card.title))
                            .or_else(|| {
                                Some(match &target.key {
                                    TicketKey::GitHub { repo_slug, .. } => repo_slug.to_string(),
                                    TicketKey::Local { path } => path.display().to_string(),
                                })
                            }),
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
                                .then(|| format!("{} · {}", entry.card.repo, entry.card.title)),
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
