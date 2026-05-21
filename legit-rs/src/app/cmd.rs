use std::fmt;

use tokio::sync::mpsc;

use crate::{app::msg::Msg, auth, config};

#[derive(Clone, PartialEq, Eq)]
pub enum Cmd {
    LoadConfig,
    ResolveAuthToken,
    FetchOpenPRs {
        owner: String,
        repo: String,
        token: String,
    },
}

impl fmt::Debug for Cmd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LoadConfig => f.write_str("LoadConfig"),
            Self::ResolveAuthToken => f.write_str("ResolveAuthToken"),
            Self::FetchOpenPRs { owner, repo, .. } => f
                .debug_struct("FetchOpenPRs")
                .field("owner", owner)
                .field("repo", repo)
                .field("token", &"<redacted>")
                .finish(),
        }
    }
}

#[tracing::instrument(name = "command", skip(tx))]
pub fn run(cmd: Cmd, tx: mpsc::UnboundedSender<Msg>) {
    tracing::info!("started");
    let msg = match cmd {
        Cmd::LoadConfig => match config::load() {
            Ok(config) => {
                tracing::info!(
                    repos = config.repos.len(),
                    bot_logins = config.bot_logins.len(),
                    file_rules = config.file_rules.len(),
                    has_user = !config.user.is_empty(),
                    has_worktree_root = config.has_any_worktree_root(),
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
        Cmd::FetchOpenPRs { .. } => Msg::PrListFailed {
            context: "list open PRs",
            error: "fetch transport not yet wired".to_owned(),
        },
    };

    if tx.send(msg).is_err() {
        tracing::debug!("command result dropped because runtime channel closed");
    }
}
