//! The ticket surface's reducer arms: dispatching Effort discovery — the
//! local probes and the GitHub map reads — building the commands a refresh
//! asks for, and handling the surface's keys.
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
        ticket_list::{DiscoveryUnit, RefreshNotice, RefreshScope, RefreshTarget},
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
