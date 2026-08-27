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
/// [`Deserialize`] is hand-written (see the impl below): an entry may be a
/// bare `"owner/repo"` string, and the retired `sourceClone` key errors by
/// name. A new field must also be added to the `RepoObject` wire struct there.
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
    #[serde(default)]
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
            let config = parse_config(&raw)
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

/// Parse config JSON with `serde_path_to_error` tracking, so every shape error
/// is prefixed with the failing key's config path ("repos[1].slug"); a
/// document-root error carries no prefix. This is the one place `repos` errors
/// get their index — nothing per-entry knows its own position.
fn parse_config(raw: &str) -> anyhow::Result<LegitConfig> {
    let mut deserializer = serde_json::Deserializer::from_str(raw);
    let config = serde_path_to_error::deserialize(&mut deserializer)?;
    // What `serde_json::from_str` would have done: reject trailing content
    // after the JSON document.
    deserializer.end()?;
    Ok(config)
}

pub fn config_path() -> anyhow::Result<PathBuf> {
    if let Some(path) = env::var_os("LEGIT_CONFIG_PATH") {
        return Ok(PathBuf::from(path));
    }

    Ok(home_dir()?.join(".legit/config.json"))
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

/// `RepoConfig`'s wire shape: the real fields plus the retired `sourceClone`
/// key, accepted here only so the impl below can report it as a rename — that
/// one-line error is the migration path — instead of serde's generic `unknown
/// field`. (This also lists `sourceClone` in the unknown-field error's expected
/// keys; harmless, since writing it leads straight to the rename error.)
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RepoObject {
    slug: Option<String>,
    main_worktree_path: Option<String>,
    worktree_root: Option<String>,
    wayfinder_roots: Option<Vec<String>>,
    #[serde(
        default,
        rename = "sourceClone",
        deserialize_with = "present_even_if_null"
    )]
    legacy_source_clone: Option<serde::de::IgnoredAny>,
}

/// `Some` whenever the key is present, `null` value included — a plain
/// `Option` field maps JSON `null` to `None` before the field type sees it,
/// which would let `"sourceClone": null` skip the rename error.
fn present_even_if_null<'de, D>(deserializer: D) -> Result<Option<serde::de::IgnoredAny>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    serde::de::IgnoredAny::deserialize(deserializer).map(Some)
}

/// A `repos` entry is either a bare `"owner/repo"` string (legacy form) or a
/// structured object, normalised to one `RepoConfig` either way. Hand-written
/// instead of `#[serde(untagged)]` so a typo'd object key surfaces serde's
/// precise `unknown field` error (via `RepoObject`'s `deny_unknown_fields`)
/// rather than the untagged enum's opaque "did not match any variant".
/// Validation is intentionally left to `LegitConfig::validate`, run in
/// `load_from_path`, so every invalid entry surfaces one consistent "failed to
/// validate" error rather than being silently dropped; `parse_config`'s path
/// tracking prefixes shape errors with the entry's index.
impl<'de> Deserialize<'de> for RepoConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct RepoConfigVisitor;

        impl<'de> serde::de::Visitor<'de> for RepoConfigVisitor {
            type Value = RepoConfig;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an \"owner/repo\" string or a repo object")
            }

            fn visit_str<E>(self, value: &str) -> Result<RepoConfig, E>
            where
                E: serde::de::Error,
            {
                Ok(RepoConfig {
                    slug: Some(value.to_owned()),
                    ..RepoConfig::default()
                })
            }

            fn visit_map<A>(self, map: A) -> Result<RepoConfig, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let object =
                    RepoObject::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
                if object.legacy_source_clone.is_some() {
                    return Err(A::Error::custom(
                        "`sourceClone` was renamed to `mainWorktreePath`; update the key and reload",
                    ));
                }
                Ok(RepoConfig {
                    slug: object.slug,
                    main_worktree_path: object.main_worktree_path,
                    worktree_root: object.worktree_root,
                    wayfinder_roots: object.wayfinder_roots,
                })
            }
        }

        deserializer.deserialize_any(RepoConfigVisitor)
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
mod tests;
