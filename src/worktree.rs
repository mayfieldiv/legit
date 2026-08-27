use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};

use crate::{
    config::{LegitConfig, RepoConfig, home_dir, resolve_config_path},
    github::rest::PR,
    repo_slug::RepoSlug,
    subprocess::{GitEnv, HardenedCommand, gh_command, git_command, run_command},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: String,
    pub head: String,
    pub branch_ref: Option<String>,
    pub branch_name: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub locked: Option<String>,
    pub prunable: Option<String>,
}

pub fn parse_worktree_list(stdout: &str) -> Vec<WorktreeEntry> {
    let mut entries = Vec::new();

    for record in stdout.split("\n\n") {
        let lines: Vec<&str> = record.lines().filter(|line| !line.is_empty()).collect();
        if lines.is_empty() {
            continue;
        }

        let mut path = None;
        let mut head = String::new();
        let mut branch_ref = None;
        let mut detached = false;
        let mut bare = false;
        let mut locked = None;
        let mut prunable = None;

        for line in lines {
            if let Some(value) = line.strip_prefix("worktree ") {
                path = Some(value.to_owned());
            } else if let Some(value) = line.strip_prefix("HEAD ") {
                head = value.to_owned();
            } else if let Some(value) = line.strip_prefix("branch ") {
                branch_ref = Some(value.to_owned());
            } else if line == "detached" {
                detached = true;
            } else if line == "bare" {
                bare = true;
            } else if line == "locked" {
                locked = Some(String::new());
            } else if let Some(value) = line.strip_prefix("locked ") {
                locked = Some(value.to_owned());
            } else if line == "prunable" {
                prunable = Some(String::new());
            } else if let Some(value) = line.strip_prefix("prunable ") {
                prunable = Some(value.to_owned());
            }
        }

        let Some(path) = path else {
            continue;
        };
        let branch_name = branch_ref.as_deref().map(|branch| {
            branch
                .strip_prefix("refs/heads/")
                .unwrap_or(branch)
                .to_owned()
        });

        entries.push(WorktreeEntry {
            path,
            head,
            branch_ref,
            branch_name,
            detached,
            bare,
            locked,
            prunable,
        });
    }

    entries
}

pub fn sanitize_branch_for_path(branch: &str) -> String {
    let mut sanitized = String::new();
    let mut previous_dash = false;

    for ch in branch.chars() {
        let next = if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            ch
        } else {
            '-'
        };

        if next == '-' {
            if previous_dash {
                continue;
            }
            previous_dash = true;
        } else {
            previous_dash = false;
        }
        sanitized.push(next);
    }

    sanitized.trim_matches('-').chars().take(80).collect()
}

pub fn resolve_main_worktree(
    config: &LegitConfig,
    slug: &RepoSlug,
) -> anyhow::Result<Option<PathBuf>> {
    let Some(path) = repo_config(config, slug).and_then(|repo| repo.main_worktree_path.as_deref())
    else {
        return Ok(None);
    };
    resolve_config_path(path)
        .with_context(|| format!("failed to resolve mainWorktreePath for {slug}"))
        .map(Some)
}

pub fn resolve_worktree_root(config: &LegitConfig, slug: &RepoSlug) -> anyhow::Result<PathBuf> {
    if let Some(root) = repo_config(config, slug).and_then(|repo| repo.worktree_root.as_deref()) {
        return resolve_config_path(root)
            .with_context(|| format!("failed to resolve worktreeRoot for {slug}"));
    }
    if let Some(root) = config.worktree_root.as_deref() {
        return resolve_config_path(root)
            .with_context(|| "failed to resolve worktreeRoot".to_owned())
            .map(|root| root.join(slug.as_str()));
    }
    Ok(home_dir()?.join(".legit/worktrees").join(slug.as_str()))
}

pub fn resolve_worktree_path(
    config: &LegitConfig,
    slug: &RepoSlug,
    pr_number: u64,
    head_ref: &str,
) -> anyhow::Result<PathBuf> {
    Ok(resolve_worktree_root(config, slug)?.join(format!(
        "{pr_number}-{}",
        sanitize_branch_for_path(head_ref)
    )))
}

/// The inverse of `resolve_worktree_path`'s leaf naming: extract the PR number
/// from a `{N}-{branch}` directory name. `None` when the name doesn't have
/// that shape. Kept beside the format so the two evolve together.
pub fn parse_worktree_leaf(leaf: &str) -> Option<u64> {
    let (num_str, rest) = leaf.split_once('-')?;
    if rest.is_empty() || !num_str.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    num_str.parse().ok()
}

pub fn expected_branch_for_pr(pr: &PR, repo_owner: &str) -> String {
    if pr.head_repository_owner.is_empty() || pr.head_repository_owner == repo_owner {
        pr.head_ref.clone()
    } else {
        format!("{}-{}", pr.head_repository_owner, pr.head_ref)
    }
}

pub fn match_worktree<'a>(
    entries: &'a [WorktreeEntry],
    expected_branch: &str,
    expected_path: &str,
) -> Option<&'a WorktreeEntry> {
    entries
        .iter()
        .find(|entry| entry.branch_name.as_deref() == Some(expected_branch))
        .or_else(|| entries.iter().find(|entry| entry.path == expected_path))
}

// Every git/gh invocation in this module is scoped to a path it is given
// (`-C`/`current_dir`), never the ambient cwd repo, so all of them run under
// `GitEnv::Scrubbed` — an inherited `GIT_DIR` could otherwise redirect them
// onto the repository a hook is running inside.

pub fn list_worktrees(main_worktree: &Path) -> anyhow::Result<Vec<WorktreeEntry>> {
    ensure_main_worktree(main_worktree)?;
    let mut command = git_command(GitEnv::Scrubbed);
    command
        .arg("-C")
        .arg(main_worktree)
        .args(["worktree", "list", "--porcelain"]);
    let stdout = run_command("git worktree list", &mut command)?;
    Ok(parse_worktree_list(&stdout))
}

pub fn create_worktree_for_pr(
    main_worktree: &Path,
    target_path: &Path,
    pr_number: u64,
) -> anyhow::Result<()> {
    create_worktree_for_pr_with_checkout(main_worktree, target_path, pr_number, checkout_pr)
}

fn create_worktree_for_pr_with_checkout(
    main_worktree: &Path,
    target_path: &Path,
    pr_number: u64,
    checkout: impl FnOnce(&Path, u64) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    ensure_main_worktree(main_worktree)?;
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let mut git = git_command(GitEnv::Scrubbed);
    // --no-checkout: the PR checkout below immediately replaces the contents
    // anyway, and it keeps `post-checkout` hooks out of this step — git
    // propagates a hook's non-zero exit status, and a hook failure here would
    // strand a registered worktree with no cleanup. The hook fires once,
    // during `checkout`, whose failure path removes the partial worktree.
    git.arg("-C")
        .arg(main_worktree)
        .args(["worktree", "add", "-d", "--no-checkout"])
        .arg(target_path);
    run_command("git worktree add", &mut git)?;

    checkout(target_path, pr_number).map_err(|checkout_error| {
        match remove_worktree(main_worktree, target_path) {
            Ok(()) => checkout_error,
            Err(cleanup_error) => checkout_error.context(format!(
                "failed to clean up partial worktree {}: {cleanup_error:#}",
                target_path.display()
            )),
        }
    })?;

    Ok(())
}

fn checkout_pr(target_path: &Path, pr_number: u64) -> anyhow::Result<()> {
    run_command(
        "gh pr checkout",
        &mut checkout_pr_command(target_path, pr_number),
    )
    .map(|_| ())
}

/// The `gh pr checkout` invocation for [`checkout_pr`], separate from the spawn
/// so a test can pin its construction. Built on [`gh_command`] (prompt/token
/// hardening plus the scrubbed git env; stdin/session hardening comes from
/// [`run_command`]).
fn checkout_pr_command(target_path: &Path, pr_number: u64) -> HardenedCommand {
    let mut gh = gh_command(GitEnv::Scrubbed);
    gh.args(["pr", "checkout", &pr_number.to_string()])
        .current_dir(target_path);
    gh
}

fn remove_worktree(main_worktree: &Path, target_path: &Path) -> anyhow::Result<()> {
    let mut command = git_command(GitEnv::Scrubbed);
    command
        .arg("-C")
        .arg(main_worktree)
        .args(["worktree", "remove", "--force"])
        .arg(target_path);
    run_command("git worktree remove", &mut command).map(|_| ())
}

fn repo_config<'a>(config: &'a LegitConfig, slug: &RepoSlug) -> Option<&'a RepoConfig> {
    config
        .repos
        .iter()
        .find(|repo| repo.slug.as_deref().is_some_and(|s| *slug == s))
}

fn ensure_main_worktree(main_worktree: &Path) -> anyhow::Result<()> {
    if !main_worktree.exists() {
        bail!("main worktree {} does not exist", main_worktree.display());
    }
    if !main_worktree.is_dir() {
        bail!(
            "main worktree {} is not a directory",
            main_worktree.display()
        );
    }

    let mut command = git_command(GitEnv::Scrubbed);
    command
        .arg("-C")
        .arg(main_worktree)
        .args(["rev-parse", "--git-dir"]);
    run_command("git rev-parse --git-dir", &mut command).with_context(|| {
        format!(
            "main worktree {} is not a git repo",
            main_worktree.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests;
