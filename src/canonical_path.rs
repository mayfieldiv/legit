//! Canonical filesystem-path identity.
//!
//! `CanonicalPathBuf` is proof of canonicalization: its only production
//! constructor runs `std::fs::canonicalize`, so two values naming the same
//! file compare equal even when the inputs were relative, symlinked, or
//! dot-ridden. Local wayfinder Tickets and Efforts key on it; the I/O lives
//! here so `src/ticket.rs` stays a pure model module.

// TODO(#118): remove once local Effort discovery constructs these.
#![allow(dead_code)]

use std::fs;
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CanonicalPathBuf(PathBuf);

impl CanonicalPathBuf {
    /// Canonicalize `path` — resolving relative prefixes, `.`/`..` segments,
    /// and symlinks — the only production constructor. Fails when the path
    /// does not exist, exactly like `std::fs::canonicalize`.
    pub fn canonicalize(path: impl AsRef<Path>) -> io::Result<Self> {
        fs::canonicalize(path).map(Self)
    }

    /// Test-only: adopt a path verbatim, so pure model tests can build keys
    /// without touching the filesystem.
    #[cfg(test)]
    pub fn assume_canonical(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }
}

impl Deref for CanonicalPathBuf {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for CanonicalPathBuf {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
mod tests;
