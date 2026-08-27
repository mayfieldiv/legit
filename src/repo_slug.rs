//! GitHub repo-slug identity.
//!
//! `RepoSlug` is the one type an `owner/repo` name travels in wherever it
//! means identity — `PrKey`, `TicketKey`/`EffortKey`, Tracked Repo lookup
//! and dedup, model maps. Equality and hashing are ASCII-case-insensitive
//! because GitHub treats slugs that way, while the stored casing is
//! preserved for display, so keys built from config and keys built from
//! wire payloads (`repository.nameWithOwner`) unify no matter how either
//! side was cased.

#[derive(Debug, Clone)]
pub struct RepoSlug(String);

impl RepoSlug {
    pub fn new(slug: impl Into<String>) -> Self {
        Self(slug.into())
    }

    /// The slug as entered — display casing, not the identity.
    pub fn as_str(&self) -> &str {
        &self.0
    }
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
        self.0.to_ascii_lowercase().hash(state);
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
        self.0
            .to_ascii_lowercase()
            .cmp(&other.0.to_ascii_lowercase())
    }
}

impl std::fmt::Display for RepoSlug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests;
