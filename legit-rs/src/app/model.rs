use std::fmt;

use crate::{config::LegitConfig, github::rest::PR};

use super::cmd::Cmd;

#[derive(Clone)]
pub struct Model {
    pub should_quit: bool,
    pub config: LegitConfig,
    pub auth_token: Option<String>,
    pub prs: Vec<PR>,
    pub list_error: Option<String>,
    pub last_error: Option<String>,
}

impl fmt::Debug for Model {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Model")
            .field("should_quit", &self.should_quit)
            .field("config", &self.config)
            .field(
                "auth_token",
                &self.auth_token.as_ref().map(|_| "<redacted>"),
            )
            .field("prs", &self.prs.len())
            .field("list_error", &self.list_error)
            .field("last_error", &self.last_error)
            .finish()
    }
}

impl Model {
    pub fn new() -> (Self, Vec<Cmd>) {
        (
            Self {
                should_quit: false,
                config: LegitConfig::default(),
                auth_token: None,
                prs: Vec::new(),
                list_error: None,
                last_error: None,
            },
            vec![Cmd::LoadConfig, Cmd::ResolveAuthToken],
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::app::model::Model;

    #[test]
    fn debug_redacts_auth_token() {
        let (mut model, _) = Model::new();
        model.auth_token = Some("secret-token".to_owned());

        let debug = format!("{model:?}");

        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("secret-token"));
    }
}
