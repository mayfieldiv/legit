use anyhow::{Context, Result, bail};

use crate::{
    secret::Secret,
    subprocess::{GitEnv, gh_command, run_command},
};

/// A GitHub token that can be sent: one run of visible ASCII with no spaces.
/// The only constructor validates, so a client that takes an `AuthToken`
/// needs no check of its own — octocrab's builder parses `Bearer <token>`
/// into a header value with an `unwrap`, and anything else (an escape
/// sequence, a notice gh wrote to stdout, a non-ASCII character) would panic
/// the fetch task and wedge the PR list at "Loading…" with no error shown.
/// Redacted in `Debug` like the `Secret` it wraps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthToken(Secret<String>);

impl AuthToken {
    /// Accept `token`, or name the byte that disqualifies it by offset and
    /// value — never the token's content.
    pub fn parse(token: &str) -> Result<Self> {
        if token.is_empty() {
            bail!("empty token");
        }
        if let Some((offset, byte)) = token
            .bytes()
            .enumerate()
            .find(|(_, byte)| !byte.is_ascii_graphic())
        {
            bail!(
                "{} bytes that aren't a token: byte {byte:#04x} at offset {offset}",
                token.len()
            );
        }
        Ok(Self(Secret::new(token.to_owned())))
    }

    pub fn expose_secret(&self) -> &str {
        self.0.expose_secret()
    }
}

#[tracing::instrument(name = "resolve_auth_token")]
pub fn resolve_token() -> Result<AuthToken> {
    tracing::info!("resolving auth token with gh cli");
    // `gh auth token` only reads the stored token, but run it through the same
    // hardened path as every other gh/git child: `gh_command` strips the ambient
    // GITHUB_TOKEN/GH_TOKEN (so it reads the *stored* token) and `run_command`
    // adds the non-interactive/timeout/shutdown-tracking hardening.
    // GitEnv::Ambient: no repository is involved, so there is nothing to scope.
    let mut command = gh_command(GitEnv::Ambient);
    command.args(["auth", "token"]);
    let output = run_command("gh auth token", &mut command)?;
    let token = AuthToken::parse(output.trim())
        .context("`gh auth token` output isn't a token (is gh printing a message to stdout?)")?;
    tracing::debug!("gh auth token returned a well-formed token");
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::AuthToken;

    #[test]
    fn a_plain_token_is_accepted() {
        assert!(AuthToken::parse("gho_ABCdef123_456-789").is_ok());
    }

    #[test]
    fn an_empty_token_is_rejected() {
        let error = AuthToken::parse("").unwrap_err().to_string();
        assert!(error.contains("empty token"), "{error}");
    }

    #[test]
    fn control_characters_and_non_ascii_are_rejected_by_offset_without_leaking_the_token() {
        // An ANSI escape sequence spliced into the output.
        let error = AuthToken::parse("gho_ab\x1b[0mcd").unwrap_err().to_string();
        assert!(error.contains("byte 0x1b at offset 6"), "{error}");
        assert!(
            !error.contains("gho_ab"),
            "must not echo the token: {error}"
        );

        // A notice on stdout ahead of the token.
        let error = AuthToken::parse("! Refreshing\ngho_abc")
            .unwrap_err()
            .to_string();
        assert!(error.contains("byte 0x20 at offset 1"), "{error}");

        // Non-ASCII text.
        let error = AuthToken::parse("gho_é").unwrap_err().to_string();
        assert!(error.contains("byte 0xc3 at offset 4"), "{error}");
    }

    #[test]
    fn debug_redacts_the_token() {
        let token = AuthToken::parse("gho_supersecret").unwrap();
        let debug = format!("{token:?}");
        assert!(debug.contains("<redacted>"), "{debug}");
        assert!(!debug.contains("supersecret"), "{debug}");
    }
}
