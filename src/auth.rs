use anyhow::{Result, bail};

use crate::{
    secret::Secret,
    subprocess::{gh_command, run_command},
};

#[tracing::instrument(name = "resolve_auth_token")]
pub fn resolve_token() -> Result<Secret<String>> {
    tracing::info!("resolving auth token with gh cli");
    // `gh auth token` only reads the stored token, but run it through the same
    // hardened path as every other gh/git child: `gh_command` strips the ambient
    // GITHUB_TOKEN/GH_TOKEN (so it reads the *stored* token) and `run_command`
    // adds the non-interactive/timeout/shutdown-tracking hardening.
    let mut command = gh_command();
    command.args(["auth", "token"]);
    let token = run_command("gh auth token", &mut command)?
        .trim()
        .to_owned();

    if token.is_empty() {
        bail!("`gh auth token` returned an empty token");
    }

    tracing::debug!("gh auth token returned non-empty token");
    Ok(Secret::new(token))
}
