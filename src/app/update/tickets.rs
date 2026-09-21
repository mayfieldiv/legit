//! The ticket surface's reducer arms: dispatching Effort discovery — the
//! local probes and the GitHub map reads — and handling the surface's keys.
//! Split out of `update` the way `refresh` is, so the reducer stays a
//! dispatcher and the ticket story reads in one place — `super::apply`
//! delegates here.
// TODO(#133): `h`/`l`, `J`/`K`, `m`, `p`, `y`, wheel
// ticks to the queue viewport.

use chrono::{DateTime, Utc};
use ratatui::crossterm::event::KeyCode;

use crate::{
    app::{
        cmd::Cmd,
        model::{Model, StatusKind, ViewMode},
        ticket_list::{DiscoveryUnit, FetchUnit, RefreshScope, RefreshTarget},
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

/// Handle one keypress on the ticket surface: the queue cursor and the
/// surface toggle back to the PR list. None of these keys move the PR
/// selection, so the list surface's files-fetch path never runs for them.
pub(super) fn handle_ticket_list_key(model: &mut Model, code: KeyCode) -> Vec<Cmd> {
    match code {
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

fn refresh_cmds(model: &mut Model, scope: RefreshScope) -> Vec<Cmd> {
    let mut targets = match scope {
        RefreshScope::View => model.tickets.all_fetches(),
        RefreshScope::Selected => model.tickets.selected_fetch().into_iter().collect(),
    };
    // Failed cards have no selectable tickets until the rail filter lands.
    // Both refresh keys must therefore retry them from the current queue.
    targets.extend(model.tickets.failed_fetches());
    targets
        .into_iter()
        .filter_map(|target| refresh_cmd(model, target))
        .collect()
}

fn refresh_cmd(model: &mut Model, target: RefreshTarget) -> Option<Cmd> {
    let unit = target.unit();
    if let FetchUnit::Discovery(discovery) = &unit
        && model.tickets.discovery_is_loading(discovery)
    {
        return None;
    }
    let cmd = match target {
        RefreshTarget::Discovery(unit) => match &unit {
            DiscoveryUnit::GitHubRepo { slug } => Cmd::ReadGitHubEfforts {
                repo: slug.clone(),
                token: model.auth_token.clone()?,
            },
            DiscoveryUnit::LocalRepo { .. } => Cmd::DiscoverRepoEfforts {
                repo: model
                    .config
                    .repos
                    .iter()
                    .find(|repo| DiscoveryUnit::for_repo(repo).as_ref() == Some(&unit))?
                    .clone(),
                unit,
            },
            DiscoveryUnit::Cwd => Cmd::DiscoverCwdEfforts {
                detected: model.repo.repo().cloned(),
                config: model.config.clone(),
            },
        },
        RefreshTarget::Local { dir, repo } => Cmd::ReadLocalEffort { dir, repo },
    };
    if !model.tickets.begin_refresh(unit.clone()) {
        return None;
    }
    if let FetchUnit::Discovery(unit) = unit {
        model.tickets.begin_discovery(unit);
    }
    Some(cmd)
}

fn finish_refresh(model: &mut Model, unit: &FetchUnit, succeeded: bool) -> Vec<Cmd> {
    match model.tickets.finish_refresh(unit, succeeded) {
        Some(count) => super::set_status(
            model,
            StatusKind::Success,
            format!(
                "Refreshed {count} effort{}",
                if count == 1 { "" } else { "s" }
            ),
        ),
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
    model.tickets.record_discovery_read(&unit, &read);
    let refreshing = model.tickets.is_refreshing(&FetchUnit::Discovery(unit));
    merge_effort(model, repo, read, now, refreshing)
}

fn merge_effort(
    model: &mut Model,
    repo: RepoIdentity,
    read: EffortRead,
    now: DateTime<Utc>,
    refreshing: bool,
) -> Vec<Cmd> {
    let error = match &read {
        EffortRead::Degraded {
            key, title, reason, ..
        } if refreshing || model.tickets.has_data(key) => Some(format!("{title}: {reason}")),
        _ => None,
    };
    model.tickets.merge_effort(repo, read, now);
    error.map_or_else(Vec::new, |error| {
        super::set_status(model, StatusKind::Error, error)
    })
}

pub(super) fn local_effort_read(
    model: &mut Model,
    dir: CanonicalPathBuf,
    repo: RepoIdentity,
    result: Result<EffortRead, String>,
    now: DateTime<Utc>,
) -> Vec<Cmd> {
    let unit = FetchUnit::Local { dir };
    match result {
        Ok(read) => {
            let succeeded = matches!(read, EffortRead::Ready(_));
            let mut cmds = merge_effort(model, repo, read, now, true);
            cmds.extend(finish_refresh(model, &unit, succeeded));
            cmds
        }
        Err(error) => {
            finish_refresh(model, &unit, false);
            super::set_status(model, StatusKind::Error, error)
        }
    }
}

pub(super) fn discovery_finished(
    model: &mut Model,
    unit: DiscoveryUnit,
    incomplete: Option<String>,
    now: DateTime<Utc>,
) -> Vec<Cmd> {
    model
        .tickets
        .finish_discovery(unit.clone(), incomplete, now);
    finish_refresh(model, &FetchUnit::Discovery(unit), true)
}

pub(super) fn discovery_failed(model: &mut Model, unit: DiscoveryUnit, error: String) -> Vec<Cmd> {
    let fetch = FetchUnit::Discovery(unit.clone());
    let refreshing = model.tickets.is_refreshing(&fetch);
    finish_refresh(model, &fetch, false);
    let message = format!("{}: {error}", unit.label());
    model.tickets.fail_discovery(unit, error);
    if refreshing {
        super::set_status(model, StatusKind::Error, message)
    } else {
        Vec::new()
    }
}
