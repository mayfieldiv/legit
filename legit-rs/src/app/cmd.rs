use std::sync::Arc;

use tokio::sync::mpsc;

use crate::{
    app::msg::Msg, auth, config, git_remote, github::limiter::NetworkLimiter,
    github::rest::OctocrabRest, secret::Secret,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    LoadConfig,
    ResolveAuthToken,
    DetectRepo,
    FetchOpenPRs {
        owner: String,
        repo: String,
        token: Secret<String>,
    },
}

#[tracing::instrument(name = "command", skip(tx, limiter))]
pub async fn run(cmd: Cmd, tx: mpsc::UnboundedSender<Msg>, limiter: Arc<NetworkLimiter>) {
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
                Err(error) => command_failed("load config", error),
            };
            let _ = tx.send(msg);
        }
        Cmd::ResolveAuthToken => {
            let msg = match blocking(auth::resolve_token).await {
                Ok(token) => {
                    tracing::info!("auth token resolved from gh cli");
                    Msg::AuthTokenResolved(token)
                }
                Err(error) => command_failed("resolve auth token", error),
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
                Err(error) => command_failed("detect repo", error),
            };
            let _ = tx.send(msg);
        }
        Cmd::FetchOpenPRs { owner, repo, token } => {
            run_fetch_open_prs(owner, repo, token, tx, limiter).await;
        }
    }
}

async fn run_fetch_open_prs(
    owner: String,
    repo: String,
    token: Secret<String>,
    tx: mpsc::UnboundedSender<Msg>,
    limiter: Arc<NetworkLimiter>,
) {
    let client = match OctocrabRest::new(&token) {
        Ok(client) => client,
        Err(error) => {
            let _ = tx.send(pr_list_failed("build github client", error));
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

    // Hold a concurrency slot for the duration of the (paginated) listing so the
    // network indicator reflects it and we stay under the shared 8-request cap.
    let _permit = limiter.acquire().await;
    let result = client.list_open_prs(&owner, &repo, pr_tx).await;
    let _ = forwarder.await;

    match result {
        Ok(()) => {
            tracing::info!(%owner, %repo, "open PR listing finished");
            let _ = tx.send(Msg::PrListLoaded);
        }
        Err(error) => {
            let _ = tx.send(pr_list_failed("list open PRs", error));
        }
    }
}

/// Log the failure here (cmd is the impure layer) and build the `Msg` for
/// the runtime to deliver to `update`. Keeps `update` a pure
/// `(Model, Msg) -> Vec<Cmd>` — the warn doesn't have to fire from inside
/// the reducer.
fn command_failed(context: &'static str, error: anyhow::Error) -> Msg {
    let error = error.to_string();
    tracing::warn!(context, %error, "command failed");
    Msg::CommandFailed { context, error }
}

/// Same as `command_failed`, but for the PR-listing pipeline (which has its
/// own `Msg` variant so the status bar can prioritise list errors over
/// generic command errors).
fn pr_list_failed(context: &'static str, error: anyhow::Error) -> Msg {
    let error = error.to_string();
    tracing::warn!(context, %error, "pr listing failed");
    Msg::PrListFailed { context, error }
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
