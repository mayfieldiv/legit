use std::{env, fs, path::PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRule {
    pub pattern: String,
    pub category: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoConfig {
    pub slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_clone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_root: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

pub fn load() -> Result<LegitConfig> {
    let path = config_path()?;
    tracing::info!(path = %path.display(), "loading config");
    load_from_path(path)
}

pub fn load_from_path(path: PathBuf) -> Result<LegitConfig> {
    match fs::read_to_string(&path) {
        Ok(raw) => {
            tracing::debug!(path = %path.display(), bytes = raw.len(), "config file read");
            serde_json::from_str(&raw)
                .with_context(|| format!("failed to parse {}", path.display()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!(path = %path.display(), "config missing; using defaults");
            Ok(LegitConfig::default())
        }
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

pub fn config_path() -> Result<PathBuf> {
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
    let entries = Vec::<RepoEntry>::deserialize(deserializer)?;
    Ok(entries
        .into_iter()
        .filter_map(|entry| match entry {
            RepoEntry::Legacy(slug) if slug.contains('/') => Some(RepoConfig {
                slug,
                ..RepoConfig::default()
            }),
            RepoEntry::Structured(repo) if repo.slug.contains('/') => Some(repo),
            _ => None,
        })
        .collect())
}

#[derive(Deserialize)]
#[serde(untagged, rename_all = "camelCase")]
enum RepoEntry {
    Legacy(String),
    Structured(RepoConfig),
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{LegitConfig, load_from_path};

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
                "repos": ["acme/widgets", "bogus", {"slug": "acme/gadgets", "sourceClone": "/src/gadgets"}, {"slug": "no-slash"}],
                "ui": {"defaultSortBy": "created"}
            }"#,
        )
        .expect("write config");

        let config = load_from_path(path.clone()).expect("config should load");
        let _ = fs::remove_file(path);

        assert_eq!(config.user, "mayfield");
        assert_eq!(config.repos.len(), 2);
        assert_eq!(config.repos[0].slug, "acme/widgets");
        assert_eq!(config.repos[1].slug, "acme/gadgets");
        assert_eq!(
            config.repos[1].source_clone.as_deref(),
            Some("/src/gadgets")
        );
        assert_eq!(config.bot_logins, LegitConfig::default().bot_logins);
        assert_eq!(config.ui.default_group_by, "smart-status");
        assert_eq!(config.ui.default_sort_by, "created");
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("legit-rs-{name}-{nanos}.json"))
    }
}
