use std::{future::Future, path::PathBuf, sync::Arc};

use tokio::sync::mpsc;

use crate::{
    app::{browser, msg::Msg},
    auth, clipboard, config, git_remote,
    git_remote::RepoInfo,
    github::graphql::GraphQlClient,
    github::limiter::NetworkLimiter,
    github::rest::OctocrabRest,
    github::rest::PrKey,
    secret::Secret,
    worktree,
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
    /// `pr` is the PR whose review-status surfaced this head SHA — carried only
    /// so the limiter can promote the fetch to interactive when that PR is
    /// focused. Checks themselves are stored per (repo, SHA), not per PR.
    FetchChecks {
        ctx: Arc<RequestContext>,
        pr: PrKey,
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
    /// Fetch a single PR's body (markdown). Dispatched when the user enters
    /// the detail view (`Enter` on the list); result comes back as
    /// `Msg::PRDetailArrived`. Also dispatched by the detail view's `r` (via
    /// `detail_refresh_extra_cmds`) to refresh the body, on top of the
    /// `Cmd::RefreshPr` that refreshes the shared enrichment. The PR number is
    /// extracted from `key`; `key` is also echoed back in `PRDetailArrived`'s
    /// `pr` field so `update` can check whether the view is still open for this
    /// PR before storing the body.
    FetchPRDetail {
        ctx: Arc<RequestContext>,
        key: PrKey,
    },
    /// Open `url` in the user's browser. The platform opener is spawned and
    /// reaped off the UI loop; spawn success/failure comes back as an open-url
    /// status message.
    OpenUrl {
        url: String,
    },
    /// List worktrees for a repo's configured source clone.
    ListWorktrees {
        repo_slug: String,
        source_clone: PathBuf,
    },
    /// Create the deterministic worktree for one PR.
    CreateWorktree {
        pr: PrKey,
        source_clone: PathBuf,
        target_path: PathBuf,
    },
    /// Copy text to the user's terminal clipboard via OSC 52.
    CopyToClipboard {
        text: String,
    },
    /// Refresh one PR's enrichment as a single unit: review-status, review
    /// threads, reviews, and — when `include_files` is set — its changed files,
    /// all concurrently. Check runs are *not* fetched here: emitting
    /// `ReviewStatusArrived` drives them through `update`'s `maybe_fetch_checks`
    /// (the canonical owner). Each sub-fetch acquires its own limiter permit (so
    /// the limiter still bounds HTTP and orders by focus) and emits the same
    /// arrival `Msg` the initial enrichment does; the command always finishes
    /// with one `Msg::RefreshComplete` so the PR's refresh indicator clears even
    /// when some sub-fetches failed.
    RefreshPr {
        ctx: Arc<RequestContext>,
        key: PrKey,
        include_files: bool,
    },
    /// Wait `delay_ms`, then emit `Msg::MergeableRetryDue` for `pr` — the
    /// one-shot delayed re-fetch for a PR whose `OPEN` mergeable came back
    /// `UNKNOWN`. The fetch itself is dispatched by `update` so the command
    /// stays a pure timer (the same split as `ScheduleStatusClear`).
    DelayedRetry {
        pr: PrKey,
        delay_ms: u64,
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
                None,
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
            let key = PrKey {
                repo_slug: ctx.repo.slug(),
                number,
            };
            fetch_threads(&ctx, &key, &tx, &limiter).await;
        }
        Cmd::FetchReviews { ctx, number } => {
            let key = PrKey {
                repo_slug: ctx.repo.slug(),
                number,
            };
            fetch_reviews(&ctx, &key, &tx, &limiter).await;
        }
        Cmd::FetchIssueComments { ctx, number } => {
            let pr = PrKey {
                repo_slug: ctx.repo.slug(),
                number,
            };
            request(
                &tx,
                &limiter,
                Some(pr.clone()),
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
        Cmd::FetchChecks { ctx, pr, head_sha } => {
            let repo_slug = ctx.repo.slug();
            request(
                &tx,
                &limiter,
                Some(pr),
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
            let key = PrKey {
                repo_slug: ctx.repo.slug(),
                number,
            };
            // Dispatching this command set the PR's `Enrichment::files` entry to
            // `Requested`; a failure must roll that back so re-selecting the PR
            // retries. `fetch_files` sends `CommandFailed` first, then we send
            // `FilesFetchFailed` to clear the in-flight guard.
            if !fetch_files(&ctx, &key, &tx, &limiter).await {
                let _ = tx.send(Msg::FilesFetchFailed { pr: key });
            }
        }
        Cmd::ScheduleStatusClear { token, delay_ms } => {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            let _ = tx.send(Msg::StatusCleared { token });
        }
        Cmd::OpenUrl { url } => match browser::spawn_open_url(&url) {
            Ok(()) => {
                let _ = tx.send(Msg::OpenUrlSucceeded { url });
            }
            Err(error) => {
                let error = format!("{error:#}");
                tracing::warn!(%url, %error, "open url failed");
                let _ = tx.send(Msg::OpenUrlFailed { url, error });
            }
        },
        Cmd::FetchPRDetail { ctx, key } => {
            let number = key.number;
            request(
                &tx,
                &limiter,
                Some(key.clone()),
                "fetch PR detail",
                async move {
                    OctocrabRest::new(&ctx.token)?
                        .fetch_pr_detail(&ctx.repo.owner, &ctx.repo.repo, number)
                        .await
                },
                move |body| vec![Msg::PRDetailArrived { pr: key, body }],
            )
            .await;
        }
        Cmd::ListWorktrees {
            repo_slug,
            source_clone,
        } => {
            let msg = match blocking(move || worktree::list_worktrees(&source_clone)).await {
                Ok(entries) => Msg::WorktreesArrived { repo_slug, entries },
                Err(error) => command_failed("list worktrees", error),
            };
            let _ = tx.send(msg);
        }
        Cmd::CreateWorktree {
            pr,
            source_clone,
            target_path,
        } => {
            let path = target_path.to_string_lossy().to_string();
            let msg = match blocking(move || {
                worktree::create_worktree_for_pr(&source_clone, &target_path, pr.number)
            })
            .await
            {
                Ok(()) => Msg::WorktreeCreated { pr, path },
                Err(error) => command_failed("create worktree", error),
            };
            let _ = tx.send(msg);
        }
        Cmd::CopyToClipboard { text } => {
            let msg = match clipboard::copy_to_clipboard(&text) {
                Ok(()) => Msg::ClipboardCopied { text },
                Err(error) => Msg::ClipboardCopyFailed {
                    text,
                    error: format!("{error:#}"),
                },
            };
            let _ = tx.send(msg);
        }
        Cmd::RefreshPr {
            ctx,
            key,
            include_files,
        } => {
            run_refresh_pr(ctx, key, include_files, tx, limiter).await;
        }
        Cmd::DelayedRetry { pr, delay_ms } => {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            let _ = tx.send(Msg::MergeableRetryDue { pr });
        }
    }
}

/// Refresh one PR end-to-end. The four sub-fetches are independent and run
/// concurrently — checks are *not* among them: emitting `ReviewStatusArrived`
/// drives the checks fetch through `update`'s canonical `maybe_fetch_checks`
/// path (the same as initial enrichment), the single owner of post-status
/// checks. Whatever fails surfaces its own `CommandFailed`; the trailing
/// `Msg::RefreshComplete` always fires so the PR's refresh indicator clears.
async fn run_refresh_pr(
    ctx: Arc<RequestContext>,
    key: PrKey,
    include_files: bool,
    tx: mpsc::UnboundedSender<Msg>,
    limiter: Arc<NetworkLimiter>,
) {
    tokio::join!(
        fetch_review_status(&ctx, &key, &tx, &limiter),
        fetch_threads(&ctx, &key, &tx, &limiter),
        fetch_reviews(&ctx, &key, &tx, &limiter),
        async {
            // A failure leaves any previously-loaded categorisation in place
            // (best-effort) — the refresh path sets no `Requested` guard to roll
            // back, unlike the on-selection `FetchFiles`.
            if include_files {
                fetch_files(&ctx, &key, &tx, &limiter).await;
            }
        },
    );
    let _ = tx.send(Msg::RefreshComplete { pr: key });
}

/// Fetch one PR's review-status and emit `ReviewStatusArrived`. Shared by the
/// refresh fan-out (carrying the PR key so the limiter can focus-promote it);
/// the repo-wide batch `Cmd::FetchReviewStatus` stays separate as it serves
/// many PRs with no single affinity.
async fn fetch_review_status(
    ctx: &Arc<RequestContext>,
    key: &PrKey,
    tx: &mpsc::UnboundedSender<Msg>,
    limiter: &Arc<NetworkLimiter>,
) -> bool {
    let number = key.number;
    let repo_slug = ctx.repo.slug();
    request(
        tx,
        limiter,
        Some(key.clone()),
        "fetch review status",
        {
            let ctx = Arc::clone(ctx);
            async move {
                GraphQlClient::new(&ctx.token)?
                    .fetch_review_status(&ctx.repo.owner, &ctx.repo.repo, &[number])
                    .await
            }
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
    .await
}

/// Fetch one PR's review threads and emit `ThreadsArrived`. Shared by
/// `Cmd::FetchThreads` and the refresh fan-out.
async fn fetch_threads(
    ctx: &Arc<RequestContext>,
    key: &PrKey,
    tx: &mpsc::UnboundedSender<Msg>,
    limiter: &Arc<NetworkLimiter>,
) -> bool {
    let number = key.number;
    let pr = key.clone();
    request(
        tx,
        limiter,
        Some(key.clone()),
        "fetch review threads",
        {
            let ctx = Arc::clone(ctx);
            async move {
                GraphQlClient::new(&ctx.token)?
                    .fetch_review_threads(&ctx.repo.owner, &ctx.repo.repo, number, &ctx.bot_logins)
                    .await
            }
        },
        move |threads| vec![Msg::ThreadsArrived { pr, threads }],
    )
    .await
}

/// Fetch one PR's reviews and emit `ReviewsArrived`. Shared by
/// `Cmd::FetchReviews` and the refresh fan-out.
async fn fetch_reviews(
    ctx: &Arc<RequestContext>,
    key: &PrKey,
    tx: &mpsc::UnboundedSender<Msg>,
    limiter: &Arc<NetworkLimiter>,
) -> bool {
    let number = key.number;
    let pr = key.clone();
    request(
        tx,
        limiter,
        Some(key.clone()),
        "fetch reviews",
        {
            let ctx = Arc::clone(ctx);
            async move {
                OctocrabRest::new(&ctx.token)?
                    .list_reviews(&ctx.repo.owner, &ctx.repo.repo, number)
                    .await
            }
        },
        move |reviews| vec![Msg::ReviewsArrived { pr, reviews }],
    )
    .await
}

/// Fetch one PR's changed files and emit `FilesArrived`. Returns whether it
/// succeeded so `Cmd::FetchFiles` can roll back its `Requested` guard on
/// failure; the refresh fan-out ignores the result (it sets no guard).
async fn fetch_files(
    ctx: &Arc<RequestContext>,
    key: &PrKey,
    tx: &mpsc::UnboundedSender<Msg>,
    limiter: &Arc<NetworkLimiter>,
) -> bool {
    let number = key.number;
    let pr = key.clone();
    request(
        tx,
        limiter,
        Some(key.clone()),
        "fetch files",
        {
            let ctx = Arc::clone(ctx);
            async move {
                OctocrabRest::new(&ctx.token)?
                    .list_files(&ctx.repo.owner, &ctx.repo.repo, number)
                    .await
            }
        },
        move |files| vec![Msg::FilesArrived { pr, files }],
    )
    .await
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

    // Hold a concurrency slot for the duration of the (paginated) listing so
    // the network indicator reflects it and the listing stays within the
    // background sub-cap (leaving headroom for the focused PR's fetches).
    // Repo-wide work, so it carries no PR affinity and is never focus-promoted.
    let _permit = limiter.acquire(None).await;
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
/// so the failure reads e.g. "fetch reviews: ...". `pr` is the PR the fetch
/// serves — prioritised by the limiter while that PR is focused — or `None`
/// for repo-wide work. `op` is a lazy future, so the permit is held only
/// across the actual await, not while it's constructed.
///
/// Returns whether the request succeeded so a caller that recorded in-flight
/// state in the model can send its own rollback message after the
/// `CommandFailed` (e.g. `FetchFiles` sends `FilesFetchFailed`). Callers with
/// no cleanup ignore the return.
async fn request<T>(
    tx: &mpsc::UnboundedSender<Msg>,
    limiter: &Arc<NetworkLimiter>,
    pr: Option<PrKey>,
    context: &'static str,
    op: impl Future<Output = anyhow::Result<T>>,
    to_msgs: impl FnOnce(T) -> Vec<Msg>,
) -> bool {
    let _permit = limiter.acquire(pr).await;
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
