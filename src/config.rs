use std::{env, ffi::OsString, fs, io, path::PathBuf};

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
///
/// Deserialized by [`RepoEntry`]'s hand-written visitor, not a derive — a new
/// field must also be added to its `visit_map` match and to `REPO_FIELDS`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_worktree_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_root: Option<String>,
    /// Wayfinder Roots for this repo's local Effort discovery. `None` means the
    /// built-in probe list; `Some` *replaces* that list (never extends it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wayfinder_roots: Option<Vec<String>>,
}

/// How a Tracked Repo is told apart from every other: PR-capable repos by slug
/// (GitHub slugs are case-insensitive, so compare with `eq_ignore_ascii_case`),
/// slug-less repos by their Main Worktree path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoIdentity {
    Slug(String),
    /// The expanded, absolute Main Worktree path — canonicalized when the
    /// directory exists, so two entries spelling the same directory differently
    /// (or an entry and the cwd repo) compare equal. A not-yet-cloned Main
    /// Worktree keeps the un-canonicalized path rather than failing: config
    /// validation is shape-only and identity must not be stricter than it.
    Path(PathBuf),
}

impl RepoConfig {
    fn validate(&self, index: usize) -> anyhow::Result<()> {
        let label = repo_label(index);
        ensure!(
            self.slug.is_some() || self.main_worktree_path.is_some(),
            "invalid {label}: at least one of slug or mainWorktreePath is required"
        );
        if let Some(slug) = &self.slug {
            validate_repo_slug(&format!("{label}.slug"), slug)?;
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

    /// The name this repo is shown under: its slug, or for a slug-less repo the
    /// basename of its expanded Main Worktree path.
    // Consumed by the ticket surface; nothing on the PR surface names a
    // slug-less repo.
    #[allow(dead_code)]
    pub fn display_name(&self) -> anyhow::Result<String> {
        if let Some(slug) = &self.slug {
            return Ok(slug.clone());
        }
        let resolved = resolve_config_path(self.main_worktree_path()?)?;
        Ok(resolved
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| resolved.display().to_string()))
    }

    /// See [`RepoIdentity`].
    // Consumed by the ticket surface, which is where slug-less repos dedupe;
    // `Model::tracked_repos` covers the slug case on the PR surface.
    #[allow(dead_code)]
    pub fn identity(&self) -> anyhow::Result<RepoIdentity> {
        if let Some(slug) = &self.slug {
            return Ok(RepoIdentity::Slug(slug.clone()));
        }
        let resolved = resolve_config_path(self.main_worktree_path()?)?;
        Ok(RepoIdentity::Path(
            fs::canonicalize(&resolved).unwrap_or(resolved),
        ))
    }

    /// The slug-less case's one required field. `validate` guarantees it for a
    /// loaded config; this only fails on a hand-built `RepoConfig`.
    fn main_worktree_path(&self) -> anyhow::Result<&str> {
        self.main_worktree_path
            .as_deref()
            .context("repo has neither slug nor mainWorktreePath")
    }
}

/// The one name every `repos` entry has (slug and mainWorktreePath are both
/// optional): its position in the array.
fn repo_label(index: usize) -> String {
    format!("repos[{index}]")
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
    // Validation is intentionally left to `LegitConfig::validate`, run in
    // `load_from_path`, so every invalid entry surfaces one consistent "failed
    // to validate" error rather than being silently dropped.
    struct ReposVisitor;

    impl<'de> serde::de::Visitor<'de> for ReposVisitor {
        type Value = Vec<RepoConfig>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a list of repos")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Vec<RepoConfig>, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut repos = Vec::with_capacity(seq.size_hint().unwrap_or(0));
            loop {
                // Entries are read one at a time (not via `Vec<RepoEntry>`) so
                // each shape error can be prefixed with the entry's index.
                let index = repos.len();
                let entry = seq
                    .next_element::<RepoEntry>()
                    .map_err(|error| A::Error::custom(format!("{}: {error}", repo_label(index))))?;
                match entry {
                    Some(RepoEntry(repo)) => repos.push(repo),
                    None => return Ok(repos),
                }
            }
        }
    }

    deserializer.deserialize_seq(ReposVisitor)
}

/// The pre-rename key for `mainWorktreePath`, still reported by name so an old
/// config fails with a precise migration error.
const LEGACY_MAIN_WORKTREE_KEY: &str = "sourceClone";

/// The object keys a `repos` entry accepts, in the order serde lists them in an
/// `unknown field` error.
const REPO_FIELDS: &[&str] = &["slug", "mainWorktreePath", "worktreeRoot", "wayfinderRoots"];

/// A single `repos` entry: either a bare `"owner/repo"` string (legacy form)
/// or a structured object, normalised to `RepoConfig` either way. Hand-written
/// instead of `#[serde(untagged)]` so a typo'd object key surfaces serde's
/// precise `unknown field` error rather than the untagged enum's opaque "did
/// not match any variant". The object case walks the map itself, rather than
/// delegating to a derived `RepoConfig` impl, so the retired `sourceClone` key
/// can be reported as a rename — that one-line error is the migration path —
/// while duplicate keys still fail as they would under a derive.
struct RepoEntry(RepoConfig);

impl<'de> Deserialize<'de> for RepoEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct RepoEntryVisitor;

        impl<'de> serde::de::Visitor<'de> for RepoEntryVisitor {
            type Value = RepoEntry;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an \"owner/repo\" string or a repo object")
            }

            fn visit_str<E>(self, value: &str) -> Result<RepoEntry, E>
            where
                E: serde::de::Error,
            {
                Ok(RepoEntry(RepoConfig {
                    slug: Some(value.to_owned()),
                    ..RepoConfig::default()
                }))
            }

            fn visit_map<A>(self, mut map: A) -> Result<RepoEntry, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                fn set<'de, A, T>(
                    slot: &mut Option<T>,
                    field: &'static str,
                    map: &mut A,
                ) -> Result<(), A::Error>
                where
                    A: serde::de::MapAccess<'de>,
                    T: Deserialize<'de>,
                {
                    if slot.is_some() {
                        return Err(A::Error::duplicate_field(field));
                    }
                    *slot = Some(map.next_value()?);
                    Ok(())
                }

                let mut repo = RepoConfig::default();
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "slug" => set(&mut repo.slug, "slug", &mut map)?,
                        "mainWorktreePath" => {
                            set(&mut repo.main_worktree_path, "mainWorktreePath", &mut map)?
                        }
                        "worktreeRoot" => set(&mut repo.worktree_root, "worktreeRoot", &mut map)?,
                        "wayfinderRoots" => {
                            set(&mut repo.wayfinder_roots, "wayfinderRoots", &mut map)?
                        }
                        LEGACY_MAIN_WORKTREE_KEY => {
                            return Err(A::Error::custom(format!(
                                "`{LEGACY_MAIN_WORKTREE_KEY}` was renamed to `mainWorktreePath`; update the key and reload"
                            )));
                        }
                        other => return Err(A::Error::unknown_field(other, REPO_FIELDS)),
                    }
                }
                Ok(RepoEntry(repo))
            }
        }

        deserializer.deserialize_any(RepoEntryVisitor)
    }
}

fn validate_repo_slug(field: &str, slug: &str) -> anyhow::Result<()> {
    // `repo` holds everything after the first '/', so `repo.contains('/')`
    // rejects three-or-more-segment slugs alongside the empty-segment cases.
    let (owner, repo) = slug.split_once('/').unwrap_or_default();
    ensure!(
        !owner.is_empty() && !repo.is_empty() && !repo.contains('/'),
        "invalid {field} {slug:?}: expected exactly owner/repo"
    );

    for part in [owner, repo] {
        ensure!(
            part != "." && part != "..",
            "invalid {field} {slug:?}: path traversal segments are not allowed"
        );
        ensure!(
            part.chars().all(is_repo_slug_char),
            "invalid {field} {slug:?}: only ASCII letters, numbers, '.', '_', and '-' are allowed"
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

/// Expand a config path for use: `~` to `$HOME`, relative paths against the
/// cwd. Deliberately not part of validation, which stays shape-only.
pub(crate) fn resolve_config_path(path: &str) -> anyhow::Result<PathBuf> {
    resolve_config_path_with(path, home_dir, env::current_dir)
}

fn resolve_config_path_with(
    path: &str,
    home_dir: impl Fn() -> anyhow::Result<PathBuf>,
    current_dir: impl Fn() -> io::Result<PathBuf>,
) -> anyhow::Result<PathBuf> {
    let expanded = if path == "~" {
        home_dir()?
    } else if let Some(rest) = path.strip_prefix("~/") {
        home_dir()?.join(rest)
    } else {
        PathBuf::from(path)
    };

    if expanded.is_absolute() {
        Ok(expanded)
    } else {
        Ok(current_dir()
            .context("failed to resolve current directory")?
            .join(expanded))
    }
}

pub(crate) fn home_dir() -> anyhow::Result<PathBuf> {
    home_dir_from(env::var_os("HOME"))
}

fn home_dir_from(home: Option<OsString>) -> anyhow::Result<PathBuf> {
    home.filter(|home| !home.as_os_str().is_empty())
        .map(PathBuf::from)
        .context("HOME is not set")
}

#[cfg(test)]
mod tests {
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
}
