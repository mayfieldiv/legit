//! Wrapper that hides a value from `Debug` output. The inner value is
//! accessible only via [`Secret::expose_secret`], so every site that touches
//! the raw value is greppable.

use std::fmt;

/// Wraps a value whose printed form would leak secrets (e.g. a GitHub API
/// token). `Debug` always renders as `<redacted>` regardless of inner state,
/// so `tracing` / `format!("{model:?}")` can't accidentally surface it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret<T>(T);

impl<T> Secret<T> {
    pub fn new(value: T) -> Self {
        Self(value)
    }

    /// Returns the wrapped value. Named "expose" rather than "value" so call
    /// sites read as an explicit decision to surface a secret.
    pub fn expose_secret(&self) -> &T {
        &self.0
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

#[cfg(test)]
mod tests {
    use super::Secret;

    #[test]
    fn debug_redacts_inner_value() {
        let secret = Secret::new("ghp_supersecret".to_owned());

        let debug = format!("{secret:?}");

        assert_eq!(debug, "<redacted>");
        assert!(!debug.contains("ghp_supersecret"));
    }

    #[test]
    fn debug_inside_option_still_redacts() {
        let opt: Option<Secret<String>> = Some(Secret::new("ghp_supersecret".to_owned()));

        let debug = format!("{opt:?}");

        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("ghp_supersecret"));
    }

    #[test]
    fn expose_secret_returns_inner_value() {
        let secret = Secret::new(42_u64);

        assert_eq!(*secret.expose_secret(), 42);
    }
}
