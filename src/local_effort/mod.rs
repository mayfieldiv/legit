//! Local wayfinder Effort discovery and dialect parsing — the ticket
//! surface's one local-filesystem interface (spec §2.2 + §3), the sibling of
//! `github::wayfinder`. An Effort directory parses into an [`EffortRead`]:
//! fully normalized or visibly degraded, never silently partial. Which file
//! marks an Effort, the two ticket dialects, and the normalization rules are
//! all implementation: this file decides *where* Efforts live (worktrees,
//! Wayfinder Roots, the cwd walk); [`format`] decides *what* one says.

// TODO(#120): remove once the fetch layer dispatches local probes.
#![allow(dead_code)]

mod format;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;

use self::format::{find_map_file, read_effort_at};
use crate::{
    canonical_path::CanonicalPathBuf,
    config::{RepoConfig, resolve_config_path},
    subprocess::{GitEnv, git_command, run_command},
    ticket::EffortRead,
    worktree::list_worktrees,
};

/// The built-in Wayfinder Root probe list. A repo's configured
/// `wayfinderRoots` *replaces* it for that repo, never extends it.
const BUILT_IN_ROOTS: &[&str] = &["docs/wayfinder", ".wayfinder", ".scratch"];

/// Discover and parse every local Effort of one Tracked Repo: start at its
/// Main Worktree, fan out across `git worktree list` (skipping missing and
/// prunable entries; a non-git base is probed alone), and probe each base's
/// Wayfinder Roots (spec §2.2). A repo with no `mainWorktreePath` has no
/// filesystem to probe and discovers nothing. Errs on a missing Main
/// Worktree or unreadable roots — the per-repo probe failure the effort
/// surface must render, never silently map to zero Efforts (§5.5).
pub fn discover_repo_efforts(repo: &RepoConfig) -> anyhow::Result<Vec<EffortRead>> {
    let Some(path) = repo.main_worktree_path.as_deref() else {
        return Ok(Vec::new());
    };
    let main = resolve_config_path(path)?;
    let bases = worktree_bases(&main)?;
    read_efforts_under(&bases, repo.wayfinder_roots.as_deref())
}

/// Discover and parse every local Effort visible from the working directory:
/// walk cwd → its git toplevel, probing each level (which finds nested
/// monorepo roots like `apps/mac-agent/docs/wayfinder/`); a cwd outside any
/// git repo is probed alone. When the cwd repo matches a configured entry
/// carrying `wayfinderRoots` — by slug (the origin remote) or by canonical
/// Main Worktree identity — the explicit roots win over the built-ins
/// (spec §2.2).
pub fn discover_cwd_efforts(
    cwd: &Path,
    config: &crate::config::LegitConfig,
) -> anyhow::Result<Vec<EffortRead>> {
    let levels = cwd_walk_levels(cwd)?;
    let toplevel = levels.last().expect("the walk holds at least the cwd");
    let roots = configured_roots_for_cwd(config, cwd, toplevel);
    read_efforts_under(&levels, roots)
}

/// The directories the cwd walk probes: the canonical cwd up to and
/// including its git toplevel, or the cwd alone outside a repo. The
/// toplevel is last, so callers can read the repo boundary off the walk.
fn cwd_walk_levels(cwd: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let cwd =
        fs::canonicalize(cwd).with_context(|| format!("canonicalizing cwd {}", cwd.display()))?;
    let Some(toplevel) = git_toplevel(&cwd).and_then(|top| fs::canonicalize(top).ok()) else {
        return Ok(vec![cwd]);
    };
    if !cwd.starts_with(&toplevel) {
        // A toplevel that isn't a cwd ancestor (exotic symlink layouts):
        // there is no walk between them, so probe just the cwd.
        return Ok(vec![cwd]);
    }
    let mut levels = Vec::new();
    let mut level = cwd.as_path();
    loop {
        levels.push(level.to_owned());
        if level == toplevel {
            return Ok(levels);
        }
        level = level
            .parent()
            .expect("starts_with guarantees the toplevel is an ancestor");
    }
}

/// The git toplevel of the repo holding `cwd`, `None` outside any repo.
/// Ambient env: like `detect_repo`, this deliberately reads the user's real
/// cwd repo.
fn git_toplevel(cwd: &Path) -> Option<PathBuf> {
    let mut command = git_command(GitEnv::Ambient);
    command
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd);
    run_command("git rev-parse --show-toplevel", &mut command)
        .ok()
        .map(|stdout| PathBuf::from(stdout.trim()))
}

/// The configured `wayfinderRoots` that replace the built-ins for the cwd
/// walk, when the cwd repo matches a `repos` entry carrying them. Matching
/// takes either evidence of "same repo": the entry's slug equals the cwd's
/// origin-remote slug, or the entry's Main Worktree names the toplevel
/// (canonical identity, so spelling differences and symlinks can't defeat
/// it). Deliberately looser than [`crate::config::RepoIdentity`], which answers dedup with
/// one key — a slugged entry whose clone is the toplevel is still the cwd
/// repo even with the remote missing or renamed. A match failure of any
/// kind — no remote, an unresolvable configured path — just means "not the
/// cwd repo", never an error: the walk falls back to the built-ins.
fn configured_roots_for_cwd<'a>(
    config: &'a crate::config::LegitConfig,
    cwd: &Path,
    toplevel: &Path,
) -> Option<&'a [String]> {
    let candidates: Vec<&RepoConfig> = config
        .repos
        .iter()
        .filter(|repo| repo.wayfinder_roots.is_some())
        .collect();
    if candidates.is_empty() {
        return None;
    }
    // One subprocess, and only when a slugged candidate needs it.
    let cwd_slug = candidates
        .iter()
        .any(|repo| repo.slug.is_some())
        .then(|| crate::git_remote::detect_repo(cwd).ok())
        .flatten();
    let toplevel = CanonicalPathBuf::canonicalize(toplevel).ok();
    candidates
        .into_iter()
        .find(|repo| {
            let slug_matches = match (&repo.slug, &cwd_slug) {
                (Some(entry), Some(cwd_slug)) => entry == cwd_slug,
                _ => false,
            };
            let path_matches = match (&repo.main_worktree_path, &toplevel) {
                (Some(path), Some(toplevel)) => resolve_config_path(path)
                    .ok()
                    .and_then(|path| CanonicalPathBuf::canonicalize(path).ok())
                    .is_some_and(|path| path == *toplevel),
                _ => false,
            };
            slug_matches || path_matches
        })
        .and_then(|repo| repo.wayfinder_roots.as_deref())
}

/// The working trees a repo's Wayfinder Roots resolve against: every linked
/// worktree of the Main Worktree that still exists and isn't prunable, or
/// the base directory alone when it isn't a git repo at all.
fn worktree_bases(main: &Path) -> anyhow::Result<Vec<PathBuf>> {
    anyhow::ensure!(
        main.is_dir(),
        "main worktree {} does not exist",
        main.display()
    );
    if !is_git_worktree(main) {
        return Ok(vec![main.to_owned()]);
    }
    Ok(list_worktrees(main)?
        .into_iter()
        .filter(|entry| entry.prunable.is_none() && !entry.bare)
        .map(|entry| PathBuf::from(entry.path))
        .filter(|path| path.is_dir())
        .collect())
}

/// Whether `dir` is inside a git repository — decides worktree fan-out vs
/// probing the base alone. Scrubbed env like every path-scoped git call: an
/// inherited `GIT_DIR` could otherwise answer for a different repo.
fn is_git_worktree(dir: &Path) -> bool {
    let mut command = git_command(GitEnv::Scrubbed);
    command.arg("-C").arg(dir).args(["rev-parse", "--git-dir"]);
    run_command("git rev-parse --git-dir", &mut command).is_ok()
}

/// Probe `roots` (configured, or the built-in list) under every base and
/// parse each Effort directory found — once: the same directory reached from
/// several bases or roots (symlinks, an absolute root shared by worktrees)
/// dedups on its canonical identity.
fn read_efforts_under(
    bases: &[PathBuf],
    roots: Option<&[String]>,
) -> anyhow::Result<Vec<EffortRead>> {
    let built_in: Vec<String> = BUILT_IN_ROOTS
        .iter()
        .map(|root| (*root).to_owned())
        .collect();
    let roots = roots.unwrap_or(&built_in);

    let mut effort_dirs = Vec::new();
    for root in roots {
        // `~`-prefixed and absolute roots stand alone; relative ones resolve
        // against each probed worktree base (never the cwd, which is what
        // `resolve_config_path` would do with them).
        let expanded = if root == "~" || root.starts_with("~/") {
            Some(resolve_config_path(root)?)
        } else {
            let path = PathBuf::from(root);
            path.is_absolute().then_some(path)
        };
        match expanded {
            Some(absolute) => effort_dirs.extend(probe_root(&absolute)?),
            None => {
                for base in bases {
                    effort_dirs.extend(probe_root(&base.join(root))?);
                }
            }
        }
    }

    let mut seen = HashSet::new();
    let mut reads = Vec::new();
    for dir in effort_dirs {
        let identity = CanonicalPathBuf::canonicalize(&dir)
            .with_context(|| format!("canonicalizing effort dir {}", dir.display()))?;
        if seen.contains(&identity) {
            continue;
        }
        reads.push(read_effort_at(identity.clone()));
        seen.insert(identity);
    }
    Ok(reads)
}

/// Probe one Wayfinder Root for Effort directories. A root either *is* a
/// single Effort (a map file directly inside) or *contains* Effort
/// subdirectories — both shapes exist in the wild (spec §2.2). A missing or
/// effort-less root probes empty; an unreadable one errs, surfaced per repo
/// by discovery (§5.5).
fn probe_root(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    if find_map_file(root).map_err(anyhow::Error::msg)?.is_some() {
        return Ok(vec![root.to_owned()]);
    }
    let entries = fs::read_dir(root).with_context(|| format!("reading root {}", root.display()))?;
    let mut efforts = Vec::new();
    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        if path.is_dir() && find_map_file(&path).map_err(anyhow::Error::msg)?.is_some() {
            efforts.push(path);
        }
    }
    efforts.sort();
    Ok(efforts)
}
