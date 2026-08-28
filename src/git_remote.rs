use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::repo_slug::RepoSlug;
use crate::subprocess::{GitEnv, git_command, run_command};

/// Parse a GitHub remote URL into a repo slug. Mirrors the TS `parseRemoteUrl`
/// in `src/lib/legit.ts` so dotted repo names (e.g. `angular.js`) and both SSH
/// and HTTPS forms parse identically. The extracted parts still pass through
/// `RepoSlug::parse`, so a remote whose owner/repo violates slug syntax is
/// rejected here rather than smuggling an unvalidated slug into the app.
pub fn parse_remote_url(url: &str) -> Result<RepoSlug> {
    let Some(rest) = parse_ssh(url).or_else(|| parse_https(url)) else {
        bail!("Cannot parse GitHub remote URL: {url}");
    };
    RepoSlug::parse(rest).with_context(|| format!("Cannot parse GitHub remote URL: {url}"))
}

fn parse_ssh(url: &str) -> Option<String> {
    let rest = url.strip_prefix("git@github.com:")?;
    strip_git_suffix(rest)
}

fn parse_https(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))?;
    strip_git_suffix(rest)
}

fn strip_git_suffix(rest: &str) -> Option<String> {
    Some(rest.strip_suffix(".git").unwrap_or(rest).to_owned())
}

/// Detect the GitHub repo for the given working directory by reading
/// `git remote get-url origin`.
#[tracing::instrument(name = "detect_repo")]
pub fn detect_repo(cwd: &Path) -> Result<RepoSlug> {
    tracing::info!(path = %cwd.display(), "detecting repo from git remote");
    // Reading the remote URL is a local operation that won't prompt, but run it
    // through the hardened path (non-interactive, timeout, shutdown-tracked) like
    // every other git child. GitEnv::Ambient because, unlike the worktree
    // commands, this operates on the user's real cwd repo.
    let mut command = git_command(GitEnv::Ambient);
    command
        .args(["remote", "get-url", "origin"])
        .current_dir(cwd);
    let url = run_command("git remote get-url origin", &mut command)
        .with_context(|| format!("no git remote 'origin' found in {}", cwd.display()))?
        .trim()
        .to_owned();

    parse_remote_url(&url)
}

#[cfg(test)]
mod tests {
    use super::parse_remote_url;

    #[test]
    fn parses_ssh_url_with_git_suffix() {
        assert_eq!(
            parse_remote_url("git@github.com:owner/repo.git").unwrap(),
            "owner/repo",
        );
    }

    #[test]
    fn parses_ssh_url_without_git_suffix() {
        assert_eq!(
            parse_remote_url("git@github.com:owner/repo").unwrap(),
            "owner/repo",
        );
    }

    #[test]
    fn parses_https_url_with_git_suffix() {
        assert_eq!(
            parse_remote_url("https://github.com/owner/repo.git").unwrap(),
            "owner/repo",
        );
    }

    #[test]
    fn parses_https_url_without_git_suffix() {
        assert_eq!(
            parse_remote_url("https://github.com/owner/repo").unwrap(),
            "owner/repo",
        );
    }

    #[test]
    fn parses_ssh_url_with_dotted_repo_with_git_suffix() {
        assert_eq!(
            parse_remote_url("git@github.com:angular/angular.js.git").unwrap(),
            "angular/angular.js",
        );
    }

    #[test]
    fn parses_ssh_url_with_dotted_repo_without_git_suffix() {
        assert_eq!(
            parse_remote_url("git@github.com:socketio/socket.io").unwrap(),
            "socketio/socket.io",
        );
    }

    #[test]
    fn parses_https_url_with_dotted_repo_with_git_suffix() {
        assert_eq!(
            parse_remote_url("https://github.com/highlightjs/highlight.js.git").unwrap(),
            "highlightjs/highlight.js",
        );
    }

    #[test]
    fn parses_https_url_with_dotted_repo_without_git_suffix() {
        assert_eq!(
            parse_remote_url("https://github.com/kubernetes/kubernetes.io").unwrap(),
            "kubernetes/kubernetes.io",
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
    fn rejects_github_url_with_invalid_slug_syntax() {
        // The host prefix parses, but the extracted parts still go through
        // `RepoSlug::parse` — a traversal segment must not become a slug.
        let err = parse_remote_url("https://github.com/owner/..").unwrap_err();
        assert!(format!("{err:#}").contains("Cannot parse"), "{err:#}");
    }
}
