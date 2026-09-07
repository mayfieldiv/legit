use anyhow::{Result, bail};

use crate::{
    secret::Secret,
    subprocess::{GitEnv, gh_command, run_command},
};

#[tracing::instrument(name = "resolve_auth_token")]
pub fn resolve_token() -> Result<Secret<String>> {
    tracing::info!("resolving auth token with gh cli");
    // `gh auth token` only reads the stored token, but run it through the same
    // hardened path as every other gh/git child: `gh_command` strips the ambient
    // GITHUB_TOKEN/GH_TOKEN (so it reads the *stored* token) and `run_command`
    // adds the non-interactive/timeout/shutdown-tracking hardening.
    // GitEnv::Ambient: no repository is involved, so there is nothing to scope.
    let mut command = gh_command(GitEnv::Ambient);
    command.args(["auth", "token"]);
    let token = run_command("gh auth token", &mut command)?
        .trim()
        .to_owned();
    validate_token(&token)?;
    tracing::debug!("gh auth token returned a well-formed token");
    Ok(Secret::new(token))
}

/// Reject anything `gh auth token` printed that can't be a token. A token is
/// one run of visible ASCII with no spaces; anything else — an escape
/// sequence, a notice gh wrote to stdout, a non-ASCII character — would
/// otherwise reach octocrab, whose builder parses `Bearer <token>` into a
/// header value with an `unwrap` and panics the fetch task, wedging the PR
/// list at "Loading…" with no error shown. The error names the offending
/// byte by offset and value, never the token's content.
fn validate_token(token: &str) -> Result<()> {
    if token.is_empty() {
        bail!("`gh auth token` returned an empty token");
    }
    if let Some((offset, byte)) = token
        .bytes()
        .enumerate()
        .find(|(_, byte)| !byte.is_ascii_graphic())
    {
        bail!(
            "`gh auth token` returned {} bytes that aren't a token: byte {byte:#04x} at offset \
             {offset} (is gh printing a message to stdout?)",
            token.len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_token;

    #[test]
    fn a_plain_token_is_accepted() {
        assert!(validate_token("gho_ABCdef123_456-789").is_ok());
    }

    #[test]
    fn an_empty_token_is_rejected() {
        let error = validate_token("").unwrap_err().to_string();
        assert!(error.contains("empty token"), "{error}");
    }

    #[test]
    fn control_characters_and_non_ascii_are_rejected_by_offset_without_leaking_the_token() {
        // An ANSI escape sequence spliced into the output.
        let error = validate_token("gho_ab\x1b[0mcd").unwrap_err().to_string();
        assert!(error.contains("byte 0x1b at offset 6"), "{error}");
        assert!(
            !error.contains("gho_ab"),
            "must not echo the token: {error}"
        );

        // A notice on stdout ahead of the token.
        let error = validate_token("! Refreshing\ngho_abc")
            .unwrap_err()
            .to_string();
        assert!(error.contains("byte 0x20 at offset 1"), "{error}");

        // Non-ASCII text.
        let error = validate_token("gho_é").unwrap_err().to_string();
        assert!(error.contains("byte 0xc3 at offset 4"), "{error}");
    }
}
