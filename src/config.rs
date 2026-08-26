use std::{env, fs, path::PathBuf};

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize, de::Error as _};

// TODO: when the group/filter engine is ported from
// `src/lib/group-filter-engine.ts`, derive these from the canonical
// GroupBy/SortBy enums instead of maintaining loose string lists here.
const VALID_GROUP_BY: &[&str] = &[
    "smart-status",
    "author",
    "repo",
    "size-category",
    "label",
    "none",
];
const VALID_SORT_BY: &[&str] = &["size", "age", "updated"];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileRule {
    pub pattern: String,
    pub category: String,
}

/// One Tracked Repo. At least one of `slug` / `main_worktree_path` is set
/// (enforced by `validate`): a slug makes the repo PR-capable, a Main Worktree
/// enables worktree features and local Effort discovery. A slug-less entry is
/// a local-only repo — no Repo Tab, no PR machinery — so `worktree_root` is
/// rejected on it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepoConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_worktree_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_root: Option<String>,
    /// Wayfinder Roots for this repo's local Effort discovery. `None` means the
    /// built-in probe list; `Some` *replaces* that list (never extends it), so
    /// an empty list opts the repo out of local discovery entirely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wayfinder_roots: Option<Vec<String>>,
}

impl RepoConfig {
    /// `index` is the entry's position in `repos`, the one name every entry
    /// has (slug and mainWorktreePath are both optional).
    fn validate(&self, index: usize) -> anyhow::Result<()> {
        let label = format!("repos[{index}]");
        ensure!(
            self.slug.is_some() || self.main_worktree_path.is_some(),
            "invalid {label}: at least one of slug or mainWorktreePath is required"
        );
        if let Some(slug) = &self.slug {
            validate_repo_slug(slug)?;
        }
        if let Some(path) = &self.main_worktree_path {
            validate_path(&format!("{label}.mainWorktreePath"), path)?;
        }
        if let Some(path) = &self.worktree_root {
            ensure!(
                self.slug.is_some(),
                "invalid {label}.worktreeRoot: requires a slug (a slug-less repo has no PRs to create worktrees for)"
            );
            validate_path(&format!("{label}.worktreeRoot"), path)?;
        }
        for (root_index, root) in self.wayfinder_roots.iter().flatten().enumerate() {
            validate_path(&format!("{label}.wayfinderRoots[{root_index}]"), root)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiConfig {
    #[serde(default = "default_group_by")]
    pub default_group_by: String,
    #[serde(default = "default_sort_by")]
    pub default_sort_by: String,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            default_group_by: default_group_by(),
            default_sort_by: default_sort_by(),
        }
    }
}

impl UiConfig {
    fn validate(&self) -> anyhow::Result<()> {
        validate_allowed_value("ui.defaultGroupBy", &self.default_group_by, VALID_GROUP_BY)?;
        validate_allowed_value("ui.defaultSortBy", &self.default_sort_by, VALID_SORT_BY)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegitConfig {
    #[serde(default)]
    pub user: String,
    #[serde(default, deserialize_with = "deserialize_repos")]
    pub repos: Vec<RepoConfig>,
    #[serde(default = "default_bot_logins")]
    pub bot_logins: Vec<String>,
    #[serde(default)]
    pub file_rules: Vec<FileRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_root: Option<String>,
    #[serde(default)]
    pub ui: UiConfig,
}

impl Default for LegitConfig {
    fn default() -> Self {
        Self {
            user: String::new(),
            repos: Vec::new(),
            bot_logins: default_bot_logins(),
            file_rules: Vec::new(),
            worktree_root: None,
            ui: UiConfig::default(),
        }
    }
}

impl LegitConfig {
    pub fn has_any_worktree_root(&self) -> bool {
        self.worktree_root.is_some() || self.repos.iter().any(|repo| repo.worktree_root.is_some())
    }

    fn validate(&self) -> anyhow::Result<()> {
        for (index, repo) in self.repos.iter().enumerate() {
            repo.validate(index)?;
        }
        if let Some(path) = &self.worktree_root {
            validate_path("worktreeRoot", path)?;
        }
        self.ui.validate()
    }
}

#[tracing::instrument(name = "load_config")]
pub fn load() -> anyhow::Result<LegitConfig> {
    let path = config_path()?;
    tracing::info!(path = %path.display(), "loading config");
    load_from_path(path)
}

pub fn load_from_path(path: PathBuf) -> anyhow::Result<LegitConfig> {
    match fs::read_to_string(&path) {
        Ok(raw) => {
            tracing::debug!(path = %path.display(), bytes = raw.len(), "config file read");
            let config: LegitConfig = serde_json::from_str(&raw)
                .with_context(|| format!("failed to parse {}", path.display()))?;
            config
                .validate()
                .with_context(|| format!("failed to validate {}", path.display()))?;
            Ok(config)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!(path = %path.display(), "config missing; using defaults");
            Ok(LegitConfig::default())
        }
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

pub fn config_path() -> anyhow::Result<PathBuf> {
    if let Some(path) = env::var_os("LEGIT_CONFIG_PATH") {
        return Ok(PathBuf::from(path));
    }

    let home = env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".legit/config.json"))
}

fn default_group_by() -> String {
    "smart-status".to_owned()
}

fn default_sort_by() -> String {
    "updated".to_owned()
}

fn default_bot_logins() -> Vec<String> {
    vec![
        "app/devin-ai-integration".to_owned(),
        "app/copilot-swe-agent".to_owned(),
    ]
}

fn deserialize_repos<'de, D>(deserializer: D) -> Result<Vec<RepoConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // Each entry is buffered as a JSON value so its array index can prefix
    // every error (`repos[2]: …`) — the only stable name an entry has, since
    // slug and mainWorktreePath are both optional. Validation is
    // intentionally left to `LegitConfig::validate`, run in `load_from_path`, so
    // every invalid entry surfaces one consistent "failed to validate" error
    // rather than being silently dropped.
    let entries = Vec::<serde_json::Value>::deserialize(deserializer)?;
    entries
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            repo_from_value(value)
                .map_err(|error| D::Error::custom(format!("repos[{index}]: {error}")))
        })
        .collect()
}

/// The pre-rename key for `mainWorktreePath`, still reported by name so an old
/// config fails with a precise migration error.
const LEGACY_MAIN_WORKTREE_KEY: &str = "sourceClone";

/// A single `repos` entry: either a bare `"owner/repo"` string (legacy form)
/// or a structured object, normalised to `RepoConfig` either way. Hand-written
/// instead of `#[serde(untagged)]` so a typo'd object key surfaces serde's
/// precise `unknown field` error (via `RepoConfig`'s `deny_unknown_fields`)
/// rather than the untagged enum's opaque "did not match any variant".
fn repo_from_value(value: serde_json::Value) -> Result<RepoConfig, serde_json::Error> {
    match value {
        serde_json::Value::String(slug) => Ok(RepoConfig {
            slug: Some(slug),
            ..RepoConfig::default()
        }),
        serde_json::Value::Object(object) => {
            // Checked before deserializing so the retired key is reported as a
            // rename instead of serde's generic `unknown field` — that one-line
            // error is the migration path.
            if object.contains_key(LEGACY_MAIN_WORKTREE_KEY) {
                return Err(serde_json::Error::custom(format!(
                    "`{LEGACY_MAIN_WORKTREE_KEY}` was renamed to `mainWorktreePath`; update the key and reload"
                )));
            }
            RepoConfig::deserialize(serde_json::Value::Object(object))
        }
        other => Err(serde_json::Error::custom(format!(
            "expected an \"owner/repo\" string or a repo object, found {other}"
        ))),
    }
}

fn validate_repo_slug(slug: &str) -> anyhow::Result<()> {
    // `repo` holds everything after the first '/', so `repo.contains('/')`
    // rejects three-or-more-segment slugs alongside the empty-segment cases.
    let (owner, repo) = slug.split_once('/').unwrap_or_default();
    ensure!(
        !owner.is_empty() && !repo.is_empty() && !repo.contains('/'),
        "invalid repo slug {slug:?}: expected exactly owner/repo"
    );

    for part in [owner, repo] {
        ensure!(
            part != "." && part != "..",
            "invalid repo slug {slug:?}: path traversal segments are not allowed"
        );
        ensure!(
            part.chars().all(is_repo_slug_char),
            "invalid repo slug {slug:?}: only ASCII letters, numbers, '.', '_', and '-' are allowed"
        );
    }

    Ok(())
}

fn is_repo_slug_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-')
}

fn validate_path(field: &str, path: &str) -> anyhow::Result<()> {
    ensure!(
        !path.trim().is_empty(),
        "invalid {field} {path:?}: must not be empty"
    );
    // `char::is_control` already covers NUL and every other C0/C1 control
    // character, none of which round-trip through OS path APIs.
    ensure!(
        !path.chars().any(char::is_control),
        "invalid {field} {path:?}: must not contain control characters"
    );

    Ok(())
}

fn validate_allowed_value(field: &str, value: &str, allowed: &[&str]) -> anyhow::Result<()> {
    ensure!(
        allowed.contains(&value),
        "invalid {field} {value:?}; expected one of {}",
        allowed.join(", ")
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{LegitConfig, RepoConfig, load_from_path};

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

        assert!(error.contains("invalid repo slug"));
        assert!(error.contains("expected exactly owner/repo"));
    }

    #[test]
    fn legacy_repo_with_invalid_slug_fails() {
        let error = load_error("legacy-invalid-slug", r#"{"repos": ["bogus"]}"#);

        assert!(error.contains("invalid repo slug"));
        assert!(error.contains("expected exactly owner/repo"));
    }

    #[test]
    fn structured_repo_rejects_path_traversal_slug() {
        let error = load_error("path-traversal-slug", r#"{"repos": [{"slug": "acme/.."}]}"#);

        assert!(error.contains("invalid repo slug"));
        assert!(error.contains("path traversal"));
    }

    #[test]
    fn slug_with_extra_segment_fails() {
        let error = load_error("extra-segment-slug", r#"{"repos": ["acme/widgets/extra"]}"#);

        assert!(error.contains("invalid repo slug"));
        assert!(error.contains("expected exactly owner/repo"));
    }

    #[test]
    fn slug_with_disallowed_char_fails() {
        let error = load_error("bad-char-slug", r#"{"repos": ["acme/wid gets"]}"#);

        assert!(error.contains("invalid repo slug"));
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
                    wayfinder_roots: Some(vec![
                        "docs/wayfinder".to_owned(),
                        "/abs/maps".to_owned()
                    ]),
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
}
