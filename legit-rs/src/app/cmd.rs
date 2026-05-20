use tokio::sync::mpsc;

use crate::{app::msg::Msg, auth, config};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    LoadConfig,
    ResolveAuthToken,
}

pub fn run(cmd: Cmd, tx: mpsc::UnboundedSender<Msg>) {
    tracing::info!(?cmd, "command started");
    let msg = match cmd {
        Cmd::LoadConfig => match config::load() {
            Ok(config) => {
                tracing::info!(
                    repos = config.repos.len(),
                    bot_logins = config.bot_logins.len(),
                    file_rules = config.file_rules.len(),
                    has_user = !config.user.is_empty(),
                    has_worktree_root = config.worktree_root.is_some(),
                    "config loaded"
                );
                Msg::ConfigLoaded(config)
            }
            Err(error) => Msg::CommandFailed {
                context: "load config",
                error: error.to_string(),
            },
        },
        Cmd::ResolveAuthToken => match auth::resolve_token() {
            Ok(token) => {
                tracing::info!("auth token resolved from gh cli");
                Msg::AuthTokenResolved(token)
            }
            Err(error) => Msg::CommandFailed {
                context: "resolve auth token",
                error: error.to_string(),
            },
        },
    };

    if tx.send(msg).is_err() {
        tracing::debug!("command result dropped because runtime channel closed");
    }
}
