use std::{path::Path, process::Command};

use anyhow::{Context, Result, bail};

use crate::subprocess::make_noninteractive;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoInfo {
    pub owner: String,
    pub repo: String,
}

impl RepoInfo {
    /// Parse an `owner/repo` slug (the config `repos` format) back into parts.
    /// `None` for malformed slugs. The owner/repo split agrees with
    /// `config::validate_repo_slug`: a `/` inside the repo part (a three-or-more
    /// segment slug like `a/b/c`) is rejected, so the two parsers can't disagree
    /// at the edges. Only `Model::tracked_repos` calls this, on slugs that have
    /// already passed config validation, so `None` is an unreachable guard
    /// there rather than an error path.
    pub fn from_slug(slug: &str) -> Option<Self> {
        let (owner, repo) = slug.split_once('/')?;
        if owner.is_empty() || repo.is_empty() || repo.contains('/') {
            return None;
        }
        Some(Self {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
        })
    }

    /// The `owner/repo` slug for this repo — the form config, tabs, and
    /// `PR::repo_slug` all use.
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }
}

/// Parse a GitHub remote URL into (owner, repo). Mirrors the TS `parseRemoteUrl`
/// in `src/lib/legit.ts` so dotted repo names (e.g. `angular.js`) and both SSH
/// and HTTPS forms parse identically.
pub fn parse_remote_url(url: &str) -> Result<RepoInfo> {
    if let Some((owner, repo)) = parse_ssh(url) {
        return Ok(RepoInfo { owner, repo });
    }
    if let Some((owner, repo)) = parse_https(url) {
        return Ok(RepoInfo { owner, repo });
    }
    bail!("Cannot parse GitHub remote URL: {url}");
}

fn parse_ssh(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("git@github.com:")?;
    split_owner_repo(rest)
}

fn parse_https(url: &str) -> Option<(String, String)> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))?;
    split_owner_repo(rest)
}

fn split_owner_repo(rest: &str) -> Option<(String, String)> {
    let (owner, repo) = rest.split_once('/')?;
    if owner.is_empty() {
        return None;
    }
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    if repo.is_empty() {
        return None;
    }
    Some((owner.to_owned(), repo.to_owned()))
}

/// Detect the GitHub repo for the given working directory by reading
/// `git remote get-url origin`.
#[tracing::instrument(name = "detect_repo")]
pub fn detect_repo(cwd: &Path) -> Result<RepoInfo> {
    tracing::info!(path = %cwd.display(), "detecting repo from git remote");
    let mut command = Command::new("git");
    command
        .args(["remote", "get-url", "origin"])
        .current_dir(cwd);
    // Reading the remote URL is a local operation that won't prompt, but harden
    // it like every other subprocess we launch while the TUI owns the terminal.
    make_noninteractive(&mut command);
    let output = command.output().with_context(|| {
        format!(
            "failed to run `git remote get-url origin` in {}",
            cwd.display()
        )
    })?;

    if !output.status.success() {
        bail!(
            "No git remote 'origin' found in {} (`git remote get-url origin` exited {})",
            cwd.display(),
            output.status,
        );
    }

    let url = String::from_utf8(output.stdout)
        .context("`git remote get-url origin` returned non-utf8 output")?
        .trim()
        .to_owned();

    parse_remote_url(&url)
}

#[cfg(test)]
mod tests {
    use super::{RepoInfo, parse_remote_url};

    fn info(owner: &str, repo: &str) -> RepoInfo {
        RepoInfo {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
        }
    }

    #[test]
    fn parses_ssh_url_with_git_suffix() {
        assert_eq!(
            parse_remote_url("git@github.com:owner/repo.git").unwrap(),
            info("owner", "repo"),
        );
    }

    #[test]
    fn parses_ssh_url_without_git_suffix() {
        assert_eq!(
            parse_remote_url("git@github.com:owner/repo").unwrap(),
            info("owner", "repo"),
        );
    }

    #[test]
    fn parses_https_url_with_git_suffix() {
        assert_eq!(
            parse_remote_url("https://github.com/owner/repo.git").unwrap(),
            info("owner", "repo"),
        );
    }

    #[test]
    fn parses_https_url_without_git_suffix() {
        assert_eq!(
            parse_remote_url("https://github.com/owner/repo").unwrap(),
            info("owner", "repo"),
        );
    }

    #[test]
    fn parses_ssh_url_with_dotted_repo_with_git_suffix() {
        assert_eq!(
            parse_remote_url("git@github.com:angular/angular.js.git").unwrap(),
            info("angular", "angular.js"),
        );
    }

    #[test]
    fn parses_ssh_url_with_dotted_repo_without_git_suffix() {
        assert_eq!(
            parse_remote_url("git@github.com:socketio/socket.io").unwrap(),
            info("socketio", "socket.io"),
        );
    }

    #[test]
    fn parses_https_url_with_dotted_repo_with_git_suffix() {
        assert_eq!(
            parse_remote_url("https://github.com/highlightjs/highlight.js.git").unwrap(),
            info("highlightjs", "highlight.js"),
        );
    }

    #[test]
    fn parses_https_url_with_dotted_repo_without_git_suffix() {
        assert_eq!(
            parse_remote_url("https://github.com/kubernetes/kubernetes.io").unwrap(),
            info("kubernetes", "kubernetes.io"),
        );
    }

    #[test]
    fn rejects_non_github_url() {
        let err = parse_remote_url("git@gitlab.com:owner/repo.git").unwrap_err();
        assert!(format!("{err}").contains("Cannot parse"));
    }

    #[test]
    fn rejects_malformed_url() {
        let err = parse_remote_url("not-a-url").unwrap_err();
        assert!(format!("{err}").contains("Cannot parse"));
    }

    #[test]
    fn from_slug_parses_owner_repo() {
        assert_eq!(RepoInfo::from_slug("acme/web"), Some(info("acme", "web")));
    }

    #[test]
    fn from_slug_rejects_empty_segments() {
        assert_eq!(RepoInfo::from_slug("acme"), None);
        assert_eq!(RepoInfo::from_slug("acme/"), None);
        assert_eq!(RepoInfo::from_slug("/web"), None);
    }

    #[test]
    fn from_slug_rejects_extra_segment_to_agree_with_validate_repo_slug() {
        // `a/b/c` would split into owner=a, repo=b/c; rejecting it keeps
        // `from_slug` in lockstep with `config::validate_repo_slug`.
        assert_eq!(RepoInfo::from_slug("a/b/c"), None);
    }
}
