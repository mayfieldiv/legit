use std::{
    env,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, bail};

use crate::{
    config::{LegitConfig, RepoConfig},
    github::rest::PR,
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

pub fn resolve_source_clone(config: &LegitConfig, slug: &str) -> anyhow::Result<Option<PathBuf>> {
    let Some(path) = repo_config(config, slug).and_then(|repo| repo.source_clone.as_deref()) else {
        return Ok(None);
    };
    resolve_config_path(path)
        .with_context(|| format!("failed to resolve sourceClone for {slug}"))
        .map(Some)
}

pub fn resolve_worktree_root(config: &LegitConfig, slug: &str) -> anyhow::Result<PathBuf> {
    if let Some(root) = repo_config(config, slug).and_then(|repo| repo.worktree_root.as_deref()) {
        return resolve_config_path(root)
            .with_context(|| format!("failed to resolve worktreeRoot for {slug}"));
    }
    if let Some(root) = config.worktree_root.as_deref() {
        return resolve_config_path(root)
            .with_context(|| "failed to resolve worktreeRoot".to_owned())
            .map(|root| root.join(slug));
    }
    Ok(home_dir()?.join(".legit/worktrees").join(slug))
}

pub fn resolve_worktree_path(
    config: &LegitConfig,
    slug: &str,
    pr_number: u64,
    head_ref: &str,
) -> anyhow::Result<PathBuf> {
    Ok(resolve_worktree_root(config, slug)?.join(format!(
        "{pr_number}-{}",
        sanitize_branch_for_path(head_ref)
    )))
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

pub fn list_worktrees(source_clone: &Path) -> anyhow::Result<Vec<WorktreeEntry>> {
    ensure_source_clone(source_clone)?;
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(source_clone)
        .args(["worktree", "list", "--porcelain"]);
    let stdout = run_command("git worktree list", &mut command)?;
    Ok(parse_worktree_list(&stdout))
}

pub fn create_worktree_for_pr(
    source_clone: &Path,
    target_path: &Path,
    pr_number: u64,
) -> anyhow::Result<()> {
    ensure_source_clone(source_clone)?;
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let mut git = Command::new("git");
    git.arg("-C")
        .arg(source_clone)
        .args(["worktree", "add", "-d"])
        .arg(target_path);
    run_command("git worktree add", &mut git)?;

    let mut gh = Command::new("gh");
    gh.args(["pr", "checkout", &pr_number.to_string()])
        .current_dir(target_path)
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_TOKEN");
    run_command("gh pr checkout", &mut gh)?;

    Ok(())
}

fn repo_config<'a>(config: &'a LegitConfig, slug: &str) -> Option<&'a RepoConfig> {
    config
        .repos
        .iter()
        .find(|repo| repo.slug.eq_ignore_ascii_case(slug))
}

fn home_dir() -> anyhow::Result<PathBuf> {
    home_dir_from(env::var_os("HOME"))
}

fn home_dir_from(home: Option<OsString>) -> anyhow::Result<PathBuf> {
    home.filter(|home| !home.as_os_str().is_empty())
        .map(PathBuf::from)
        .context("HOME is not set")
}

fn resolve_config_path(path: &str) -> anyhow::Result<PathBuf> {
    resolve_config_path_with(path, home_dir, env::current_dir)
}

fn resolve_config_path_with(
    path: &str,
    home_dir: impl Fn() -> anyhow::Result<PathBuf>,
    current_dir: impl Fn() -> io::Result<PathBuf>,
) -> anyhow::Result<PathBuf> {
    let expanded = if path == "~" {
        home_dir()?
    } else if let Some(rest) = path.strip_prefix("~/") {
        home_dir()?.join(rest)
    } else {
        PathBuf::from(path)
    };

    if expanded.is_absolute() {
        Ok(expanded)
    } else {
        Ok(current_dir()
            .context("failed to resolve current directory")?
            .join(expanded))
    }
}

fn ensure_source_clone(source_clone: &Path) -> anyhow::Result<()> {
    if !source_clone.exists() {
        bail!("source clone {} does not exist", source_clone.display());
    }
    if !source_clone.is_dir() {
        bail!("source clone {} is not a directory", source_clone.display());
    }

    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(source_clone)
        .args(["rev-parse", "--git-dir"]);
    run_command("git rev-parse --git-dir", &mut command)
        .with_context(|| format!("source clone {} is not a git repo", source_clone.display()))?;
    Ok(())
}

fn run_command(label: &str, command: &mut Command) -> anyhow::Result<String> {
    let output = command
        .output()
        .with_context(|| format!("failed to run `{label}`"))?;

    if !output.status.success() {
        let stderr = stderr_tail(&output.stderr);
        if stderr.is_empty() {
            bail!("`{label}` exited with {}", output.status);
        }
        bail!("`{label}` failed: {stderr}");
    }

    String::from_utf8(output.stdout).with_context(|| format!("`{label}` returned non-utf8 output"))
}

fn stderr_tail(stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr).trim().to_owned();
    const MAX: usize = 1_000;
    if stderr.chars().count() <= MAX {
        return stderr;
    }
    let suffix: String = stderr
        .chars()
        .rev()
        .take(MAX.saturating_sub(1))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{suffix}")
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::{
        config::RepoConfig,
        github::rest::{PRState, PrKey},
    };

    fn sample_pr(head_ref: &str, head_owner: &str) -> PR {
        PR {
            number: 42,
            repo_slug: "acme/widgets".to_owned(),
            title: "patch".to_owned(),
            author: "octocat".to_owned(),
            created_at: chrono::Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap(),
            updated_at: chrono::Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap(),
            additions: 0,
            deletions: 0,
            is_draft: false,
            labels: Vec::new(),
            requested_reviewers: Vec::new(),
            assignees: Vec::new(),
            review_decision: String::new(),
            mergeable: "UNKNOWN".to_owned(),
            last_commit_date: None,
            head_commit_sha: None,
            review_status_loaded: false,
            head_ref: head_ref.to_owned(),
            base_ref: "main".to_owned(),
            head_repository_owner: head_owner.to_owned(),
            state: PRState::Open,
        }
    }

    #[test]
    fn parses_a_single_attached_worktree_on_a_branch() {
        let stdout = [
            "worktree /Users/me/src/widgets",
            "HEAD abc123def4567890abc123def4567890abc123de",
            "branch refs/heads/main",
            "",
        ]
        .join("\n");

        assert_eq!(
            parse_worktree_list(&stdout),
            vec![WorktreeEntry {
                path: "/Users/me/src/widgets".to_owned(),
                head: "abc123def4567890abc123def4567890abc123de".to_owned(),
                branch_ref: Some("refs/heads/main".to_owned()),
                branch_name: Some("main".to_owned()),
                detached: false,
                bare: false,
                locked: None,
                prunable: None,
            }]
        );
    }

    #[test]
    fn parses_multiple_worktrees_with_flags() {
        let stdout = [
            "worktree /Users/me/src/widgets",
            "bare",
            "",
            "worktree /Users/me/.legit/worktrees/acme/widgets/1-foo",
            "HEAD deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
            "branch refs/heads/foo",
            "",
            "worktree /tmp/detached-head",
            "HEAD aabbccddaabbccddaabbccddaabbccddaabbccdd",
            "detached",
            "locked because I said so",
            "",
            "worktree /tmp/orphan",
            "HEAD 0000000000000000000000000000000000000000",
            "detached",
            "prunable gitdir file points to non-existent location",
            "",
        ]
        .join("\n");

        let entries = parse_worktree_list(&stdout);

        assert_eq!(entries.len(), 4);
        assert!(entries[0].bare);
        assert_eq!(entries[1].branch_name.as_deref(), Some("foo"));
        assert!(entries[2].detached);
        assert_eq!(entries[2].locked.as_deref(), Some("because I said so"));
        assert_eq!(
            entries[3].prunable.as_deref(),
            Some("gitdir file points to non-existent location")
        );
    }

    #[test]
    fn parser_ignores_empty_records_and_tolerates_bare_locked() {
        let stdout = "\n\nworktree /a\nHEAD 1111111111111111111111111111111111111111\nbranch refs/heads/x\nlocked\n\n\n\n";

        let entries = parse_worktree_list(stdout);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].locked.as_deref(), Some(""));
    }

    #[test]
    fn sanitizes_branch_for_path() {
        assert_eq!(sanitize_branch_for_path("feature/login"), "feature-login");
        assert_eq!(sanitize_branch_for_path("feat: cool!"), "feat-cool");
        assert_eq!(sanitize_branch_for_path("a//b"), "a-b");
        assert_eq!(
            sanitize_branch_for_path("release.v1_beta"),
            "release.v1_beta"
        );
        assert_eq!(sanitize_branch_for_path("/foo/"), "foo");
        assert_eq!(sanitize_branch_for_path(&"a".repeat(200)).len(), 80);
    }

    #[test]
    fn expected_branch_for_pr_matches_gh_checkout_convention() {
        assert_eq!(
            expected_branch_for_pr(&sample_pr("feature/foo", "acme"), "acme"),
            "feature/foo"
        );
        assert_eq!(
            expected_branch_for_pr(&sample_pr("patch-1", "contributor"), "acme"),
            "contributor-patch-1"
        );
        assert_eq!(
            expected_branch_for_pr(&sample_pr("patch-1", ""), "acme"),
            "patch-1"
        );
    }

    #[test]
    fn matches_by_branch_before_path() {
        let entries = vec![
            WorktreeEntry {
                path: "/some/other/place".to_owned(),
                head: "a".repeat(40),
                branch_ref: Some("refs/heads/unrelated".to_owned()),
                branch_name: Some("unrelated".to_owned()),
                detached: false,
                bare: false,
                locked: None,
                prunable: None,
            },
            WorktreeEntry {
                path: "/Users/me/.legit/worktrees/acme/widgets/1-foo".to_owned(),
                head: "b".repeat(40),
                branch_ref: Some("refs/heads/foo".to_owned()),
                branch_name: Some("foo".to_owned()),
                detached: false,
                bare: false,
                locked: None,
                prunable: None,
            },
        ];

        assert_eq!(
            match_worktree(&entries, "foo", "/nonmatching/path").map(|entry| entry.path.as_str()),
            Some("/Users/me/.legit/worktrees/acme/widgets/1-foo")
        );
        assert_eq!(
            match_worktree(
                &entries,
                "ghost-branch",
                "/Users/me/.legit/worktrees/acme/widgets/1-foo",
            )
            .and_then(|entry| entry.branch_name.as_deref()),
            Some("foo")
        );
        assert!(match_worktree(&entries, "ghost", "/nowhere").is_none());
    }

    #[test]
    fn resolves_worktree_paths_from_config() {
        let config = LegitConfig {
            repos: vec![RepoConfig {
                slug: "acme/widgets".to_owned(),
                ..Default::default()
            }],
            ..Default::default()
        };

        assert_eq!(
            resolve_worktree_path(&config, "acme/widgets", 1234, "feature/foo")
                .expect("default worktree path"),
            home_dir()
                .expect("home directory")
                .join(".legit/worktrees/acme/widgets/1234-feature-foo")
        );

        let config = LegitConfig {
            repos: vec![RepoConfig {
                slug: "acme/widgets".to_owned(),
                worktree_root: Some("/wts/widgets".to_owned()),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(
            resolve_worktree_path(&config, "acme/widgets", 7, "main")
                .expect("repo-specific worktree path"),
            PathBuf::from("/wts/widgets/7-main")
        );

        let config = LegitConfig {
            repos: vec![RepoConfig {
                slug: "acme/widgets".to_owned(),
                ..Default::default()
            }],
            worktree_root: Some("/srv/wts".to_owned()),
            ..Default::default()
        };
        assert_eq!(
            resolve_worktree_path(&config, "acme/widgets", 7, "main")
                .expect("global worktree path"),
            PathBuf::from("/srv/wts/acme/widgets/7-main")
        );
    }

    #[test]
    fn resolves_source_clone_from_config() {
        let config = LegitConfig {
            repos: vec![
                RepoConfig {
                    slug: "acme/widgets".to_owned(),
                    source_clone: Some("~/src/widgets".to_owned()),
                    ..Default::default()
                },
                RepoConfig {
                    slug: "acme/gadgets".to_owned(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        assert_eq!(
            resolve_source_clone(&config, "acme/widgets").expect("sourceClone path"),
            Some(home_dir().expect("home directory").join("src/widgets"))
        );
        assert_eq!(
            resolve_source_clone(&config, "acme/gadgets").expect("missing sourceClone"),
            None
        );
        assert_eq!(
            resolve_source_clone(&config, "acme/unknown").expect("unknown repo sourceClone"),
            None
        );
    }

    #[test]
    fn home_expansion_requires_home() {
        let error = home_dir_from(None).expect_err("missing HOME should fail");

        assert_eq!(error.to_string(), "HOME is not set");
    }

    #[test]
    fn empty_home_is_treated_as_missing() {
        let error = home_dir_from(Some(OsString::new())).expect_err("empty HOME should fail");

        assert_eq!(error.to_string(), "HOME is not set");
    }

    #[test]
    fn tilde_config_paths_require_home() {
        let error = resolve_config_path_with(
            "~/src/widgets",
            || anyhow::bail!("HOME is not set"),
            || Ok(PathBuf::from("/cwd")),
        )
        .expect_err("tilde path without HOME should fail");

        assert!(error.to_string().contains("HOME is not set"));
    }

    #[test]
    fn relative_config_paths_require_current_dir() {
        let error = resolve_config_path_with(
            "src/widgets",
            || Ok(PathBuf::from("/home/me")),
            || Err(io::Error::new(io::ErrorKind::NotFound, "cwd was deleted")),
        )
        .expect_err("relative path without current dir should fail");

        assert!(
            error
                .to_string()
                .contains("failed to resolve current directory")
        );
    }

    #[test]
    fn pr_key_url_stays_available_for_worktree_messages() {
        let key = PrKey {
            repo_slug: "acme/widgets".to_owned(),
            number: 42,
        };

        assert_eq!(key.html_url(), "https://github.com/acme/widgets/pull/42");
    }
}
