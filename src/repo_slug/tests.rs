//! Unit tests for `RepoSlug` identity semantics.

use super::RepoSlug;

#[test]
fn identity_ignores_ascii_case_and_keeps_display_casing() {
    let config_cased = RepoSlug::new("MayfieldIV/Legit");
    let wire_cased = RepoSlug::new("mayfieldiv/legit");
    assert_eq!(config_cased, wire_cased);
    assert_eq!(config_cased.to_string(), "MayfieldIV/Legit");

    let mut seen = std::collections::HashSet::new();
    seen.insert(config_cased);
    assert!(seen.contains(&wire_cased));
}

#[test]
fn different_slugs_stay_distinct() {
    assert_ne!(RepoSlug::new("acme/web"), RepoSlug::new("acme/api"));
}
