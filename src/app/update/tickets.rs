//! The ticket surface's reducer arms: dispatching local Effort discovery and
//! handling the surface's keys. Split out of `update` the way `refresh` is,
//! so the reducer stays a dispatcher and the ticket story reads in one place
//! — `super::apply` delegates here.
// TODO(#132): `r`/`R`. TODO(#133): `h`/`l`, `J`/`K`, `m`, `p`, `y`, wheel
// ticks to the queue viewport.

use ratatui::crossterm::event::KeyCode;

use crate::app::{
    cmd::Cmd,
    model::{Model, ViewMode},
    ticket_list::DiscoveryUnit,
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
        let Some(main_worktree_path) = repo.main_worktree_path.clone() else {
            continue;
        };
        // TODO: make `RepoConfig` an enum (slugged / local-only) so
        // `display_name` is total and this expect goes away.
        let name = repo
            .display_name()
            .expect("a repo with a mainWorktreePath has a display name");
        let unit = DiscoveryUnit::LocalRepo {
            name,
            main_worktree_path,
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
