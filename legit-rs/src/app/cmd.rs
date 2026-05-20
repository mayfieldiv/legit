use tokio::sync::mpsc;

use crate::{app::msg::Msg, auth, config};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    LoadConfig,
    ResolveAuthToken,
}

pub async fn run(cmd: Cmd, tx: mpsc::UnboundedSender<Msg>) {
    let msg = match cmd {
        Cmd::LoadConfig => match config::load() {
            Ok(config) => Msg::ConfigLoaded(config),
            Err(error) => Msg::CommandFailed {
                context: "load config",
                error: error.to_string(),
            },
        },
        Cmd::ResolveAuthToken => match auth::resolve_token() {
            Ok(token) => Msg::AuthTokenResolved(token),
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
