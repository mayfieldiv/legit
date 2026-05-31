use crate::{config::LegitConfig, git_remote::RepoInfo, secret::Secret};

use super::{cmd::Cmd, pr_list::PrList};

#[derive(Clone, Debug)]
pub struct Model {
    pub should_quit: bool,
    pub config: LegitConfig,
    pub auth_token: Option<Secret<String>>,
    pub repo: Option<RepoInfo>,
    pub list: PrList,
    pub last_error: Option<String>,
}

impl Model {
    pub fn new() -> (Self, Vec<Cmd>) {
        (
            Self {
                should_quit: false,
                config: LegitConfig::default(),
                auth_token: None,
                repo: None,
                list: PrList::new(),
                last_error: None,
            },
            vec![Cmd::LoadConfig, Cmd::ResolveAuthToken, Cmd::DetectRepo],
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::{app::model::Model, secret::Secret};

    #[test]
    fn debug_redacts_auth_token() {
        let (mut model, _) = Model::new();
        model.auth_token = Some(Secret::new("secret-token".to_owned()));

        let debug = format!("{model:?}");

        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("secret-token"));
    }
}
