use crate::config::LegitConfig;

use super::cmd::Cmd;

#[derive(Debug, Clone)]
pub struct Model {
    pub should_quit: bool,
    pub config: LegitConfig,
    pub auth_token: Option<String>,
    pub last_error: Option<String>,
}

impl Model {
    pub fn new() -> (Self, Vec<Cmd>) {
        (
            Self {
                should_quit: false,
                config: LegitConfig::default(),
                auth_token: None,
                last_error: None,
            },
            vec![Cmd::LoadConfig, Cmd::ResolveAuthToken],
        )
    }
}
