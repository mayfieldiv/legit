use std::{future::Future, sync::Arc};

use tokio::sync::mpsc;

use crate::{
    app::msg::Msg, auth, config, git_remote, git_remote::RepoInfo, github::graphql::GraphQlClient,
    github::limiter::NetworkLimiter, github::rest::OctocrabRest, github::rest::PR,
    github::rest::PrKey, secret::Secret,
};

/// The inputs every per-PR enrichment request shares: which repo, the auth
/// token, and the configured bot logins (used for comment/thread bot
/// detection). Built once per list-load in `update::enrichment_cmds` and shared
/// by `Arc` so the per-PR fan-out clones one pointer per command instead of the
/// owner/repo/token/bot-login strings each time.
#[derive(Debug, PartialEq, Eq)]
pub struct RequestContext {
    pub repo: RepoInfo,
    pub token: Secret<String>,
    pub bot_logins: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    LoadConfig,
    ResolveAuthToken,
    DetectRepo,
    /// Listing open PRs streams results back one-by-one and runs before the
    /// enrichment fan-out, so it carries only repo + token rather than the
    /// shared `RequestContext` (it has no use for `bot_logins`).
    FetchOpenPRs {
        repo: RepoInfo,
        token: Secret<String>,
    },
    FetchReviewStatus {
        ctx: Arc<RequestContext>,
        pr_numbers: Vec<u64>,
    },
    FetchThreads {
        ctx: Arc<RequestContext>,
        number: u64,
    },
    FetchReviews {
        ctx: Arc<RequestContext>,
        number: u64,
    },
    FetchIssueComments {
        ctx: Arc<RequestContext>,
        number: u64,
    },
    FetchChecks {
        ctx: Arc<RequestContext>,
        head_sha: String,
    },
    /// Fetch one PR's changed files (additions/deletions per file), dispatched
    /// when the PR becomes selected. The raw `FileChange`s come back via
    /// `Msg::FilesArrived`; categorisation happens in `update`.
    FetchFiles {
        ctx: Arc<RequestContext>,
        number: u64,
    },
    /// Clear the status message after `delay_ms`, but only if it's still the one
    /// identified by `token` (see `Model::status_gen`).
    ScheduleStatusClear {
        token: u64,
        delay_ms: u64,
    },
    /// Fetch a single PR's detail (base PR fields + body). Dispatched when the
    /// user enters the detail view (`Enter` on the list); result comes back as
    /// `Msg::PRDetailArrived`. Also dispatched on `r` to refresh the current
    /// PR's detail without going through the refresh-queue (#11).
    ///
    /// `pr` is boxed behind `Arc` so this variant stays pointer-sized; PR is a
    /// wide struct and without indirection the entire `Cmd` enum and every
    /// `Vec<Cmd>` would be padded to ~360 bytes per element.
    FetchPRDetail {
        ctx: Arc<RequestContext>,
        pr: Arc<PR>,
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
                Err(error) => {
                    // Config is a hard prerequisite, not a best-effort command,
                    // so it gets its own halt-the-list failure rather than the
                    // transient `command_failed` status. `{error:#}` renders the
                    // full validation cause chain.
                    let error = format!("{error:#}");
                    tracing::warn!(%error, "config load failed");
                    Msg::ConfigLoadFailed { error }
                }
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
            match result {
                Ok(repo) => {
                    tracing::info!(owner = %repo.owner, repo = %repo.repo, "repo detected");
                    let _ = tx.send(Msg::RepoDetected(Some(repo)));
                }
                Err(error) => {
                    // Surface the failure as a transient status, AND settle the
                    // detection gate with `None` so the configured Tracked Repos
                    // still fetch (running outside a git repo isn't fatal).
                    let _ = tx.send(command_failed("detect repo", error));
                    let _ = tx.send(Msg::RepoDetected(None));
                }
            }
        }
        Cmd::FetchOpenPRs { repo, token } => {
            run_fetch_open_prs(repo, token, tx, limiter).await;
        }
        Cmd::FetchReviewStatus { ctx, pr_numbers } => {
            let repo_slug = ctx.repo.slug();
            request(
                &tx,
                &limiter,
                "fetch review status",
                async move {
                    GraphQlClient::new(&ctx.token)?
                        .fetch_review_status(&ctx.repo.owner, &ctx.repo.repo, &pr_numbers)
                        .await
                },
                move |results| {
                    results
                        .into_iter()
                        .map(|(number, status)| Msg::ReviewStatusArrived {
                            pr: PrKey {
                                repo_slug: repo_slug.clone(),
                                number,
                            },
                            status,
                        })
                        .collect()
                },
            )
            .await;
        }
        Cmd::FetchThreads { ctx, number } => {
            let pr = PrKey {
                repo_slug: ctx.repo.slug(),
                number,
            };
            request(
                &tx,
                &limiter,
                "fetch review threads",
                async move {
                    GraphQlClient::new(&ctx.token)?
                        .fetch_review_threads(
                            &ctx.repo.owner,
                            &ctx.repo.repo,
                            number,
                            &ctx.bot_logins,
                        )
                        .await
                },
                move |threads| vec![Msg::ThreadsArrived { pr, threads }],
            )
            .await;
        }
        Cmd::FetchReviews { ctx, number } => {
            let pr = PrKey {
                repo_slug: ctx.repo.slug(),
                number,
            };
            request(
                &tx,
                &limiter,
                "fetch reviews",
                async move {
                    OctocrabRest::new(&ctx.token)?
                        .list_reviews(&ctx.repo.owner, &ctx.repo.repo, number)
                        .await
                },
                move |reviews| vec![Msg::ReviewsArrived { pr, reviews }],
            )
            .await;
        }
        Cmd::FetchIssueComments { ctx, number } => {
            let pr = PrKey {
                repo_slug: ctx.repo.slug(),
                number,
            };
            request(
                &tx,
                &limiter,
                "fetch issue comments",
                async move {
                    OctocrabRest::new(&ctx.token)?
                        .list_issue_comments(
                            &ctx.repo.owner,
                            &ctx.repo.repo,
                            number,
                            &ctx.bot_logins,
                        )
                        .await
                },
                move |comments| vec![Msg::IssueCommentsArrived { pr, comments }],
            )
            .await;
        }
        Cmd::FetchChecks { ctx, head_sha } => {
            let repo_slug = ctx.repo.slug();
            request(
                &tx,
                &limiter,
                "fetch check runs",
                async move {
                    let checks = OctocrabRest::new(&ctx.token)?
                        .list_check_runs(&ctx.repo.owner, &ctx.repo.repo, &head_sha)
                        .await?;
                    Ok((head_sha, checks))
                },
                move |(head_sha, checks)| {
                    vec![Msg::ChecksArrived {
                        repo_slug,
                        head_sha,
                        checks,
                    }]
                },
            )
            .await;
        }
        Cmd::FetchFiles { ctx, number } => {
            let pr = PrKey {
                repo_slug: ctx.repo.slug(),
                number,
            };
            // Dispatching this command set the PR's `Enrichment::files` entry to
            // `Requested`; a failure must roll that back so re-selecting the PR
            // retries. `request` sends `CommandFailed` first, then we send
            // `FilesFetchFailed` to clear the in-flight guard.
            let failed_pr = pr.clone();
            let ok = request(
                &tx,
                &limiter,
                "fetch files",
                async move {
                    OctocrabRest::new(&ctx.token)?
                        .list_files(&ctx.repo.owner, &ctx.repo.repo, number)
                        .await
                },
                move |files| vec![Msg::FilesArrived { pr, files }],
            )
            .await;
            if !ok {
                let _ = tx.send(Msg::FilesFetchFailed { pr: failed_pr });
            }
        }
        Cmd::ScheduleStatusClear { token, delay_ms } => {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            let _ = tx.send(Msg::StatusCleared { token });
        }
        Cmd::FetchPRDetail { ctx, pr } => {
            let number = pr.number;
            request(
                &tx,
                &limiter,
                "fetch PR detail",
                async move {
                    OctocrabRest::new(&ctx.token)?
                        .fetch_pr_detail(&ctx.repo.owner, &ctx.repo.repo, number)
                        .await
                },
                move |detail| vec![Msg::PRDetailArrived(detail)],
            )
            .await;
        }
    }
}

async fn run_fetch_open_prs(
    repo: RepoInfo,
    token: Secret<String>,
    tx: mpsc::UnboundedSender<Msg>,
    limiter: Arc<NetworkLimiter>,
) {
    let repo_slug = repo.slug();
    let client = match OctocrabRest::new(&token) {
        Ok(client) => client,
        Err(error) => {
            let _ = tx.send(pr_list_failed(repo_slug, "build github client", error));
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
    let result = client.list_open_prs(&repo.owner, &repo.repo, pr_tx).await;
    let _ = forwarder.await;

    match result {
        Ok(()) => {
            tracing::info!(owner = %repo.owner, repo = %repo.repo, "open PR listing finished");
            let _ = tx.send(Msg::PrListLoaded { repo_slug });
        }
        Err(error) => {
            let _ = tx.send(pr_list_failed(repo_slug, "list open PRs", error));
        }
    }
}

/// Hold one concurrency permit, run a single GitHub request, and forward the
/// messages it produces — or, on error, one `CommandFailed` (covering the
/// client build inside the request too). Captures the build-permit-dispatch
/// shape every per-PR enrichment command shares; `context` names the operation
/// so the failure reads e.g. "fetch reviews: ...". `op` is a lazy future, so
/// the permit is held only across the actual await, not while it's constructed.
///
/// Returns whether the request succeeded so a caller that recorded in-flight
/// state in the model can send its own rollback message after the
/// `CommandFailed` (e.g. `FetchFiles` sends `FilesFetchFailed`). Callers with
/// no cleanup ignore the return.
async fn request<T>(
    tx: &mpsc::UnboundedSender<Msg>,
    limiter: &Arc<NetworkLimiter>,
    context: &'static str,
    op: impl Future<Output = anyhow::Result<T>>,
    to_msgs: impl FnOnce(T) -> Vec<Msg>,
) -> bool {
    let _permit = limiter.acquire().await;
    match op.await {
        Ok(value) => {
            for msg in to_msgs(value) {
                let _ = tx.send(msg);
            }
            true
        }
        Err(error) => {
            let _ = tx.send(command_failed(context, error));
            false
        }
    }
}

/// Log the failure here (cmd is the impure layer) and build the `Msg` for
/// the runtime to deliver to `update`. Keeps `update` a pure
/// `(Model, Msg) -> Vec<Cmd>` — the warn doesn't have to fire from inside
/// the reducer. `{error:#}` renders the full anyhow cause chain (e.g. the
/// underlying HTTP status), not just the outermost context.
fn command_failed(context: &'static str, error: anyhow::Error) -> Msg {
    let error = format!("{error:#}");
    tracing::warn!(context, %error, "command failed");
    Msg::CommandFailed { context, error }
}

/// Same as `command_failed`, but for the PR-listing pipeline (which has its
/// own `Msg` variant so the status bar can prioritise list errors over
/// generic command errors). `repo_slug` names the Tracked Repo whose listing
/// failed so other repos' phases are untouched.
fn pr_list_failed(repo_slug: String, context: &'static str, error: anyhow::Error) -> Msg {
    let error = format!("{error:#}");
    tracing::warn!(%repo_slug, context, %error, "pr listing failed");
    Msg::PrListFailed {
        repo_slug,
        context,
        error,
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
