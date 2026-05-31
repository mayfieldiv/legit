use std::sync::Arc;

use tokio::sync::mpsc;

use crate::{
    app::msg::Msg, auth, config, git_remote, github::graphql::GraphQlClient,
    github::limiter::NetworkLimiter, github::rest::OctocrabRest, secret::Secret,
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
    FetchReviewStatus {
        owner: String,
        repo: String,
        token: Secret<String>,
        pr_numbers: Vec<u64>,
    },
    FetchThreads {
        owner: String,
        repo: String,
        token: Secret<String>,
        number: u64,
        bot_logins: Vec<String>,
    },
    FetchReviews {
        owner: String,
        repo: String,
        token: Secret<String>,
        number: u64,
    },
    FetchIssueComments {
        owner: String,
        repo: String,
        token: Secret<String>,
        number: u64,
        bot_logins: Vec<String>,
    },
    FetchChecks {
        owner: String,
        repo: String,
        token: Secret<String>,
        head_sha: String,
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
        Cmd::FetchReviewStatus {
            owner,
            repo,
            token,
            pr_numbers,
        } => {
            let client = match GraphQlClient::new(&token) {
                Ok(client) => client,
                Err(error) => {
                    let _ = tx.send(review_status_failed("build graphql client", error));
                    return;
                }
            };
            let _permit = limiter.acquire().await;
            match client.fetch_review_status(&owner, &repo, &pr_numbers).await {
                Ok(results) => {
                    for (pr_number, status) in results {
                        let _ = tx.send(Msg::ReviewStatusArrived { pr_number, status });
                    }
                }
                Err(error) => {
                    let _ = tx.send(review_status_failed("fetch review status", error));
                }
            }
        }
        Cmd::FetchThreads {
            owner,
            repo,
            token,
            number,
            bot_logins,
        } => {
            let client = match GraphQlClient::new(&token) {
                Ok(client) => client,
                Err(error) => {
                    let _ = tx.send(enrichment_failed(
                        Area::Threads,
                        "build graphql client",
                        error,
                    ));
                    return;
                }
            };
            let _permit = limiter.acquire().await;
            match client
                .fetch_review_threads(&owner, &repo, number, &bot_logins)
                .await
            {
                Ok(threads) => {
                    let _ = tx.send(Msg::ThreadsArrived {
                        pr_number: number,
                        threads,
                    });
                }
                Err(error) => {
                    let _ = tx.send(enrichment_failed(
                        Area::Threads,
                        "fetch review threads",
                        error,
                    ));
                }
            }
        }
        Cmd::FetchReviews {
            owner,
            repo,
            token,
            number,
        } => {
            let client = match OctocrabRest::new(&token) {
                Ok(client) => client,
                Err(error) => {
                    let _ = tx.send(enrichment_failed(
                        Area::Reviews,
                        "build github client",
                        error,
                    ));
                    return;
                }
            };
            let _permit = limiter.acquire().await;
            match client.list_reviews(&owner, &repo, number).await {
                Ok(reviews) => {
                    let _ = tx.send(Msg::ReviewsArrived {
                        pr_number: number,
                        reviews,
                    });
                }
                Err(error) => {
                    let _ = tx.send(enrichment_failed(Area::Reviews, "fetch reviews", error));
                }
            }
        }
        Cmd::FetchIssueComments {
            owner,
            repo,
            token,
            number,
            bot_logins,
        } => {
            let client = match OctocrabRest::new(&token) {
                Ok(client) => client,
                Err(error) => {
                    let _ = tx.send(enrichment_failed(
                        Area::IssueComments,
                        "build github client",
                        error,
                    ));
                    return;
                }
            };
            let _permit = limiter.acquire().await;
            match client
                .list_issue_comments(&owner, &repo, number, &bot_logins)
                .await
            {
                Ok(comments) => {
                    let _ = tx.send(Msg::IssueCommentsArrived {
                        pr_number: number,
                        comments,
                    });
                }
                Err(error) => {
                    let _ = tx.send(enrichment_failed(
                        Area::IssueComments,
                        "fetch issue comments",
                        error,
                    ));
                }
            }
        }
        Cmd::FetchChecks {
            owner,
            repo,
            token,
            head_sha,
        } => {
            let client = match OctocrabRest::new(&token) {
                Ok(client) => client,
                Err(error) => {
                    let _ = tx.send(enrichment_failed(
                        Area::Checks,
                        "build github client",
                        error,
                    ));
                    return;
                }
            };
            let _permit = limiter.acquire().await;
            match client.list_check_runs(&owner, &repo, &head_sha).await {
                Ok(checks) => {
                    let _ = tx.send(Msg::ChecksArrived { head_sha, checks });
                }
                Err(error) => {
                    let _ = tx.send(enrichment_failed(Area::Checks, "fetch check runs", error));
                }
            }
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

/// Which enrichment area a failed fetch belongs to. Selects the per-area
/// failure `Msg` so the model can attribute errors precisely.
enum Area {
    Threads,
    Reviews,
    Checks,
    IssueComments,
}

fn review_status_failed(context: &'static str, error: anyhow::Error) -> Msg {
    let error = error.to_string();
    tracing::warn!(context, %error, "review status fetch failed");
    Msg::ReviewStatusFailed { context, error }
}

fn enrichment_failed(area: Area, context: &'static str, error: anyhow::Error) -> Msg {
    let error = error.to_string();
    tracing::warn!(context, %error, "enrichment fetch failed");
    match area {
        Area::Threads => Msg::ThreadsFailed { context, error },
        Area::Reviews => Msg::ReviewsFailed { context, error },
        Area::Checks => Msg::ChecksFailed { context, error },
        Area::IssueComments => Msg::IssueCommentsFailed { context, error },
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
