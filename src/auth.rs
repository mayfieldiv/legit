use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::{secret::Secret, subprocess::make_noninteractive};

#[tracing::instrument(name = "resolve_auth_token")]
pub fn resolve_token() -> Result<Secret<String>> {
    tracing::info!("resolving auth token with gh cli");
    let mut command = Command::new("gh");
    command
        .args(["auth", "token"])
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_TOKEN");
    // `gh auth token` only reads the stored token, but harden it like every
    // other subprocess we launch while the TUI owns the terminal: a prompt here
    // could never be answered.
    make_noninteractive(&mut command);
    let output = command.output().context("failed to run `gh auth token`")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        if stderr.is_empty() {
            bail!("`gh auth token` exited with {}", output.status);
        }
        bail!("`gh auth token` exited with {}: {}", output.status, stderr);
    }

    let token = String::from_utf8(output.stdout)
        .context("`gh auth token` returned non-utf8 output")?
        .trim()
        .to_owned();

    if token.is_empty() {
        bail!("`gh auth token` returned an empty token");
    }

    tracing::debug!("gh auth token returned non-empty token");
    Ok(Secret::new(token))
}
