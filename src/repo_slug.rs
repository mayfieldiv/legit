//! GitHub repo-slug identity.
//!
//! `RepoSlug` is the one type an `owner/repo` name travels in wherever it
//! means identity — `PrKey`, `TicketKey`/`EffortKey`, Tracked Repo lookup
//! and dedup, model maps. Equality, hashing, and ordering are
//! ASCII-case-insensitive because GitHub treats slugs that way, while the
//! stored casing is preserved for display, so keys built from config and keys
//! built from wire payloads (`repository.nameWithOwner`) unify no matter how
//! either side was cased.
//!
//! [`RepoSlug::parse`] is the only constructor and owns all slug syntax, so
//! holding a `RepoSlug` certifies two invariants: it is a syntactically valid
//! GitHub `owner/repo` name, and it is safe to embed in a filesystem path
//! (no `.`/`..` segments, no characters outside `[A-Za-z0-9._-]` — the
//! worktree layout joins slugs into paths).

use anyhow::{Result, ensure};

#[derive(Debug, Clone)]
pub struct RepoSlug(String);

impl RepoSlug {
    /// Parse and validate an `owner/repo` slug: exactly two non-empty
    /// segments, neither `.` nor `..`, only ASCII letters, numbers, `.`,
    /// `_`, and `-`. Every slug in the program — config, CWD detection,
    /// wire payloads — enters through here.
    pub fn parse(slug: impl Into<String>) -> Result<Self> {
        let slug = slug.into();
        // `name` holds everything after the first '/', so `name.contains('/')`
        // rejects three-or-more-segment slugs alongside the empty-segment cases.
        let (owner, name) = slug.split_once('/').unwrap_or_default();
        ensure!(
            !owner.is_empty() && !name.is_empty() && !name.contains('/'),
            "invalid repo slug {slug:?}: expected exactly owner/repo"
        );
        for part in [owner, name] {
            ensure!(
                part != "." && part != "..",
                "invalid repo slug {slug:?}: path traversal segments are not allowed"
            );
            ensure!(
                part.chars().all(is_repo_slug_char),
                "invalid repo slug {slug:?}: only ASCII letters, numbers, '.', '_', and '-' are allowed"
            );
        }
        Ok(Self(slug))
    }

    /// Unvalidated construction — every remaining caller is being migrated to
    /// [`RepoSlug::parse`]; deleted once `RepoInfo` is gone.
    pub fn new(slug: impl Into<String>) -> Self {
        Self(slug.into())
    }

    /// The slug as entered — display casing, not the identity.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The owner segment, in display casing.
    pub fn owner(&self) -> &str {
        self.split().0
    }

    /// The repo-name segment, in display casing.
    pub fn name(&self) -> &str {
        self.split().1
    }

    fn split(&self) -> (&str, &str) {
        self.0
            .split_once('/')
            .expect("RepoSlug::parse guarantees an owner/repo split")
    }

    /// The identity view: the slug's bytes with ASCII case folded away.
    /// `Eq`/`Hash`/`Ord` all read the slug through this so they can never
    /// disagree with each other.
    fn folded_bytes(&self) -> impl Iterator<Item = u8> + '_ {
        self.0.bytes().map(|byte| byte.to_ascii_lowercase())
    }
}

fn is_repo_slug_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-')
}

impl PartialEq for RepoSlug {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl Eq for RepoSlug {}

/// Same identity against a raw string, for the config/wire boundaries where
/// the other side hasn't been typed yet (and for test assertions).
impl PartialEq<str> for RepoSlug {
    fn eq(&self, other: &str) -> bool {
        self.0.eq_ignore_ascii_case(other)
    }
}

impl PartialEq<&str> for RepoSlug {
    fn eq(&self, other: &&str) -> bool {
        self.0.eq_ignore_ascii_case(other)
    }
}

impl std::hash::Hash for RepoSlug {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for byte in self.folded_bytes() {
            state.write_u8(byte);
        }
        // Length terminator, as `Hash for str` writes one: without it the
        // fold of ("a", "b/c") and ("a/b", "c") style neighbours could
        // collide when hashed in sequence.
        state.write_u8(0xff);
    }
}

impl PartialOrd for RepoSlug {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RepoSlug {
    /// Case-insensitive, to stay consistent with `Eq`: slugs that compare
    /// equal must order as equal.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.folded_bytes().cmp(other.folded_bytes())
    }
}

impl std::fmt::Display for RepoSlug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Serializes as the display-cased slug string, so a `RepoSlug` round-trips
/// through config JSON unchanged.
impl serde::Serialize for RepoSlug {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

/// Deserializes via [`RepoSlug::parse`], so a slug that arrives through serde
/// (config `repos` entries) carries the same validated-syntax invariant as
/// every other construction site.
impl<'de> serde::Deserialize<'de> for RepoSlug {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let slug = String::deserialize(deserializer)?;
        RepoSlug::parse(slug).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests;
