//! The ticket surface's reducer arms: dispatching Effort discovery — the
//! local probes and the GitHub map reads — building the commands a refresh
//! asks for, and handling the surface's keys.
//! Split out of `update` the way `refresh` is, so the reducer stays a
//! dispatcher and the ticket story reads in one place — `super::apply`
//! delegates here.

use chrono::{DateTime, Utc};

#[cfg(test)]
mod tests;
use ratatui::crossterm::event::KeyCode;

use crate::{
    app::{
        cmd::Cmd,
        list_cursor::Direction,
        model::{Model, StatusKind, ViewMode},
        ticket_list::{DiscoveryUnit, RefreshNotice, RefreshScope, RefreshTarget, RepoScope},
    },
    auth::AuthToken,
    canonical_path::CanonicalPathBuf,
    config::RepoIdentity,
    repo_slug::RepoSlug,
    ticket::EffortRead,
};

/// Dispatch local Effort discovery once config and repo detection have both
/// settled: one probe per Tracked Repo with a Main Worktree (a slug-only repo
/// has no filesystem to probe), plus the cwd walk — attributed to the detected
/// repo, which is why the gate waits on detection. Auth is not a prerequisite:
/// nothing here touches the network, so `t` can land on data before GitHub
/// answers. Units already in flight or loaded are skipped, so a `R`-driven
/// config reload re-probes only new or failed units (the `needs_listing`
/// idiom). Two config entries spelling one Main Worktree differently both
/// probe; the pool's Effort-key dedup collapses what they find.
pub(super) fn maybe_discover_local_efforts(model: &mut Model) -> Vec<Cmd> {
    if !model.config_loaded || !model.repo.is_settled() {
        return Vec::new();
    }
    let mut cmds = Vec::new();
    for repo in &model.config.repos {
        let Some(unit) = DiscoveryUnit::for_repo(repo) else {
            continue;
        };
        if !model.tickets.needs_discovery(&unit) {
            continue;
        }
        model.tickets.begin_discovery(unit.clone());
        cmds.push(Cmd::DiscoverRepoEfforts {
            unit,
            repo: repo.clone(),
        });
    }
    if model.tickets.needs_discovery(&DiscoveryUnit::Cwd) {
        model.tickets.begin_discovery(DiscoveryUnit::Cwd);
        cmds.push(Cmd::DiscoverCwdEfforts {
            detected: model.repo.repo().cloned(),
            config: model.config.clone(),
        });
    }
    cmds
}

/// One PR-capable Tracked Repo's map read, unless its unit is in flight or
/// loaded — so a `R`-driven config reload reads only new or failed repos. The
/// gate is the caller's (`super::maybe_fetch_github`): a map read is an HTTP
/// request, so unlike the local probes it waits on the token, and its unit is
/// a slug, which the detected cwd repo supplies even when it isn't configured.
pub(super) fn map_read_cmd(model: &mut Model, repo: &RepoSlug, token: &AuthToken) -> Option<Cmd> {
    let unit = DiscoveryUnit::GitHubRepo { slug: repo.clone() };
    if !model.tickets.needs_discovery(&unit) {
        return None;
    }
    model.tickets.begin_discovery(unit);
    Some(Cmd::ReadGitHubEfforts {
        repo: repo.clone(),
        token: token.clone(),
    })
}

/// Ticket-surface navigation never dispatches the PR selection's files fetch.
pub(super) fn handle_ticket_list_key(model: &mut Model, code: KeyCode) -> Vec<Cmd> {
    match code {
        KeyCode::PageDown => model
            .tickets
            .scroll_summary(Direction::Down, super::DETAIL_SCROLL_PAGE),
        KeyCode::PageUp => model
            .tickets
            .scroll_summary(Direction::Up, super::DETAIL_SCROLL_PAGE),
        KeyCode::Char('p' | 'y') => {
            if let Some(summary) = model.tickets.selected_summary() {
                let text = if code == KeyCode::Char('p') {
                    summary.handoff_prompt.clone()
                } else {
                    summary.location.clone()
                };
                super::set_status(model, StatusKind::Info, format!("Copying {text}"));
                return vec![Cmd::CopyToClipboard { text }];
            }
        }
        KeyCode::Char('h') | KeyCode::Left | KeyCode::Char('[') => super::step_tab(model, -1),
        KeyCode::Char('l') | KeyCode::Right | KeyCode::Char(']') => super::step_tab(model, 1),
        KeyCode::Char(c) if c.is_ascii_digit() => {
            super::jump_to_tab(model, (c as u8 - b'0') as usize)
        }
        KeyCode::Char('J') => model.tickets.step_effort(Direction::Down),
        KeyCode::Char('K') => model.tickets.step_effort(Direction::Up),
        KeyCode::Char('m') => model.tickets.cycle_mode(),
        KeyCode::Char('R') => return refresh_cmds(model, RefreshScope::View),
        KeyCode::Char('r') => return refresh_cmds(model, RefreshScope::Selected),
        KeyCode::Char('q') => model.should_quit = true,
        KeyCode::Char('t') => model.view_mode = ViewMode::List,
        // Cursor movement is network-silent (spec §5.1): the map read already
        // delivered everything the queue shows.
        KeyCode::Char('j') | KeyCode::Down => model.tickets.move_down(),
        KeyCode::Char('k') | KeyCode::Up => model.tickets.move_up(),
        _ => {}
    }
    Vec::new()
}

pub(super) fn sync_scope(model: &mut Model) {
    let scope = model.active_scope().map(|repo| {
        let mut discoveries: Vec<_> = model
            .config
            .repos
            .iter()
            .filter(|entry| entry.slug.as_ref() == Some(&repo))
            .filter_map(DiscoveryUnit::for_repo)
            .collect();
        if model.repo.repo() == Some(&repo) {
            discoveries.push(DiscoveryUnit::Cwd);
        }
        RepoScope { repo, discoveries }
    });
    model.tickets.set_repo_scope(scope);
}

pub(super) fn normalize_summary(model: &mut Model, now: DateTime<Utc>) {
    use crate::app::{ticket_list_layout, ticket_summary_layout};
    if !matches!(model.view_mode, ViewMode::TicketList) {
        return;
    }
    let max_scroll = ticket_list_layout::summary_width(model.terminal_width)
        .zip(model.tickets.selected_summary())
        .map_or(0, |(width, summary)| {
            ticket_summary_layout::content_lines(
                summary,
                usize::from(width),
                now,
                &crate::palette::DARK,
            )
            .len()
            .saturating_sub(ticket_list_layout::summary_rows(model.terminal_height))
        });
    model.tickets.clamp_summary(max_scroll);
}

/// Build each target's command from the Model's side of the seam — the
/// token, the config entry, the detected repo — and let the queue mark what
/// was sent. A target with no token or no config entry is dropped unmarked.
fn refresh_cmds(model: &mut Model, scope: RefreshScope) -> Vec<Cmd> {
    let Model {
        tickets,
        auth_token,
        config,
        repo: detection,
        ..
    } = model;
    tickets.begin_refresh(scope, |target| {
        Some(match target {
            RefreshTarget::Discovery(DiscoveryUnit::GitHubRepo { slug }) => {
                Cmd::ReadGitHubEfforts {
                    repo: slug.clone(),
                    token: auth_token.clone()?,
                }
            }
            RefreshTarget::Discovery(unit @ DiscoveryUnit::LocalRepo { .. }) => {
                Cmd::DiscoverRepoEfforts {
                    repo: config
                        .repos
                        .iter()
                        .find(|repo| DiscoveryUnit::for_repo(repo).as_ref() == Some(unit))?
                        .clone(),
                    unit: unit.clone(),
                }
            }
            RefreshTarget::Discovery(DiscoveryUnit::Cwd) => Cmd::DiscoverCwdEfforts {
                detected: detection.repo().cloned(),
                config: config.clone(),
            },
            RefreshTarget::Local { dir, repo } => Cmd::ReadLocalEffort {
                dir: dir.clone(),
                repo: repo.clone(),
            },
        })
    })
}

fn notify(model: &mut Model, notice: Option<RefreshNotice>) -> Vec<Cmd> {
    match notice {
        Some(RefreshNotice::Refreshed(count)) => super::set_status(
            model,
            StatusKind::Success,
            format!(
                "Refreshed {count} effort{}",
                if count == 1 { "" } else { "s" }
            ),
        ),
        Some(RefreshNotice::Failed(text)) => super::set_status(model, StatusKind::Error, text),
        None => Vec::new(),
    }
}

pub(super) fn effort_arrived(
    model: &mut Model,
    unit: DiscoveryUnit,
    repo: RepoIdentity,
    read: EffortRead,
    now: DateTime<Utc>,
) -> Vec<Cmd> {
    let notice = model.tickets.effort_arrived(&unit, repo, read, now);
    notify(model, notice)
}

pub(super) fn local_effort_read(
    model: &mut Model,
    dir: CanonicalPathBuf,
    repo: RepoIdentity,
    result: Result<EffortRead, String>,
    now: DateTime<Utc>,
) -> Vec<Cmd> {
    let notice = model.tickets.local_effort_read(dir, repo, result, now);
    notify(model, notice)
}

pub(super) fn discovery_finished(
    model: &mut Model,
    unit: DiscoveryUnit,
    incomplete: Option<String>,
    now: DateTime<Utc>,
) -> Vec<Cmd> {
    let notice = model.tickets.finish_discovery(unit, incomplete, now);
    notify(model, notice)
}

pub(super) fn discovery_failed(model: &mut Model, unit: DiscoveryUnit, error: String) -> Vec<Cmd> {
    let notice = model.tickets.fail_discovery(unit, error);
    notify(model, notice)
}
