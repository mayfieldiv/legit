use std::sync::Arc;

use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind};

use crate::{git_remote::RepoInfo, github::rest::PrKey, secret::Secret};

use super::{
    cmd::{Cmd, RequestContext},
    detail_items,
    model::{DetailState, FilesState, Model, RepoDetection, StatusKind, StatusMessage, ViewMode},
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

/// Fire one `Cmd::FetchOpenPRs` per Tracked Repo once all three startup
/// prerequisites have landed: the auth token authorizes the requests, repo
/// detection has *settled* (it completes the tracked set with the CWD repo when
/// detection succeeds, and contributes nothing when it fails — but either way
/// the tracked set is final), and a settled config supplies the current user
/// and bot logins that drive smart-status. Any one missing yields no command —
/// we wait for the last. The detection gate keys off settled-ness, not success:
/// gating on `Detected` would wedge the app at an empty list when detection
/// fails (outside a git repo / no GitHub remote), never fetching even the
/// configured Tracked Repos. The config gate is load-bearing twice over: it
/// guarantees no PR's blocker is derived before the user is known, and it
/// guarantees the tracked set is final so every repo fetches exactly once.
/// Marks each repo's listing as Loading so the view swaps from "No open PRs" to
/// "Loading pull requests…" until results land.
fn maybe_fetch_open_prs(model: &mut Model) -> Vec<Cmd> {
    let Some(token) = model.auth_token.as_ref() else {
        return Vec::new();
    };
    if !model.repo.is_settled() || !model.config_loaded {
        return Vec::new();
    }
    let token = token.clone();
    let mut cmds = Vec::new();
    for repo in model.tracked_repos() {
        model.list.begin_fetch(&repo.slug());
        cmds.push(Cmd::FetchOpenPRs {
            repo,
            token: token.clone(),
        });
    }
    cmds
}

/// Fan out per-PR enrichment for one Tracked Repo after its REST list
/// settles: one batched review-status query plus per-PR threads / reviews /
/// issue-comments fetches. Checks are deferred until review-status reports
/// each PR's head SHA. Yields nothing if auth isn't ready or the repo has no
/// PRs in the list.
fn enrichment_cmds(model: &Model, repo_slug: &str) -> Vec<Cmd> {
    let Some(token) = model.auth_token.as_ref() else {
        return Vec::new();
    };
    let Some(repo) = model.tracked_repo(repo_slug) else {
        return Vec::new();
    };
    let numbers: Vec<u64> = model
        .list
        .prs()
        .iter()
        .filter(|pr| pr.repo_slug == repo_slug)
        .map(|pr| pr.number)
        .collect();
    if numbers.is_empty() {
        return Vec::new();
    }
    let ctx = request_context(&repo, token, &model.config.bot_logins);
    let mut cmds = Vec::with_capacity(numbers.len() * 3 + 1);
    cmds.push(Cmd::FetchReviewStatus {
        ctx: Arc::clone(&ctx),
        pr_numbers: numbers.clone(),
    });
    for number in numbers {
        cmds.push(Cmd::FetchThreads {
            ctx: Arc::clone(&ctx),
            number,
        });
        cmds.push(Cmd::FetchReviews {
            ctx: Arc::clone(&ctx),
            number,
        });
        cmds.push(Cmd::FetchIssueComments {
            ctx: Arc::clone(&ctx),
            number,
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

/// Build a checks fetch for a freshly-learned head SHA against the Tracked
/// Repo it belongs to, unless checks for that (repo, SHA) already arrived —
/// the same SHA in another repo (a fork) has its own check runs, so it never
/// suppresses this repo's fetch. A `None` SHA (a PR with no commits yet)
/// yields nothing. `pr` is the PR the SHA came from, carried so the fetch can
/// be focus-promoted in the limiter.
fn maybe_fetch_checks(model: &Model, head_sha: Option<String>, pr: &PrKey) -> Vec<Cmd> {
    let Some(sha) = head_sha else {
        return Vec::new();
    };
    if model
        .enrichment
        .checks
        .contains_key(&(pr.repo_slug.clone(), sha.clone()))
    {
        return Vec::new();
    }
    let (Some(token), Some(repo)) = (model.auth_token.as_ref(), model.tracked_repo(&pr.repo_slug))
    else {
        return Vec::new();
    };
    vec![Cmd::FetchChecks {
        ctx: request_context(&repo, token, &model.config.bot_logins),
        pr: pr.clone(),
        head_sha: sha,
    }]
}

/// Request the currently-selected PR's changed files, unless they were already
/// requested. Idempotent via the PR's `Enrichment::files` entry, so it's safe
/// to call after every selection-changing event: each PR's files are fetched at
/// most once (any existing entry — `Requested` or `Loaded` — suppresses a
/// re-dispatch, and a failed fetch removes the entry via `Msg::FilesFetchFailed`
/// so re-selecting the PR retries), and a single keypress moves the cursor one
/// PR, so at most one `FetchFiles` is ever dispatched per call. Yields nothing
/// when auth isn't ready, no PR is selected, or the selected PR's repo isn't
/// tracked.
fn maybe_fetch_selected_files(model: &mut Model) -> Vec<Cmd> {
    let Some(token) = model.auth_token.as_ref() else {
        return Vec::new();
    };
    let Some(pr) = model.list.selected_pr() else {
        return Vec::new();
    };
    let key = pr.key();
    let number = pr.number;
    if model.enrichment.files.contains_key(&key) {
        return Vec::new();
    }
    let Some(repo) = model.tracked_repo(&key.repo_slug) else {
        return Vec::new();
    };
    let ctx = request_context(&repo, token, &model.config.bot_logins);
    model.enrichment.files.insert(key, FilesState::Requested);
    vec![Cmd::FetchFiles { ctx, number }]
}

/// Dispatch `Cmd::FetchPRDetail` for `key`, using the model's current auth
/// token and the tracked repo the PR belongs to. Yields nothing when auth
/// isn't ready or the PR's repo isn't tracked (the caller is already guarded,
/// but this is defensive).
fn fetch_pr_detail_cmd(model: &Model, key: &PrKey) -> Vec<Cmd> {
    let Some(token) = model.auth_token.as_ref() else {
        return Vec::new();
    };
    let Some(repo) = model.tracked_repo(&key.repo_slug) else {
        return Vec::new();
    };
    let ctx = request_context(&repo, token, &model.config.bot_logins);
    vec![Cmd::FetchPRDetail {
        ctx,
        key: key.clone(),
    }]
}

/// Switch to the Repo Tab at `index` (0 = All): re-derive the visible list
/// under the new scope and reset the selection to its top. A no-op for the
/// already-active tab so re-pressing its digit doesn't lose the selection.
fn select_tab(model: &mut Model, index: usize) {
    if index == model.active_tab {
        return;
    }
    model.active_tab = index;
    model.relayout();
    model.list.select_first_visible();
}

/// Step the active tab by `delta`, wrapping at the ends (h from All lands on
/// the last repo, l from the last repo lands on All).
fn step_tab(model: &mut Model, delta: isize) {
    let count = (model.tracked_repos().len() + 1) as isize;
    let next = (model.active_tab as isize + delta).rem_euclid(count) as usize;
    select_tab(model, next);
}

/// Jump straight to tab `index` (digit keys; 0 = All). Digits past the last
/// tab are ignored.
fn jump_to_tab(model: &mut Model, index: usize) {
    if index <= model.tracked_repos().len() {
        select_tab(model, index);
    }
}

/// Handle one keypress while the filter editor is open. The editor consumes
/// every key (a digit must type, not switch tabs; `q` must type, not quit) —
/// only Esc and Enter leave it. Every text change re-filters live; the
/// open/clear/submit transitions can add or remove the chip row, so they
/// re-derive the viewport too.
fn handle_filter_editing_key(model: &mut Model, code: KeyCode) {
    match code {
        KeyCode::Esc => {
            model.list.filter_clear();
            model.sync_viewport();
        }
        KeyCode::Enter => {
            model.list.filter_submit();
            model.sync_viewport();
        }
        KeyCode::Backspace => model.list.filter_backspace(),
        KeyCode::Char(c) => model.list.filter_push(c),
        // Anything else (arrows, etc.) is consumed without effect.
        _ => return,
    }
    model.relayout();
}

/// Handle one keypress in normal list mode (no filter editor open).
fn handle_list_key(model: &mut Model, code: KeyCode) -> Vec<Cmd> {
    match code {
        KeyCode::Char('q') => model.should_quit = true,
        KeyCode::Char('j') => model.list.move_down(),
        KeyCode::Char('k') => model.list.move_up(),
        KeyCode::Char('g') => {
            // Cycle smart-status -> repo -> none -> smart-status, resetting
            // selection, then rebuild the layout under the new grouping.
            model.list.cycle_grouping();
            model.relayout();
        }
        KeyCode::Char('h') | KeyCode::Left | KeyCode::Char('[') => step_tab(model, -1),
        KeyCode::Char('l') | KeyCode::Right | KeyCode::Char(']') => step_tab(model, 1),
        KeyCode::Char('/') => {
            model.list.filter_open();
            model.sync_viewport();
            model.relayout();
        }
        KeyCode::Esc if model.list.filter().is_visible() => {
            // Esc on an applied filter drops it (editing-mode Esc is handled
            // by the editor).
            model.list.filter_clear();
            model.sync_viewport();
            model.relayout();
        }
        KeyCode::Char(c) if c.is_ascii_digit() => {
            jump_to_tab(model, (c as u8 - b'0') as usize);
        }
        KeyCode::Enter => {
            // Enter on the selected PR enters the Detail view with a fresh
            // `DetailState` (no body, scroll at the top). If auth is ready and
            // the PR's repo is tracked the fetch fires immediately; otherwise
            // the detail view shows "Loading PR detail…" with no fetch in
            // flight. Nothing re-fires the fetch when auth/config later land, so
            // in that rare startup-race case the user must re-press Enter — it
            // doesn't happen in practice (auth resolves before any keypress).
            if let Some(pr) = model.list.selected_pr() {
                let key = pr.key();
                let cmds = fetch_pr_detail_cmd(model, &key);
                model.view_mode = ViewMode::Detail(DetailState {
                    key,
                    body: None,
                    scroll: 0,
                    focused_index: 0,
                });
                return cmds;
            }
        }
        _ => {}
    }
    Vec::new()
}

/// Lines scrolled per PageUp/PageDown in the detail body.
const DETAIL_SCROLL_PAGE: u16 = 10;

/// How many items the open detail view's focus sequence holds (1 — just the
/// body — while threads/comments haven't arrived). 0 outside Detail mode.
fn detail_focusable_len(model: &Model) -> usize {
    let ViewMode::Detail(detail) = &model.view_mode else {
        return 0;
    };
    let threads = model
        .enrichment
        .review_threads
        .get(&detail.key)
        .map_or(&[][..], Vec::as_slice);
    let comments = model
        .enrichment
        .issue_comments
        .get(&detail.key)
        .map_or(&[][..], Vec::as_slice);
    detail_items::focusable_items(threads, comments, model.detail_filters()).len()
}

/// Step the detail focus by `delta`, clamped to the focusable sequence
/// (`j`/`Down` forward, `k`/`Up` back). A no-op outside Detail mode.
fn move_detail_focus(model: &mut Model, delta: isize) {
    let len = detail_focusable_len(model);
    if let ViewMode::Detail(detail) = &mut model.view_mode {
        detail.focused_index = detail
            .focused_index
            .saturating_add_signed(delta)
            .min(len.saturating_sub(1));
    }
}

/// Re-clamp the detail focus after the focusable sequence may have shrunk
/// (threads/comments arriving with fewer items, or a filter hiding the focused
/// card). A no-op outside Detail mode.
fn clamp_detail_focus(model: &mut Model) {
    let len = detail_focusable_len(model);
    if let ViewMode::Detail(detail) = &mut model.view_mode {
        detail.focused_index = detail.focused_index.min(len.saturating_sub(1));
    }
}

/// Measure the open detail view's body via the same `detail_content` layout
/// the view renders, so scroll math and rendering can't disagree. `None`
/// outside Detail mode, while the body hasn't arrived, or if the PR left the
/// list. Measured at a fixed epoch: the layout's line ranges are
/// age-independent (a byline is one line whatever its age string says), and
/// `update` stays a pure `(Model, Msg)` reducer with no clock.
fn detail_layout(model: &Model) -> Option<crate::view::detail::DetailContent> {
    let ViewMode::Detail(detail) = &model.view_mode else {
        return None;
    };
    let description = detail.body.as_ref()?;
    let pr = model.list.pr(&detail.key)?;
    Some(crate::view::detail::detail_content(
        model,
        pr,
        description,
        detail.focused_index,
        model.terminal_width,
        chrono::DateTime::UNIX_EPOCH,
    ))
}

/// The detail body's viewport height: the terminal minus the pinned header and
/// status bar.
fn detail_viewport_rows(model: &Model) -> u16 {
    model
        .terminal_height
        .saturating_sub(crate::view::detail::CHROME_ROWS)
}

/// Clamp the open detail view's scroll offset so it can never sit more than
/// one screenful above the last content line. The content is the full
/// `detail_layout` — description, checks, thread and conversation cards —
/// under-clamping to a subset would stop the user reaching the bottom. A no-op
/// outside Detail mode or while the body hasn't arrived (there is nothing to
/// scroll yet). Mirrors the render-time backstop in
/// `view::detail::render_body`, but here `scroll` is the stored source of
/// intent: clamping in `update` keeps a held PageDown from drifting
/// unboundedly and leaving the subsequent PageUp presses visually dead.
fn clamp_detail_scroll(model: &mut Model) {
    let Some(content) = detail_layout(model) else {
        return;
    };
    let max_scroll = (content.lines.len() as u16).saturating_sub(detail_viewport_rows(model));
    if let ViewMode::Detail(detail) = &mut model.view_mode {
        detail.scroll = detail.scroll.min(max_scroll);
    }
}

/// Adjust the detail scroll so the focused card is fully visible: scroll up to
/// its first line when it starts above the viewport, down to its last line
/// when it ends below — and never past its first line, so a card taller than
/// the viewport shows its top. Mirrors the TS `scrollChildIntoView` on focus
/// change.
fn scroll_detail_focus_into_view(model: &mut Model) {
    let Some(content) = detail_layout(model) else {
        return;
    };
    let viewport = detail_viewport_rows(model) as usize;
    let ViewMode::Detail(detail) = &mut model.view_mode else {
        return;
    };
    let Some(range) = content.item_ranges.get(detail.focused_index) else {
        return;
    };
    let scroll = detail.scroll as usize;
    let new_scroll = if range.start < scroll {
        range.start
    } else if range.end > scroll + viewport {
        (range.end - viewport).min(range.start)
    } else {
        scroll
    };
    detail.scroll = new_scroll as u16;
}

/// Handle one keypress while the detail view is open. The caller guarantees
/// `model.view_mode` is `ViewMode::Detail`, so the scroll/refresh arms match
/// the inner `DetailState` directly.
fn handle_detail_key(model: &mut Model, code: KeyCode) -> Vec<Cmd> {
    match code {
        KeyCode::Esc => {
            // Return to the list view. A single assignment drops the whole
            // `DetailState` (body + scroll), so there is no side state to clear
            // by hand — the next Enter builds a fresh one starting at the top.
            model.view_mode = ViewMode::List;
        }
        KeyCode::Char('r') => {
            // Refresh the current PR detail: refetch the body. Clears the
            // cached body first so the view briefly shows the loading
            // placeholder, consistent with the initial enter-and-fetch flow.
            // Preserves the scroll position so the user stays at the same
            // place after a quick re-fetch.
            //
            // Clone the key so the borrow of model.view_mode ends before the
            // body is reassigned below (which needs a unique borrow of the
            // model via `fetch_pr_detail_cmd`).
            if let ViewMode::Detail(detail) = &model.view_mode {
                let key = detail.key.clone();
                let cmds = fetch_pr_detail_cmd(model, &key);
                if !cmds.is_empty()
                    && let ViewMode::Detail(detail) = &mut model.view_mode
                {
                    detail.body = None;
                }
                return cmds;
            }
        }
        // Focus forward/back: j/k (and arrows) cycle the focusable items —
        // body, thread roots, replies, issue comments — not the raw scroll
        // offset (PageUp/PageDown still scroll). The scroll follows the focus
        // so the newly-focused card is always on screen.
        KeyCode::Char('j') | KeyCode::Down => {
            move_detail_focus(model, 1);
            scroll_detail_focus_into_view(model);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            move_detail_focus(model, -1);
            scroll_detail_focus_into_view(model);
        }
        // Page down
        KeyCode::PageDown => {
            if let ViewMode::Detail(detail) = &mut model.view_mode {
                detail.scroll = detail.scroll.saturating_add(DETAIL_SCROLL_PAGE);
            }
            clamp_detail_scroll(model);
        }
        // Page up
        KeyCode::PageUp => {
            if let ViewMode::Detail(detail) = &mut model.view_mode {
                detail.scroll = detail.scroll.saturating_sub(DETAIL_SCROLL_PAGE);
            }
        }
        _ => {}
    }
    Vec::new()
}

pub fn update(model: &mut Model, msg: Msg) -> Vec<Cmd> {
    match msg {
        Msg::TerminalEvent(Event::Key(key)) => {
            if key.kind != KeyEventKind::Press {
                return Vec::new();
            }
            // Detail mode owns the keypress entirely: its keys never touch the
            // list selection, so the list-mode files-fetch path below must not
            // run for them (e.g. Esc-to-list must not act as if the key was a
            // list keypress). Returning here keeps that out of the list path.
            if matches!(model.view_mode, ViewMode::Detail(_)) {
                return handle_detail_key(model, key.code);
            }
            // List-mode keys. The filter editor (modal precedence) sees every
            // key first and produces no command; a normal list key may (Enter
            // -> FetchPRDetail), in which case dispatch it and stop.
            if model.list.filter().is_editing() {
                handle_filter_editing_key(model, key.code);
            } else {
                let cmds = handle_list_key(model, key.code);
                if !cmds.is_empty() {
                    return cmds;
                }
            }
            // Any key that left us in list mode can have moved the selection;
            // fetch the now-selected PR's files just-in-time. The guard skips a
            // keypress that *entered* detail (Enter): it normally returns a
            // FetchPRDetail above, but when that fetch is suppressed (auth not
            // ready / repo untracked) it would otherwise fall through to here.
            if matches!(model.view_mode, ViewMode::List) {
                return maybe_fetch_selected_files(model);
            }
            Vec::new()
        }
        Msg::TerminalEvent(Event::Resize(width, height)) => {
            model.terminal_width = width;
            model.terminal_height = height;
            model.sync_viewport();
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
            // Settle the detection gate either way: `Some` adds the CWD repo to
            // the tracked set, `None` (no git repo / no GitHub remote) leaves
            // the tracked set to the configured repos alone. Both release the
            // gate so configured Tracked Repos still fetch.
            model.repo = match repo {
                Some(repo) => RepoDetection::Detected(repo),
                None => RepoDetection::Failed,
            };
            maybe_fetch_open_prs(model)
        }
        Msg::PrArrived(pr) => {
            model.list.push(pr);
            // The new PR has no enrichment yet, so it joins "Loading details…";
            // rebuild the layout so it renders immediately.
            model.relayout();
            // The first PR to arrive becomes selected; fetch its files for the
            // summary panel (deduped, so later arrivals that don't move the
            // selection cost nothing).
            maybe_fetch_selected_files(model)
        }
        Msg::PrListLoaded { repo_slug } => {
            model.list.complete_fetch(&repo_slug);
            // This repo's REST stream has settled — fan out enrichment for its
            // PRs now, without waiting on slower repos.
            enrichment_cmds(model, &repo_slug)
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
            maybe_fetch_checks(model, head_sha, &pr)
        }
        Msg::ThreadsArrived { pr, threads } => {
            model.enrichment.review_threads.insert(pr, threads);
            model.refresh_blockers();
            // The detail focus sequence is built from these threads; a shorter
            // list can strand the focus past the end.
            clamp_detail_focus(model);
            Vec::new()
        }
        Msg::ReviewsArrived { pr, reviews } => {
            model.enrichment.reviews.insert(pr, reviews);
            model.refresh_blockers();
            Vec::new()
        }
        Msg::ChecksArrived {
            repo_slug,
            head_sha,
            checks,
        } => {
            model
                .enrichment
                .checks
                .insert((repo_slug, head_sha), checks);
            model.refresh_blockers();
            Vec::new()
        }
        Msg::IssueCommentsArrived { pr, comments } => {
            model.enrichment.issue_comments.insert(pr, comments);
            // Same focus-clamp rule as ThreadsArrived: the sequence may shrink.
            clamp_detail_focus(model);
            Vec::new()
        }
        Msg::FilesArrived { pr, files } => {
            // Categorise the raw file changes against the config `file_rules`
            // here (the pure layer, where config lives), mirroring how blockers
            // are derived in `update` rather than in the impure command.
            // Overwrites the PR's `Requested` state with `Loaded`.
            let categorization = crate::file_category::categorize(&files, &model.config.file_rules);
            model
                .enrichment
                .files
                .insert(pr, FilesState::Loaded(categorization));
            Vec::new()
        }
        Msg::FilesFetchFailed { pr } => {
            // The `Requested` state must not outlive a failed request: removing
            // the entry returns the PR to "never requested", so re-selecting it
            // retries instead of leaving the file breakdown stuck on its loading
            // placeholder. The error itself is surfaced by the accompanying
            // `CommandFailed`.
            model.enrichment.files.remove(&pr);
            Vec::new()
        }
        Msg::PrListFailed {
            repo_slug,
            context,
            error,
        } => {
            let message = format!("{context} ({repo_slug}): {error}");
            model.list.fail_fetch(&repo_slug, message);
            Vec::new()
        }
        Msg::ConfigLoadFailed { error } => {
            // Config is a hard prerequisite (current user + bot logins drive
            // smart-status), so a malformed config is an app-level fatal that
            // blocks every fetch instead of fetching with wrong defaults.
            // `config_loaded` stays false, so `maybe_fetch_open_prs` never fires.
            model.fatal = Some(format!("config error: {error}"));
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
        Msg::PRDetailArrived { pr, body } => {
            // Render the markdown description to display lines exactly once,
            // here on arrival, and cache the result — the view then reuses it
            // every frame instead of re-parsing. Store it only when the view is
            // still open for this PR; discard it if the user already navigated
            // back to the list or entered a different PR's detail.
            if let ViewMode::Detail(detail) = &mut model.view_mode
                && detail.key == pr
            {
                detail.body = Some(crate::view::detail::render_description_lines(&body));
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
