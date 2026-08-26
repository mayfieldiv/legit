use std::{
    ffi::OsString,
    fs, io,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    LegitConfig, RepoConfig, RepoIdentity, home_dir, home_dir_from, load_from_path,
    resolve_config_path_with,
};

#[test]
fn missing_config_returns_defaults() {
    let path = temp_path("missing");

    let config = load_from_path(path).expect("config should load");

    assert_eq!(config, LegitConfig::default());
}

#[test]
fn partial_config_fills_defaults_and_tolerates_legacy_repos() {
    let path = temp_path("partial");
    fs::write(
        &path,
        r#"{
            "user": "mayfield",
            "repos": ["acme/widgets", {"slug": "acme/gadgets", "mainWorktreePath": "/src/gadgets"}],
            "ui": {"defaultSortBy": "age"}
        }"#,
    )
    .expect("write config");

    let config = load_from_path(path.clone()).expect("config should load");
    let _ = fs::remove_file(path);

    assert_eq!(config.user, "mayfield");
    assert_eq!(config.repos.len(), 2);
    assert_eq!(config.repos[0].slug.as_deref(), Some("acme/widgets"));
    assert_eq!(config.repos[1].slug.as_deref(), Some("acme/gadgets"));
    assert_eq!(
        config.repos[1].main_worktree_path.as_deref(),
        Some("/src/gadgets")
    );
    assert_eq!(config.bot_logins, LegitConfig::default().bot_logins);
    assert_eq!(config.ui.default_group_by, "smart-status");
    assert_eq!(config.ui.default_sort_by, "age");
}

#[test]
fn structured_repo_with_invalid_slug_fails() {
    let error = load_error(
        "structured-invalid-slug",
        r#"{"repos": [{"slug": "no-slash"}]}"#,
    );

    assert!(
        error.contains("invalid repos[0].slug \"no-slash\""),
        "{error}"
    );
    assert!(error.contains("expected exactly owner/repo"));
}

#[test]
fn slug_error_names_the_offending_entry() {
    let error = load_error(
        "second-invalid-slug",
        r#"{"repos": ["acme/widgets", {"slug": "bogus"}]}"#,
    );

    assert!(error.contains("invalid repos[1].slug \"bogus\""), "{error}");
}

#[test]
fn legacy_repo_with_invalid_slug_fails() {
    let error = load_error("legacy-invalid-slug", r#"{"repos": ["bogus"]}"#);

    assert!(error.contains("invalid repos[0].slug"), "{error}");
    assert!(error.contains("expected exactly owner/repo"));
}

#[test]
fn structured_repo_rejects_path_traversal_slug() {
    let error = load_error("path-traversal-slug", r#"{"repos": [{"slug": "acme/.."}]}"#);

    assert!(error.contains("invalid repos[0].slug"), "{error}");
    assert!(error.contains("path traversal"));
}

#[test]
fn slug_with_extra_segment_fails() {
    let error = load_error("extra-segment-slug", r#"{"repos": ["acme/widgets/extra"]}"#);

    assert!(error.contains("invalid repos[0].slug"), "{error}");
    assert!(error.contains("expected exactly owner/repo"));
}

#[test]
fn slug_with_disallowed_char_fails() {
    let error = load_error("bad-char-slug", r#"{"repos": ["acme/wid gets"]}"#);

    assert!(error.contains("invalid repos[0].slug"), "{error}");
    assert!(error.contains("only ASCII letters"));
}

#[test]
fn invalid_default_group_by_fails() {
    let error = load_error(
        "invalid-group-by",
        r#"{"ui": {"defaultGroupBy": "../bad"}}"#,
    );

    assert!(error.contains("invalid ui.defaultGroupBy"));
}

#[test]
fn invalid_worktree_root_fails() {
    let error = load_error("invalid-worktree-root", r#"{"worktreeRoot": ""}"#);

    assert!(error.contains("invalid worktreeRoot"));
    assert!(error.contains("must not be empty"));
}

#[test]
fn invalid_main_worktree_path_fails() {
    let error = load_error(
        "invalid-main-worktree-path",
        r#"{"repos": [{"slug": "acme/widgets", "mainWorktreePath": "bad\npath"}]}"#,
    );

    assert!(error.contains("invalid repos[0].mainWorktreePath"));
    assert!(error.contains("control characters"));
}

#[test]
fn repo_object_round_trips_every_field() {
    let path = temp_path("round-trip");
    fs::write(
        &path,
        r#"{"repos": [
            {"slug": "acme/widgets", "mainWorktreePath": "~/src/widgets", "worktreeRoot": "/wt", "wayfinderRoots": ["docs/wayfinder", "/abs/maps"]},
            {"mainWorktreePath": "~/src/local-only"}
        ]}"#,
    )
    .expect("write config");

    let config = load_from_path(path.clone()).expect("config should load");
    let _ = fs::remove_file(path);

    assert_eq!(
        config.repos,
        vec![
            RepoConfig {
                slug: Some("acme/widgets".to_owned()),
                main_worktree_path: Some("~/src/widgets".to_owned()),
                worktree_root: Some("/wt".to_owned()),
                wayfinder_roots: Some(vec!["docs/wayfinder".to_owned(), "/abs/maps".to_owned()]),
            },
            RepoConfig {
                slug: None,
                main_worktree_path: Some("~/src/local-only".to_owned()),
                worktree_root: None,
                wayfinder_roots: None,
            },
        ]
    );

    let json = serde_json::to_value(&config).expect("serialize");
    let reparsed: LegitConfig = serde_json::from_value(json).expect("reparse");
    assert_eq!(reparsed, config);
}

#[test]
fn repo_object_without_slug_or_main_worktree_path_fails() {
    let error = load_error(
        "neither-slug-nor-path",
        r#"{"repos": [{"worktreeRoot": "/wt"}]}"#,
    );

    assert!(error.contains("invalid repos[0]:"), "{error}");
    assert!(
        error.contains("at least one of slug or mainWorktreePath"),
        "{error}"
    );
}

#[test]
fn slug_less_repo_rejects_worktree_root() {
    let error = load_error(
        "slug-less-worktree-root",
        r#"{"repos": [{"mainWorktreePath": "/src/local", "worktreeRoot": "/wt"}]}"#,
    );

    assert!(error.contains("invalid repos[0].worktreeRoot"), "{error}");
    assert!(error.contains("requires a slug"), "{error}");
}

#[test]
fn slug_less_repo_validates_its_main_worktree_path() {
    let error = load_error(
        "slug-less-empty-path",
        r#"{"repos": [{"mainWorktreePath": "  "}]}"#,
    );

    assert!(
        error.contains("invalid repos[0].mainWorktreePath"),
        "{error}"
    );
    assert!(error.contains("must not be empty"), "{error}");
}

#[test]
fn invalid_wayfinder_root_entry_fails() {
    let error = load_error(
        "invalid-wayfinder-root",
        r#"{"repos": [{"slug": "acme/widgets", "wayfinderRoots": ["docs/wayfinder", ""]}]}"#,
    );

    assert!(
        error.contains("invalid repos[0].wayfinderRoots[1]"),
        "{error}"
    );
    assert!(error.contains("must not be empty"), "{error}");
}

#[test]
fn legacy_source_clone_names_the_rename() {
    let error = load_error(
        "legacy-source-clone",
        r#"{"repos": ["acme/gizmos", {"slug": "acme/widgets", "sourceClone": "/src/widgets"}]}"#,
    );

    assert!(
        error.contains("repos[1]: `sourceClone` was renamed to `mainWorktreePath`"),
        "{error}"
    );
    assert!(!error.contains("unknown field"), "{error}");
}

#[test]
fn unknown_top_level_field_fails() {
    let error = load_error("unknown-top-level", r#"{"usr": "mayfield"}"#);

    assert!(error.contains("unknown field"), "{error}");
    assert!(error.contains("usr"), "{error}");
}

#[test]
fn unknown_ui_field_fails() {
    let error = load_error("unknown-ui-field", r#"{"ui": {"defaultSortByy": "age"}}"#);

    assert!(error.contains("unknown field"), "{error}");
    assert!(error.contains("defaultSortByy"), "{error}");
}

#[test]
fn repo_object_with_unknown_field_fails() {
    let error = load_error(
        "unknown-repo-field",
        r#"{"repos": ["acme/gizmos", {"slug": "acme/widgets", "mainWorktreePth": "/src"}]}"#,
    );

    assert!(
        error.contains("repos[1]: unknown field `mainWorktreePth`"),
        "{error}"
    );
    assert!(
        error.contains("expected one of `slug`, `mainWorktreePath`"),
        "{error}"
    );
}

#[test]
fn repo_object_with_duplicate_key_fails() {
    let error = load_error(
        "duplicate-repo-key",
        r#"{"repos": [{"slug": "acme/widgets", "slug": "acme/gadgets"}]}"#,
    );

    assert!(
        error.contains("repos[0]: duplicate field `slug`"),
        "{error}"
    );
}

#[test]
fn repo_field_shape_error_names_the_full_key_path() {
    let error = load_error(
        "wrong-typed-repo-field",
        r#"{"repos": [{"slug": "acme/widgets", "wayfinderRoots": "docs"}]}"#,
    );

    assert!(error.contains("repos[0].wayfinderRoots:"), "{error}");
    assert!(error.contains("invalid type"), "{error}");
}

#[test]
fn repo_entry_of_wrong_type_fails() {
    let error = load_error("repo-entry-number", r#"{"repos": ["acme/widgets", 42]}"#);

    assert!(error.contains("repos[1]:"), "{error}");
    assert!(
        error.contains("expected an \"owner/repo\" string or a repo object"),
        "{error}"
    );
}

#[test]
fn has_any_worktree_root_includes_global_and_per_repo_roots() {
    let mut config = LegitConfig::default();
    assert!(!config.has_any_worktree_root());

    config.worktree_root = Some("/global".to_owned());
    assert!(config.has_any_worktree_root());

    config.worktree_root = None;
    config.repos = vec![RepoConfig {
        slug: Some("acme/widgets".to_owned()),
        worktree_root: Some("/repo".to_owned()),
        ..Default::default()
    }];
    assert!(config.has_any_worktree_root());
}

#[test]
fn display_name_prefers_slug_then_expanded_basename() {
    let with_slug = RepoConfig {
        slug: Some("acme/widgets".to_owned()),
        main_worktree_path: Some("~/src/widgets".to_owned()),
        ..Default::default()
    };
    assert_eq!(with_slug.display_name().expect("slug"), "acme/widgets");

    let slug_less = RepoConfig {
        main_worktree_path: Some("~/src/local-only/".to_owned()),
        ..Default::default()
    };
    assert_eq!(slug_less.display_name().expect("basename"), "local-only");

    // `~` alone names the home directory, so the basename is home's, not "~".
    let home = RepoConfig {
        main_worktree_path: Some("~".to_owned()),
        ..Default::default()
    };
    let expected = home_dir()
        .expect("home directory")
        .file_name()
        .expect("home has a basename")
        .to_string_lossy()
        .into_owned();
    assert_eq!(home.display_name().expect("home basename"), expected);

    // A path with no final component (`..` is a parent walk, not a name)
    // falls back to the whole path rather than failing or showing "..".
    let parent_walk = RepoConfig {
        main_worktree_path: Some("/src/widgets/..".to_owned()),
        ..Default::default()
    };
    assert_eq!(
        parent_walk.display_name().expect("fallback"),
        "/src/widgets/.."
    );
}

#[test]
fn identity_is_the_slug_or_the_canonical_main_worktree() {
    let with_slug = RepoConfig {
        slug: Some("acme/widgets".to_owned()),
        main_worktree_path: Some("~/src/widgets".to_owned()),
        ..Default::default()
    };
    assert_eq!(
        with_slug.identity().expect("slug identity"),
        RepoIdentity::Slug("acme/widgets".to_owned())
    );
    // GitHub slugs are case-insensitive, and so is slug identity.
    assert_eq!(
        with_slug.identity().expect("slug identity"),
        RepoIdentity::Slug("Acme/Widgets".to_owned())
    );
    assert_ne!(
        RepoIdentity::Slug("acme/widgets".to_owned()),
        RepoIdentity::Path(PathBuf::from("acme/widgets"))
    );

    let dir = temp_path("identity").with_extension("");
    let nested = dir.join("nested");
    fs::create_dir_all(&nested).expect("create dir");

    let spelled_indirectly = RepoConfig {
        main_worktree_path: Some(nested.join("..").join("nested").display().to_string()),
        ..Default::default()
    };
    assert_eq!(
        spelled_indirectly.identity().expect("canonical path"),
        RepoIdentity::Path(fs::canonicalize(&nested).expect("canonical nested"))
    );

    // Not cloned yet: identity is the expanded path, not an error.
    let missing = dir.join("missing");
    let not_yet_cloned = RepoConfig {
        main_worktree_path: Some(missing.display().to_string()),
        ..Default::default()
    };
    assert_eq!(
        not_yet_cloned.identity().expect("uncanonicalized path"),
        RepoIdentity::Path(missing)
    );

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn home_expansion_requires_home() {
    let error = home_dir_from(None).expect_err("missing HOME should fail");

    assert_eq!(error.to_string(), "HOME is not set");
}

#[test]
fn empty_home_is_treated_as_missing() {
    let error = home_dir_from(Some(OsString::new())).expect_err("empty HOME should fail");

    assert_eq!(error.to_string(), "HOME is not set");
}

#[test]
fn tilde_config_paths_require_home() {
    let error = resolve_config_path_with(
        "~/src/widgets",
        || anyhow::bail!("HOME is not set"),
        || Ok(PathBuf::from("/cwd")),
    )
    .expect_err("tilde path without HOME should fail");

    assert!(error.to_string().contains("HOME is not set"));
}

#[test]
fn relative_config_paths_require_current_dir() {
    let error = resolve_config_path_with(
        "src/widgets",
        || Ok(PathBuf::from("/home/me")),
        || Err(io::Error::new(io::ErrorKind::NotFound, "cwd was deleted")),
    )
    .expect_err("relative path without current dir should fail");

    assert!(
        error
            .to_string()
            .contains("failed to resolve current directory")
    );
}

fn temp_path(name: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("legit-rs-{name}-{nanos}.json"))
}

fn load_error(name: &str, raw: &str) -> String {
    let path = temp_path(name);
    fs::write(&path, raw).expect("write config");

    let error = load_from_path(path.clone()).expect_err("config should fail");
    let _ = fs::remove_file(path);

    format!("{error:#}")
}
