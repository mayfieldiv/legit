use std::sync::Arc;

use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind};

use crate::{git_remote::RepoInfo, secret::Secret};

use super::{
    cmd::{Cmd, RequestContext},
    model::{Model, StatusKind, StatusMessage},
    msg::Msg,
};

/// How long a transient status message lingers before its scheduled clear.
const STATUS_SUCCESS_CLEAR_MS: u64 = 4_000;
const STATUS_ERROR_CLEAR_MS: u64 = 8_000;

/// Set the transient status message, bumping the generation so a pending clear
/// for an older message no-ops. Returns a `ScheduleStatusClear` for Success (4s)
/// and Error (8s); Info persists until replaced.
fn set_status(model: &mut Model, kind: StatusKind, text: String) -> Vec<Cmd> {
    model.status_gen = model.status_gen.wrapping_add(1);
    model.status = Some(StatusMessage { kind, text });
    let delay_ms = match kind {
        StatusKind::Success => STATUS_SUCCESS_CLEAR_MS,
        StatusKind::Error => STATUS_ERROR_CLEAR_MS,
        StatusKind::Info => return Vec::new(),
    };
    vec![Cmd::ScheduleStatusClear {
        token: model.status_gen,
        delay_ms,
    }]
}

/// Fire `Cmd::FetchOpenPRs` once all three startup prerequisites have landed:
/// the auth token authorizes the request, repo detection defines what to fetch,
/// and a settled config supplies the current user and bot logins that drive
/// smart-status. Any one missing yields no command — we wait for the last. The
/// config gate is load-bearing: it guarantees no PR's blocker is derived before
/// the user is known, so a lost startup race can never misclassify a PR.
/// Marks the PR list as Loading so the view swaps from "No open PRs" to
/// "Loading pull requests…" until results land.
fn maybe_fetch_open_prs(model: &mut Model) -> Vec<Cmd> {
    let (Some(token), Some(repo)) = (model.auth_token.as_ref(), model.repo.as_ref()) else {
        return Vec::new();
    };
    if !model.config_loaded {
        return Vec::new();
    }
    model.list.begin_fetch();
    vec![Cmd::FetchOpenPRs {
        repo: repo.clone(),
        token: token.clone(),
    }]
}

/// Fan out per-PR enrichment after the REST list settles: one batched
/// review-status query plus per-PR threads / reviews / issue-comments fetches.
/// Checks are deferred until review-status reports each PR's head SHA. Yields
/// nothing if auth/repo aren't ready or the list is empty.
fn enrichment_cmds(model: &Model) -> Vec<Cmd> {
    let (Some(token), Some(repo)) = (model.auth_token.as_ref(), model.repo.as_ref()) else {
        return Vec::new();
    };
    let prs = model.list.prs();
    if prs.is_empty() {
        return Vec::new();
    }
    let ctx = request_context(repo, token, &model.config.bot_logins);
    let mut cmds = Vec::with_capacity(prs.len() * 3 + 1);
    cmds.push(Cmd::FetchReviewStatus {
        ctx: Arc::clone(&ctx),
        pr_numbers: prs.iter().map(|pr| pr.number).collect(),
    });
    for pr in prs {
        cmds.push(Cmd::FetchThreads {
            ctx: Arc::clone(&ctx),
            number: pr.number,
        });
        cmds.push(Cmd::FetchReviews {
            ctx: Arc::clone(&ctx),
            number: pr.number,
        });
        cmds.push(Cmd::FetchIssueComments {
            ctx: Arc::clone(&ctx),
            number: pr.number,
        });
    }
    cmds
}

/// Build the `Arc<RequestContext>` shared by a fan-out of enrichment commands:
/// the tracked repo, auth token, and configured bot logins.
fn request_context(
    repo: &RepoInfo,
    token: &Secret<String>,
    bot_logins: &[String],
) -> Arc<RequestContext> {
    Arc::new(RequestContext {
        repo: repo.clone(),
        token: token.clone(),
        bot_logins: bot_logins.to_vec(),
    })
}

/// Build a checks fetch for a freshly-learned head SHA, unless checks for it
/// already arrived. A `None` SHA (a PR with no commits yet) yields nothing.
fn maybe_fetch_checks(model: &Model, head_sha: Option<String>) -> Vec<Cmd> {
    let Some(sha) = head_sha else {
        return Vec::new();
    };
    if model.enrichment.checks.contains_key(&sha) {
        return Vec::new();
    }
    let (Some(token), Some(repo)) = (model.auth_token.as_ref(), model.repo.as_ref()) else {
        return Vec::new();
    };
    vec![Cmd::FetchChecks {
        ctx: request_context(repo, token, &model.config.bot_logins),
        head_sha: sha,
    }]
}

pub fn update(model: &mut Model, msg: Msg) -> Vec<Cmd> {
    match msg {
        Msg::TerminalEvent(Event::Key(key)) => {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('q') => model.should_quit = true,
                    KeyCode::Char('j') => model.list.move_down(),
                    KeyCode::Char('k') => model.list.move_up(),
                    KeyCode::Char('g') => {
                        // Cycle smart-status -> repo -> none -> smart-status,
                        // resetting selection, then rebuild the layout under the
                        // new grouping.
                        model.list.cycle_grouping();
                        model.relayout();
                    }
                    _ => {}
                }
            }
            Vec::new()
        }
        Msg::TerminalEvent(Event::Resize(_, height)) => {
            // The status bar takes one row; everything above belongs to the
            // list. Saturating-sub keeps a 0-row viewport handled gracefully.
            model.list.resize((height as usize).saturating_sub(1));
            Vec::new()
        }
        Msg::TerminalEvent(_) => Vec::new(),
        Msg::ConfigLoaded(config) => {
            model.config = config;
            // Releasing the fetch gate here lets the PR fetch fire if auth + repo
            // already landed — config (a local file read) usually wins the
            // startup race, but when it arrives last it must kick off the fetch.
            model.config_loaded = true;
            maybe_fetch_open_prs(model)
        }
        Msg::AuthTokenResolved(token) => {
            model.auth_token = Some(token);
            maybe_fetch_open_prs(model)
        }
        Msg::RepoDetected(repo) => {
            model.repo = Some(repo);
            maybe_fetch_open_prs(model)
        }
        Msg::PrArrived(pr) => {
            model.list.push(pr);
            // The new PR has no enrichment yet, so it joins "Loading details…";
            // rebuild the layout so it renders immediately.
            model.relayout();
            Vec::new()
        }
        Msg::PrListLoaded => {
            model.list.complete_fetch();
            // The REST stream has settled — fan out enrichment for every PR now
            // in the list.
            enrichment_cmds(model)
        }
        Msg::NetworkStatsChanged(stats) => {
            model.network_stats = stats;
            Vec::new()
        }
        Msg::ReviewStatusArrived { pr, status } => {
            // Overwrite the list fields the REST endpoint couldn't supply, then
            // — once we know the head SHA — fan out the checks fetch for it.
            let head_sha = status.head_commit_sha.clone();
            if let Some(entry) = model.list.pr_mut(&pr) {
                entry.additions = status.additions;
                entry.deletions = status.deletions;
                entry.review_decision = status.review_decision;
                entry.mergeable = status.mergeable;
                entry.last_commit_date = status.last_commit_date;
                entry.head_commit_sha = status.head_commit_sha;
            } else {
                // PR no longer in the list (e.g. filtered/refetched); drop it.
                return Vec::new();
            }
            // review_decision/mergeable feed the blocker rules, so re-derive.
            model.refresh_blockers();
            maybe_fetch_checks(model, head_sha)
        }
        Msg::ThreadsArrived { pr, threads } => {
            model.enrichment.review_threads.insert(pr, threads);
            model.refresh_blockers();
            Vec::new()
        }
        Msg::ReviewsArrived { pr, reviews } => {
            model.enrichment.reviews.insert(pr, reviews);
            model.refresh_blockers();
            Vec::new()
        }
        Msg::ChecksArrived { head_sha, checks } => {
            model.enrichment.checks.insert(head_sha, checks);
            model.refresh_blockers();
            Vec::new()
        }
        Msg::IssueCommentsArrived { pr, comments } => {
            model.enrichment.issue_comments.insert(pr, comments);
            Vec::new()
        }
        Msg::PrListFailed { context, error } => {
            model.list.fail_fetch(format!("{context}: {error}"));
            Vec::new()
        }
        Msg::ConfigLoadFailed { error } => {
            // Config is a hard prerequisite (current user + bot logins drive
            // smart-status), so a malformed config halts the list with a
            // persistent failure instead of fetching with wrong defaults.
            // `config_loaded` stays false, so `maybe_fetch_open_prs` never fires.
            model.list.fail_fetch(format!("config error: {error}"));
            Vec::new()
        }
        Msg::CommandFailed { context, error } => {
            // Covers bootstrap-command failures and best-effort per-PR
            // enrichment: surface the error, keep all PRs and any enrichment
            // that did arrive, and never crash.
            set_status(model, StatusKind::Error, format!("{context}: {error}"))
        }
        Msg::StatusCleared { token } => {
            // Ignore a stale timer — a newer message has since taken the slot.
            if model.status_gen == token {
                model.status = None;
            }
            Vec::new()
        }
        Msg::Quit => {
            model.should_quit = true;
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests;
