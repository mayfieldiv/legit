//! Browser URL construction and command helpers.
//!
//! The reducer stays pure: helpers here only build URLs and `Cmd::OpenUrl`.
//! The impure platform opener runs from `cmd`.

use std::process::{Command, Stdio};

use anyhow::Context;

use crate::github::rest::PR;

use super::cmd::Cmd;

const DEVIN_ORIGIN: &str = "https://app.devin.ai/";

pub fn pr_url(repo_slug: &str, number: u64) -> String {
    format!("https://github.com/{repo_slug}/pull/{number}")
}

pub fn devin_url(repo_slug: &str, number: u64) -> String {
    let mut parts = repo_slug.split('/');
    let owner = parts.next().unwrap_or("");
    let repo = parts.next().unwrap_or("undefined");
    format!("https://app.devin.ai/review/{owner}/{repo}/pull/{number}")
}

pub fn open_url(url: impl Into<String>) -> Cmd {
    Cmd::OpenUrl { url: url.into() }
}

pub fn open_in_browser(pr: &PR) -> Cmd {
    open_url(pr_url(&pr.repo_slug, pr.number))
}

pub fn open_in_devin(pr: &PR) -> Cmd {
    open_url(devin_url(&pr.repo_slug, pr.number))
}

pub fn open_label(url: &str) -> &'static str {
    if url.starts_with(DEVIN_ORIGIN) {
        "Devin"
    } else {
        "browser"
    }
}

/// Spawn the platform URL opener and return after the child has been created.
/// A small reaper thread waits on the child so repeated opens do not leave
/// defunct opener processes behind, but the TUI never waits for the browser
/// command to finish.
pub fn spawn_open_url(url: &str) -> anyhow::Result<()> {
    let mut child = platform_open_command(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("spawn browser opener")?;

    let _ = std::thread::Builder::new()
        .name("legit-browser-open-reaper".to_owned())
        .spawn(move || {
            let _ = child.wait();
        });

    Ok(())
}

fn platform_open_command(url: &str) -> Command {
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("open");
        command.arg(url);
        command
    }

    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("cmd");
        command.args(["/c", "start", "", url]);
        command
    }

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    }
}

#[cfg(test)]
mod tests {
    use super::{devin_url, open_label, pr_url};

    #[test]
    fn builds_github_pr_url() {
        assert_eq!(
            pr_url("mayfieldiv/legit", 45),
            "https://github.com/mayfieldiv/legit/pull/45"
        );
    }

    #[test]
    fn builds_devin_url_with_ts_format() {
        assert_eq!(
            devin_url("mayfieldiv/legit", 45),
            "https://app.devin.ai/review/mayfieldiv/legit/pull/45"
        );
    }

    #[test]
    fn labels_devin_urls_separately() {
        assert_eq!(
            open_label("https://app.devin.ai/review/mayfieldiv/legit/pull/45"),
            "Devin"
        );
        assert_eq!(
            open_label("https://github.com/mayfieldiv/legit/pull/45"),
            "browser"
        );
    }
}
