use std::fmt;

use tokio::sync::mpsc;

use crate::{
    app::msg::Msg,
    auth, config, git_remote,
    github::rest::{GitHubRest, OctocrabRest},
};

#[derive(Clone, PartialEq, Eq)]
pub enum Cmd {
    LoadConfig,
    ResolveAuthToken,
    DetectRepo,
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
            Self::DetectRepo => f.write_str("DetectRepo"),
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
pub async fn run(cmd: Cmd, tx: mpsc::UnboundedSender<Msg>) {
    tracing::info!("started");
    match cmd {
        Cmd::LoadConfig => {
            let msg = match blocking(config::load).await {
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
            };
            let _ = tx.send(msg);
        }
        Cmd::ResolveAuthToken => {
            let msg = match blocking(auth::resolve_token).await {
                Ok(token) => {
                    tracing::info!("auth token resolved from gh cli");
                    Msg::AuthTokenResolved(token)
                }
                Err(error) => Msg::CommandFailed {
                    context: "resolve auth token",
                    error: error.to_string(),
                },
            };
            let _ = tx.send(msg);
        }
        Cmd::DetectRepo => {
            let result = blocking(|| {
                let cwd = std::env::current_dir()?;
                git_remote::detect_repo(&cwd)
            })
            .await;
            let msg = match result {
                Ok(repo) => {
                    tracing::info!(owner = %repo.owner, repo = %repo.repo, "repo detected");
                    Msg::RepoDetected(repo)
                }
                Err(error) => Msg::CommandFailed {
                    context: "detect repo",
                    error: error.to_string(),
                },
            };
            let _ = tx.send(msg);
        }
        Cmd::FetchOpenPRs { owner, repo, token } => {
            run_fetch_open_prs(owner, repo, token, tx).await;
        }
    }
}

async fn run_fetch_open_prs(
    owner: String,
    repo: String,
    token: String,
    tx: mpsc::UnboundedSender<Msg>,
) {
    let client = match OctocrabRest::new(&token) {
        Ok(client) => client,
        Err(error) => {
            let _ = tx.send(Msg::PrListFailed {
                context: "build github client",
                error: error.to_string(),
            });
            return;
        }
    };

    let (pr_tx, mut pr_rx) = mpsc::unbounded_channel();
    let runtime_tx = tx.clone();
    let forwarder = tokio::spawn(async move {
        while let Some(pr) = pr_rx.recv().await {
            if runtime_tx.send(Msg::PrArrived(pr)).is_err() {
                break;
            }
        }
    });

    let result = client.list_open_prs(&owner, &repo, pr_tx).await;
    let _ = forwarder.await;

    match result {
        Ok(()) => {
            tracing::info!(%owner, %repo, "open PR listing finished");
            let _ = tx.send(Msg::PrListLoaded);
        }
        Err(error) => {
            let _ = tx.send(Msg::PrListFailed {
                context: "list open PRs",
                error: error.to_string(),
            });
        }
    }
}

/// Run `f` on the blocking pool and fold the `JoinError` into the same
/// `anyhow::Result` the inner function returns. Lets call sites stay
/// 2-arm (`Ok(value)` / `Err(error)`) instead of the 3-arm shape
/// `spawn_blocking().await` would force (`Ok(Ok)` / `Ok(Err)` / `Err`).
async fn blocking<F, T>(f: F) -> anyhow::Result<T>
where
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f).await?
}
