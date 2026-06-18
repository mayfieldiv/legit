//! Refresh (`r`/`R`) and the one-shot mergeable retry: deciding what to
//! re-fetch, marking PRs in flight, ordering `R` by smart-status tier, and the
//! lazy-`UNKNOWN` mergeable re-fetch. Split out of `update` so the reducer arms
//! stay thin and the whole refresh story reads in one place — `super::apply`'s
//! refresh arms delegate here, as does the detail view's `r` (`begin_refresh`).

use crate::{
    app::{
        cmd::Cmd,
        model::{Model, StatusKind},
    },
    blocker::Tier,
    git_remote::RepoInfo,
    github::rest::{PR, PRState, PrKey},
    secret::Secret,
};

use super::{request_context, set_status};

/// Delay before the one-shot re-fetch of a PR whose `OPEN` mergeable came back
/// `UNKNOWN`, giving GitHub time to finish its lazy mergeability computation.
const MERGEABLE_RETRY_MS: u64 = 3_000;

/// `r`: refresh the selected PR, with files (the summary panel shows its File
/// Category breakdown). The limiter promotes it via focus, so it leads
/// regardless of tier. With nothing selected because the active Repo Tab has no
/// PRs, re-list that repo instead — "check GitHub for new PRs".
pub(super) fn refresh_selected_cmds(model: &mut Model) -> Vec<Cmd> {
    let Some(key) = model.list.selected_pr().map(PR::key) else {
        return relist_empty_repo_cmds(model);
    };
    begin_refresh(model, key, true).into_iter().collect()
}

/// `R`: refresh every visible PR, dispatched in smart-status tier order so the
/// limiter's FIFO background lane drains `me-blocking` first; re-read the config
/// so repos added since launch are picked up. `count` is the PRs actually
/// dispatched — already-refreshing ones dedupe to no-ops. Then re-list (for
/// discovery) the in-scope repos so newly-opened PRs surface and closed ones
/// are pruned — always, not only when nothing was re-enriched.
pub(super) fn refresh_all_cmds(model: &mut Model) -> Vec<Cmd> {
    let mut keys: Vec<PrKey> = model
        .list
        .visible_pr_indices()
        .filter_map(|index| model.list.prs().get(index).map(PR::key))
        .collect();
    keys.sort_by_key(|key| refresh_tier_rank(model, key));
    let mut cmds = vec![Cmd::LoadConfig];
    let mut count = 0;
    for key in keys {
        if let Some(cmd) = begin_refresh(model, key, false) {
            cmds.push(cmd);
            count += 1;
        }
    }
    if count > 0 {
        cmds.extend(set_status(
            model,
            StatusKind::Info,
            format!("Refreshing {count} PRs…"),
        ));
    }
    // Re-list to discover newly-opened PRs (and prune ones closed since) — on
    // top of re-enriching what is already pooled, regardless of how many PRs
    // that was. The config reload above only re-lists never-fetched or failed
    // repos, never an already-loaded one, so it can't surface new PRs in a repo
    // already listed.
    cmds.extend(relist_for_discovery(model));
    cmds
}

/// Dispatch one repo's open-PR listing as a re-list (`r`/`R` discovery), marking
/// it Loading so the view shows the "Loading pull requests…" placeholder.
/// `None` when a listing is already in flight for it — re-dispatching then would
/// re-stream and duplicate the pooled PRs. The pooled PRs are left in place:
/// `merge_listed` dedupes the re-stream (preserving each PR's enrichment) and
/// `finish_listing` prunes the ones that didn't reappear.
fn dispatch_relist(model: &mut Model, repo: RepoInfo, token: &Secret<String>) -> Option<Cmd> {
    let slug = repo.slug();
    if model.list.is_loading(Some(&slug)) {
        return None;
    }
    model.list.begin_fetch(&slug);
    Some(Cmd::FetchOpenPRs {
        repo,
        token: token.clone(),
    })
}

/// Re-fetch open-PR listings so `R` discovers newly-opened PRs and prunes ones
/// closed since: every tracked repo on the All tab, or just the active repo on
/// a Repo Tab, matching the tab's scope. Pooled PRs are reconciled, not cleared
/// (see `dispatch_relist`). A no-op without auth.
fn relist_for_discovery(model: &mut Model) -> Vec<Cmd> {
    let Some(token) = model.auth_token.as_ref().cloned() else {
        return Vec::new();
    };
    let scope = model.active_scope();
    let mut cmds = Vec::new();
    for repo in model.tracked_repos() {
        if scope.as_deref().is_some_and(|active| active != repo.slug()) {
            continue;
        }
        cmds.extend(dispatch_relist(model, repo, &token));
    }
    cmds
}

/// `r` on a Repo Tab whose repo has no pooled PRs: re-fetch that repo's open-PR
/// listing so newly-opened PRs surface — the "check GitHub for new PRs" path.
/// `r` otherwise just refreshes the selected PR, so an empty repo tab (nothing
/// selected) would never re-check GitHub. This is `relist_for_discovery`
/// restricted to an empty Repo Tab: a no-op on the All tab (no single repo to
/// target) and when the repo already has PRs (a filter merely hid them — `r`
/// leaves those alone). Past those guards the active scope is exactly this repo,
/// so the shared helper re-lists it alone (and handles auth / the in-flight
/// guard). (`R` calls `relist_for_discovery` unguarded, so it re-lists even
/// non-empty repos.)
fn relist_empty_repo_cmds(model: &mut Model) -> Vec<Cmd> {
    let Some(slug) = model.active_scope() else {
        return Vec::new();
    };
    if model.list.any_in_scope(Some(&slug)) {
        return Vec::new();
    }
    relist_for_discovery(model)
}

/// One PR's `Cmd::RefreshPr` fan-out finished: clear its indicator and, once
/// every in-flight refresh has drained, post the run's "Refreshed N" summary
/// and reset the run counter. A completion for a PR that isn't in `refreshing`
/// (never dispatched, or already drained) is a harmless no-op.
pub(super) fn complete_refresh(model: &mut Model, pr: PrKey) -> Vec<Cmd> {
    if model.refreshing.remove(&pr) {
        model.refresh_completed += 1;
    }
    if model.refreshing.is_empty() && model.refresh_completed > 0 {
        let count = std::mem::take(&mut model.refresh_completed);
        let plural = if count == 1 { "" } else { "s" };
        return set_status(
            model,
            StatusKind::Success,
            format!("Refreshed {count} PR{plural}"),
        );
    }
    Vec::new()
}

/// Begin a refresh of `key`, returning its `Cmd::RefreshPr` — or `None` when
/// the PR is already refreshing (the dedupe no-op), or auth isn't ready / the
/// repo isn't tracked (no command, so the PR is *not* marked refreshing and
/// can't get stuck showing the indicator). Marking and command-building are one
/// step so the `refreshing` set never disagrees with what was dispatched.
///
/// Clears the PR's mergeable-retry guard so a refresh that still sees `UNKNOWN`
/// can schedule a fresh one-shot retry, and evicts its cached check runs so the
/// refresh re-fetches them even on an unchanged head SHA (see `evict_checks`).
pub(super) fn begin_refresh(model: &mut Model, key: PrKey, include_files: bool) -> Option<Cmd> {
    if model.refreshing.contains(&key) {
        return None;
    }
    let token = model.auth_token.as_ref()?;
    let repo = model.tracked_repo(&key.repo_slug)?;
    let ctx = request_context(&repo, token, &model.config.bot_logins);
    let cmd = Cmd::RefreshPr {
        ctx,
        key: key.clone(),
        include_files,
    };
    model.mergeable_retried.remove(&key);
    evict_checks(model, &key);
    model.refreshing.insert(key);
    Some(cmd)
}

/// Drop the cached check runs for `key`'s current head commit, so the refresh's
/// `ReviewStatusArrived` re-fetches them through the canonical
/// `maybe_fetch_checks` path even when the head SHA is unchanged (CI re-run on
/// the same commit — the prime "did it pass yet?" case). Without the eviction
/// `maybe_fetch_checks` would suppress that fetch as already-present.
fn evict_checks(model: &mut Model, key: &PrKey) {
    if let Some(sha) = model.list.pr(key).and_then(|pr| pr.head_commit_sha.clone()) {
        model
            .enrichment
            .checks
            .remove(&(key.repo_slug.clone(), sha));
    }
}

/// `R`'s dispatch rank for a PR by its cached smart-status tier: `me-blocking`
/// (0) before `needs-review` (1) before `waiting-on-author` (2), an un-derived
/// PR last (3). Dispatch order becomes the limiter's FIFO background order, so a
/// higher-tier PR refreshes first. The selected/open PR needs no rank — the
/// limiter promotes it via focus.
fn refresh_tier_rank(model: &Model, key: &PrKey) -> u8 {
    match model.blockers.get(key).map(|blocker| blocker.tier) {
        Some(Tier::MeBlocking) => 0,
        Some(Tier::NeedsReview) => 1,
        Some(Tier::WaitingOnAuthor) => 2,
        None => 3,
    }
}

/// Schedule the one-shot mergeable retry for `pr` when its freshly-stored
/// status warrants it: GitHub computes mergeability lazily (returning `UNKNOWN`
/// on the first read) but reports `UNKNOWN` permanently for merged/closed PRs,
/// so retry only an `OPEN` PR still showing `UNKNOWN`, and only once per PR
/// (the guard, cleared by a manual refresh).
pub(super) fn maybe_retry_mergeable(model: &mut Model, pr: &PrKey) -> Vec<Cmd> {
    let Some(entry) = model.list.pr(pr) else {
        return Vec::new();
    };
    if entry.mergeable != "UNKNOWN" || entry.state != PRState::Open {
        return Vec::new();
    }
    if model.mergeable_retried.contains(pr) {
        return Vec::new();
    }
    model.mergeable_retried.insert(pr.clone());
    vec![Cmd::DelayedRetry {
        pr: pr.clone(),
        delay_ms: MERGEABLE_RETRY_MS,
    }]
}

/// A scheduled mergeable re-fetch is due: re-run review-status for the one PR.
/// `Msg::MergeableRetryDue` carries only the identity, so rebuild the request
/// context here. The arrival re-runs the UNKNOWN check, but the guard set when
/// this retry was scheduled keeps it one-shot. A no-op when auth isn't ready or
/// the PR's repo isn't tracked.
pub(super) fn mergeable_retry_due_cmds(model: &Model, pr: &PrKey) -> Vec<Cmd> {
    let (Some(token), Some(repo)) = (model.auth_token.as_ref(), model.tracked_repo(&pr.repo_slug))
    else {
        return Vec::new();
    };
    let ctx = request_context(&repo, token, &model.config.bot_logins);
    vec![Cmd::FetchReviewStatus {
        ctx,
        pr_numbers: vec![pr.number],
    }]
}
