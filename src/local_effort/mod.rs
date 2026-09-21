//! Local wayfinder Effort discovery and dialect parsing — the ticket
//! surface's one local-filesystem interface (spec §2.2 + §3), the sibling of
//! `github::wayfinder`. An Effort directory parses into an [`EffortRead`]:
//! fully normalized or visibly degraded, never silently partial. Which file
//! marks an Effort, the two ticket dialects, and the normalization rules are
//! all implementation: this file decides *where* Efforts live (worktrees,
//! Wayfinder Roots, the cwd walk); [`format`] decides *what* one says.

mod format;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;

pub use self::format::read_effort_at;
use self::format::{find_map_file, probe_file_type};
use crate::{
    canonical_path::CanonicalPathBuf,
    config::{LegitConfig, RepoConfig, RepoIdentity, resolve_config_path},
    repo_slug::RepoSlug,
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

/// Discover and parse every local Effort visible from the working directory,
/// with the Tracked Repo they are attributed to: walk cwd → its git toplevel,
/// probing each level (which finds nested monorepo roots like
/// `apps/mac-agent/docs/wayfinder/`); a cwd outside any git repo is probed
/// alone. `detected` is the cwd's GitHub repo as repo detection found it —
/// passed in rather than re-detected, so the walk and `Cmd::DetectRepo`
/// can't answer differently for one cwd.
///
/// Attribution: a configured entry the cwd repo matches — by slug or by
/// repository membership of its Main Worktree — leads, so this probe and
/// that repo's own probe (which publish the same Effort keys) can't
/// disagree; the detected slug is only the fallback for an unconfigured
/// cwd repo — spec §2.1 keys a slug-less repo by path even when its remote
/// is on GitHub; and a cwd nobody configured or detected is its toplevel.
/// A matched entry's `wayfinderRoots` win over the built-ins (spec §2.2).
pub fn discover_cwd_efforts(
    cwd: &Path,
    config: &LegitConfig,
    detected: Option<&RepoSlug>,
) -> anyhow::Result<(RepoIdentity, Vec<EffortRead>)> {
    let levels = cwd_walk_levels(cwd)?;
    let toplevel = levels.last().expect("the walk holds at least the cwd");
    let matched = configured_repos_for_cwd(config, detected, toplevel);
    let roots = matched
        .iter()
        .find_map(|repo| repo.wayfinder_roots.as_deref());
    let identity = matched
        .first()
        .and_then(|repo| repo.identity().ok())
        .or_else(|| detected.cloned().map(RepoIdentity::Slug))
        .unwrap_or_else(|| RepoIdentity::Path(toplevel.clone()));
    let reads = read_efforts_under(&levels, roots)?;
    Ok((identity, reads))
}

/// The directories the cwd walk probes: the canonical cwd up to and
/// including its git toplevel, or the cwd alone outside a repo. The
/// toplevel is last, so callers can read the repo boundary off the walk.
fn cwd_walk_levels(cwd: &Path) -> anyhow::Result<Vec<CanonicalPathBuf>> {
    let cwd = CanonicalPathBuf::canonicalize(cwd)
        .with_context(|| format!("canonicalizing cwd {}", cwd.display()))?;
    let toplevel = git_toplevel(&cwd)
        .and_then(|top| fs::canonicalize(top).ok())
        // A toplevel that isn't a cwd ancestor (exotic symlink layouts):
        // there is no walk between them, so probe just the cwd.
        .filter(|top| cwd.starts_with(top))
        .unwrap_or_else(|| cwd.to_path_buf());
    Ok(cwd
        .ancestors()
        .take_while(|level| level.starts_with(&toplevel))
        .collect())
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

/// The `repos` entries the cwd repo is, in config order. Matching takes
/// either evidence of "same repo": the entry's slug equals the cwd's
/// `detected` origin-remote slug, or the entry's Main Worktree belongs to
/// the same repository as the toplevel (see [`repository_identity`] — so a
/// cwd in one of the repo's linked worktrees matches too, which the repo's
/// own probe fans out to and attributes to the entry). Deliberately looser
/// than [`RepoIdentity`], which answers dedup with one key — a slugged entry
/// whose clone is the toplevel is still the cwd repo even with the remote
/// missing or renamed. A match failure of any kind — no remote, an
/// unresolvable configured path — just means "not the cwd repo", never an
/// error. Several entries can match (two spellings of one Main Worktree
/// both probe); the caller picks per field.
fn configured_repos_for_cwd<'a>(
    config: &'a LegitConfig,
    detected: Option<&RepoSlug>,
    toplevel: &Path,
) -> Vec<&'a RepoConfig> {
    // One subprocess, and only when an entry needs it. Ambient env like
    // `git_toplevel`: this is the user's real cwd repo.
    let cwd_repository = config
        .repos
        .iter()
        .any(|repo| repo.main_worktree_path.is_some())
        .then(|| repository_identity(toplevel, GitEnv::Ambient))
        .flatten();
    config
        .repos
        .iter()
        .filter(|repo| {
            let slug_matches =
                detected.is_some_and(|detected| repo.slug.as_ref() == Some(detected));
            let repository_matches = match (&repo.main_worktree_path, &cwd_repository) {
                (Some(path), Some(cwd_repository)) => resolve_config_path(path)
                    .ok()
                    .and_then(|path| repository_identity(&path, GitEnv::Scrubbed))
                    .is_some_and(|repository| repository == *cwd_repository),
                _ => false,
            };
            slug_matches || repository_matches
        })
        .collect()
}

/// What identifies the repository `dir` belongs to: the canonical git common
/// dir, which every worktree of one repository shares (the Main Worktree's
/// `.git`, a linked worktree's `.git` file pointing back into it); outside
/// git, the canonical directory itself. Canonical either way, so spelling
/// differences and symlinks can't defeat the comparison. `None` when `dir`
/// can't be resolved at all.
fn repository_identity(dir: &Path, env: GitEnv) -> Option<CanonicalPathBuf> {
    let mut command = git_command(env);
    command
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--git-common-dir"]);
    let repository = match run_command("git rev-parse --git-common-dir", &mut command) {
        // Relative to `dir` inside the Main Worktree (`.git`), absolute from
        // a linked one; `join` handles both.
        Ok(stdout) => dir.join(stdout.trim()),
        Err(_) => dir.to_owned(),
    };
    CanonicalPathBuf::canonicalize(repository).ok()
}

/// The working trees a repo's Wayfinder Roots resolve against: every linked
/// worktree of the Main Worktree that still exists and isn't prunable, or
/// the base directory alone when it isn't a git repo at all. Only a missing
/// worktree skips — an unprobeable one errs (§5.5: skip covers exactly
/// missing/prunable, never an unreadable path).
fn worktree_bases(main: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let probe = |path: &Path| probe_file_type(path).map_err(anyhow::Error::msg);
    anyhow::ensure!(
        probe(main)?.is_some_and(|ty| ty.is_dir()),
        "main worktree {} does not exist",
        main.display()
    );
    if !is_git_worktree(main) {
        return Ok(vec![main.to_owned()]);
    }
    let mut bases = Vec::new();
    for entry in list_worktrees(main)? {
        if entry.prunable.is_some() || entry.bare {
            continue;
        }
        let path = PathBuf::from(entry.path);
        if probe(&path)?.is_some_and(|ty| ty.is_dir()) {
            bases.push(path);
        }
    }
    Ok(bases)
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
    bases: &[impl AsRef<Path>],
    roots: Option<&[String]>,
) -> anyhow::Result<Vec<EffortRead>> {
    let roots: Vec<&str> = roots.map_or_else(
        || BUILT_IN_ROOTS.to_vec(),
        |roots| roots.iter().map(String::as_str).collect(),
    );

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
                    effort_dirs.extend(probe_root(&base.as_ref().join(root))?);
                }
            }
        }
    }

    let mut seen = HashSet::new();
    let mut reads = Vec::new();
    for dir in effort_dirs {
        let identity = CanonicalPathBuf::canonicalize(&dir)
            .with_context(|| format!("canonicalizing effort dir {}", dir.display()))?;
        if seen.insert(identity.clone()) {
            reads.push(read_effort_at(identity));
        }
    }
    Ok(reads)
}

/// Probe one Wayfinder Root for Effort directories. A root either *is* a
/// single Effort (a map file directly inside) or *contains* Effort
/// subdirectories — both shapes exist in the wild (spec §2.2). A missing or
/// effort-less root probes empty; an unreadable one — its own metadata,
/// its listing, or an entry's — errs, surfaced per repo by discovery (§5.5:
/// never silently omit an Effort).
fn probe_root(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let probe = |path: &Path| probe_file_type(path).map_err(anyhow::Error::msg);
    if !probe(root)?.is_some_and(|ty| ty.is_dir()) {
        return Ok(Vec::new());
    }
    if find_map_file(root).map_err(anyhow::Error::msg)?.is_some() {
        return Ok(vec![root.to_owned()]);
    }
    let entries = fs::read_dir(root).with_context(|| format!("reading root {}", root.display()))?;
    let mut efforts = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("reading root {}", root.display()))?;
        let path = entry.path();
        if probe(&path)?.is_some_and(|ty| ty.is_dir())
            && find_map_file(&path).map_err(anyhow::Error::msg)?.is_some()
        {
            efforts.push(path);
        }
    }
    efforts.sort();
    Ok(efforts)
}
