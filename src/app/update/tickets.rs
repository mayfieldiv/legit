//! The ticket surface's reducer arms: dispatching Effort discovery — the
//! local probes and the GitHub map reads — and handling the surface's keys.
//! Split out of `update` the way `refresh` is, so the reducer stays a
//! dispatcher and the ticket story reads in one place — `super::apply`
//! delegates here.
// TODO(#132): `r`/`R`. TODO(#133): `h`/`l`, `J`/`K`, `m`, `p`, `y`, wheel
// ticks to the queue viewport.

use ratatui::crossterm::event::KeyCode;

use crate::{
    app::{
        cmd::Cmd,
        model::{Model, ViewMode},
        ticket_list::DiscoveryUnit,
    },
    auth::AuthToken,
    repo_slug::RepoSlug,
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
