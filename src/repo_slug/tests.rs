//! Unit tests for `RepoSlug` parsing and identity semantics.

use super::RepoSlug;

fn slug(text: &str) -> RepoSlug {
    RepoSlug::parse(text).unwrap()
}

#[test]
fn identity_ignores_ascii_case_and_keeps_display_casing() {
    let config_cased = slug("MayfieldIV/Legit");
    let wire_cased = slug("mayfieldiv/legit");
    assert_eq!(config_cased, wire_cased);
    assert_eq!(config_cased.to_string(), "MayfieldIV/Legit");

    let mut seen = std::collections::HashSet::new();
    seen.insert(config_cased);
    assert!(seen.contains(&wire_cased));
}

#[test]
fn different_slugs_stay_distinct() {
    assert_ne!(slug("acme/web"), slug("acme/api"));
}

#[test]
fn ordering_folds_case_and_orders_equal_slugs_as_equal() {
    assert_eq!(
        slug("ACME/web").cmp(&slug("acme/WEB")),
        std::cmp::Ordering::Equal
    );
    assert!(slug("acme/api") < slug("ACME/web"));
}

#[test]
fn exposes_owner_and_name_in_display_casing() {
    let parsed = slug("MayfieldIV/Legit");
    assert_eq!(parsed.owner(), "MayfieldIV");
    assert_eq!(parsed.name(), "Legit");
}

#[test]
fn parses_dotted_repo_names() {
    assert_eq!(slug("angular/angular.js").name(), "angular.js");
}

#[test]
fn parse_rejects_missing_or_empty_segments() {
    for invalid in ["acme", "acme/", "/web", "/"] {
        let error = RepoSlug::parse(invalid).unwrap_err();
        assert!(
            format!("{error}").contains("expected exactly owner/repo"),
            "{invalid:?}: {error}"
        );
    }
}

#[test]
fn parse_rejects_extra_segments() {
    let error = RepoSlug::parse("a/b/c").unwrap_err();
    assert!(format!("{error}").contains("expected exactly owner/repo"));
}

#[test]
fn parse_rejects_path_traversal_segments() {
    for invalid in ["../web", "acme/..", "./web", "acme/."] {
        let error = RepoSlug::parse(invalid).unwrap_err();
        assert!(
            format!("{error}").contains("path traversal"),
            "{invalid:?}: {error}"
        );
    }
}

#[test]
fn parse_rejects_disallowed_characters() {
    for invalid in ["ac me/web", "acme/we:b", "acmé/web"] {
        let error = RepoSlug::parse(invalid).unwrap_err();
        assert!(
            format!("{error}").contains("only ASCII letters"),
            "{invalid:?}: {error}"
        );
    }
}

#[test]
fn serde_round_trips_display_casing_and_validates_on_deserialize() {
    let parsed: RepoSlug = serde_json::from_str("\"MayfieldIV/Legit\"").unwrap();
    assert_eq!(parsed.as_str(), "MayfieldIV/Legit");
    assert_eq!(
        serde_json::to_string(&parsed).unwrap(),
        "\"MayfieldIV/Legit\""
    );

    let error = serde_json::from_str::<RepoSlug>("\"not-a-slug\"").unwrap_err();
    assert!(format!("{error}").contains("expected exactly owner/repo"));
}
