use std::process::Command;

use anyhow::{Context, Result, bail};

pub fn resolve_token() -> Result<String> {
    let output = Command::new("gh")
        .args(["auth", "token"])
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_TOKEN")
        .output()
        .context("failed to run `gh auth token`")?;

    if !output.status.success() {
        bail!("`gh auth token` exited with {}", output.status);
    }

    let token = String::from_utf8(output.stdout)
        .context("`gh auth token` returned non-utf8 output")?
        .trim()
        .to_owned();

    if token.is_empty() {
        bail!("`gh auth token` returned an empty token");
    }

    Ok(token)
}
