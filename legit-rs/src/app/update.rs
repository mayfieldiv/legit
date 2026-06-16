use std::sync::Arc;

use ratatui::crossterm::event::{
    Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use crate::{
    format::abbreviate_home, git_remote::RepoInfo, github::rest::PrKey, secret::Secret, worktree,
};

use super::{
    browser,
    cmd::{Cmd, RequestContext},
    detail_items, detail_layout, list_layout,
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

/// List worktrees for every configured Tracked Repo that has a source clone.
fn list_worktree_cmds(model: &Model) -> Vec<Cmd> {
    model
        .tracked_repos()
        .into_iter()
        .filter_map(|repo| {
            let repo_slug = repo.slug();
            match worktree::resolve_source_clone(&model.config, &repo_slug) {
                Ok(Some(source_clone)) => Some(Cmd::ListWorktrees {
                    repo_slug,
                    source_clone,
                }),
                Ok(None) => None,
                Err(error) => {
                    tracing::warn!(%repo_slug, %error, "failed to resolve worktree source clone");
                    None
                }
            }
        })
        .collect()
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
        cmds.extend(pr_enrichment_cmds(&ctx, number));
    }
    cmds
}

/// The per-PR enrichment trio — threads, reviews, issue comments — for one PR.
/// The shared definition of "enrich this PR": `enrichment_cmds` fans it out
/// across a repo's whole list, and the detail view's `r` re-dispatches it for
/// the open PR.
fn pr_enrichment_cmds(ctx: &Arc<RequestContext>, number: u64) -> [Cmd; 3] {
    [
        Cmd::FetchThreads {
            ctx: Arc::clone(ctx),
            number,
        },
        Cmd::FetchReviews {
            ctx: Arc::clone(ctx),
            number,
        },
        Cmd::FetchIssueComments {
            ctx: Arc::clone(ctx),
            number,
        },
    ]
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

/// Start the `w` worktree flow for a PR, or surface the missing-config error.
fn create_worktree_cmds(model: &mut Model, pr: crate::github::rest::PR) -> Vec<Cmd> {
    if let Some(existing) = model.worktree_for_pr(&pr) {
        let path = existing.path.clone();
        return set_status(
            model,
            StatusKind::Success,
            format!("Worktree already exists at {}", abbreviate_home(&path)),
        );
    }

    let source_clone = match worktree::resolve_source_clone(&model.config, &pr.repo_slug) {
        Ok(Some(source_clone)) => source_clone,
        Ok(None) => {
            return set_status(
                model,
                StatusKind::Error,
                format!(
                    "No sourceClone configured for {}; edit ~/.legit/config.json",
                    pr.repo_slug
                ),
            );
        }
        Err(error) => {
            return set_status(
                model,
                StatusKind::Error,
                format!(
                    "Failed to resolve sourceClone for {}: {error:#}",
                    pr.repo_slug
                ),
            );
        }
    };
    let target_path = match worktree::resolve_worktree_path(
        &model.config,
        &pr.repo_slug,
        pr.number,
        &pr.head_ref,
    ) {
        Ok(path) => path,
        Err(error) => {
            return set_status(
                model,
                StatusKind::Error,
                format!(
                    "Failed to resolve worktree path for {}: {error:#}",
                    pr.repo_slug
                ),
            );
        }
    };
    let mut cmds = set_status(model, StatusKind::Info, "Creating worktree…".to_owned());
    cmds.push(Cmd::CreateWorktree {
        pr: pr.key(),
        source_clone,
        target_path,
    });
    cmds
}

/// Every fetch the detail view's `r` refresh re-dispatches for the open PR:
/// the body plus the per-PR enrichment trio whose results the Review Threads
/// and Conversation sections render. Enrichment otherwise fetches exactly once
/// per list load, so this is also the retry path when an initial fetch failed
/// and left a section stuck on its loading placeholder. Yields nothing when
/// auth isn't ready or the PR's repo isn't tracked, like `fetch_pr_detail_cmd`.
fn refresh_detail_cmds(model: &Model, key: &PrKey) -> Vec<Cmd> {
    let Some(token) = model.auth_token.as_ref() else {
        return Vec::new();
    };
    let Some(repo) = model.tracked_repo(&key.repo_slug) else {
        return Vec::new();
    };
    let ctx = request_context(&repo, token, &model.config.bot_logins);
    let mut cmds = vec![Cmd::FetchPRDetail {
        ctx: Arc::clone(&ctx),
        key: key.clone(),
    }];
    cmds.extend(pr_enrichment_cmds(&ctx, key.number));
    cmds
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
        KeyCode::Char('j') | KeyCode::Down => model.list.move_down(),
        KeyCode::Char('k') | KeyCode::Up => model.list.move_up(),
        KeyCode::Char('g') => {
            // Cycle smart-status -> repo -> none -> smart-status, resetting
            // selection, then rebuild the layout under the new grouping.
            model.list.cycle_grouping();
            model.relayout();
        }
        KeyCode::Char('h') | KeyCode::Left | KeyCode::Char('[') => step_tab(model, -1),
        KeyCode::Char('l') | KeyCode::Right | KeyCode::Char(']') => step_tab(model, 1),
        KeyCode::Char('o') => {
            if let Some(pr) = model.list.selected_pr().cloned() {
                return apply(model, Msg::OpenInBrowser(pr));
            }
        }
        KeyCode::Char('d') => {
            if let Some(pr) = model.list.selected_pr().cloned() {
                return apply(model, Msg::OpenInDevin(pr));
            }
        }
        KeyCode::Char('w') => {
            if let Some(pr) = model.list.selected_pr().cloned() {
                return create_worktree_cmds(model, pr);
            }
        }
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
                    focus: detail_items::DetailFocus::Body,
                    followed: None,
                    expanded: std::collections::HashSet::new(),
                });
                return cmds;
            }
        }
        _ => {}
    }
    Vec::new()
}

/// Lines scrolled per PageUp/PageDown in the detail body.
const DETAIL_SCROLL_PAGE: usize = 10;

/// Lines scrolled per mouse-wheel tick in the detail body.
const DETAIL_SCROLL_WHEEL: usize = 3;

/// Display rows scrolled per mouse-wheel tick in the Open PR List.
const LIST_SCROLL_WHEEL: usize = 3;

/// The open detail view's Focus Sequence derivation under the current
/// filters, or `None` outside Detail mode. The shared input to focus stepping
/// and re-anchoring.
fn detail_items(model: &Model) -> Option<detail_items::DetailItems<'_>> {
    let ViewMode::Detail(detail) = &model.view_mode else {
        return None;
    };
    Some(detail_items::DetailItems::derive(
        model.enrichment.threads_for(&detail.key),
        model.enrichment.comments_for(&detail.key),
        model.detail_filters(),
    ))
}

fn handle_list_left_click(model: &mut Model, mouse: MouseEvent) -> Vec<Cmd> {
    let Some(visible_row) = list_layout::visible_row_at(model, mouse.column, mouse.row) else {
        return Vec::new();
    };
    if model.list.select_visible_row(visible_row) {
        maybe_fetch_selected_files(model)
    } else {
        Vec::new()
    }
}

fn handle_detail_left_click(model: &mut Model, mouse: MouseEvent) -> Vec<Cmd> {
    let body_top = detail_layout::HEADER_HEIGHT;
    let status_row = model.terminal_height.saturating_sub(1);
    if mouse.row < body_top || mouse.row >= status_row {
        return Vec::new();
    }
    let body_row = usize::from(mouse.row - body_top);
    let ViewMode::Detail(detail) = &model.view_mode else {
        return Vec::new();
    };
    let content_row = detail.scroll.saturating_add(body_row);
    let Some(content) = measured_detail_content(model) else {
        return Vec::new();
    };
    let Some(index) = content
        .item_ranges
        .iter()
        .position(|range| range.contains(&content_row))
    else {
        return Vec::new();
    };
    let Some(items) = detail_items(model) else {
        return Vec::new();
    };
    let focus = items.focus_at(index);
    if let ViewMode::Detail(detail) = &mut model.view_mode {
        detail.focus = focus;
    }
    Vec::new()
}

/// Step the detail focus by `delta` through the Focus Sequence (`j`/`Down`
/// forward, `k`/`Up` back), clamped at the ends. The target card's identity
/// is stored, not its raw position, so later sequence changes can't retarget
/// it. A no-op outside Detail mode.
fn move_detail_focus(model: &mut Model, delta: isize) {
    let Some(items) = detail_items(model) else {
        return;
    };
    let ViewMode::Detail(detail) = &model.view_mode else {
        return;
    };
    let index = items.resolve_focus(&detail.focus).index();
    let focus = items.focus_at(index.saturating_add_signed(delta));
    if let ViewMode::Detail(detail) = &mut model.view_mode {
        detail.focus = focus;
    }
}

/// Re-anchor the detail focus against the current Focus Sequence: identity
/// wins, so threads/comments arriving or filter toggles that insert items
/// above the focus move its index — never which card is focused. A focus
/// whose item vanished (hidden by a filter, gone on refresh) falls back to
/// its last position, clamped to the shrunk sequence. A no-op outside Detail
/// mode.
fn resolve_detail_focus(model: &mut Model) {
    let Some(items) = detail_items(model) else {
        return;
    };
    let ViewMode::Detail(detail) = &model.view_mode else {
        return;
    };
    let focus = items.resolve_focus(&detail.focus);
    if let ViewMode::Detail(detail) = &mut model.view_mode {
        detail.focus = focus;
    }
}

/// Measure the open detail view's body via the same `detail_content` layout
/// the view renders, so scroll math and rendering can't disagree. `None`
/// outside Detail mode, while the body hasn't arrived, or if the PR left the
/// list. Measured at a fixed epoch: the layout's line ranges are
/// age-independent (a byline is one line whatever its age string says), and
/// `update` stays a pure `(Model, Msg)` reducer with no clock.
fn measured_detail_content(model: &Model) -> Option<detail_layout::DetailContent> {
    let ViewMode::Detail(detail) = &model.view_mode else {
        return None;
    };
    let description = detail.body.as_ref()?;
    let pr = model.list.pr(&detail.key)?;
    Some(detail_layout::detail_content(
        model,
        pr,
        description,
        detail,
        model.terminal_width,
        chrono::DateTime::UNIX_EPOCH,
    ))
}

/// The detail body's viewport height: the terminal minus the pinned header and
/// status bar.
fn detail_viewport_rows(model: &Model) -> usize {
    usize::from(model.terminal_height).saturating_sub(usize::from(detail_layout::CHROME_ROWS))
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
            // Refresh the open PR: refetch the body plus the threads / reviews
            // / issue comments behind the Review Threads and Conversation
            // sections. The cached body clears so the view shows the loading
            // placeholder, consistent with the initial enter-and-fetch flow;
            // threads/comments keep rendering their current data until the
            // fresh lists overwrite it. Preserves the scroll position so the
            // user stays at the same place after a quick re-fetch.
            //
            // Clone the key so the borrow of model.view_mode ends before the
            // body is reassigned below (which needs a unique borrow of the
            // model via `refresh_detail_cmds`).
            if let ViewMode::Detail(detail) = &model.view_mode {
                let key = detail.key.clone();
                let cmds = refresh_detail_cmds(model, &key);
                if !cmds.is_empty()
                    && let ViewMode::Detail(detail) = &mut model.view_mode
                {
                    detail.body = None;
                }
                return cmds;
            }
        }
        // Toggle resolved-thread visibility (hidden by default). The toggle
        // can shift the focused card or fall the focus back to another card;
        // either way `normalize_detail` sees the moved anchor and scrolls the
        // card back into view.
        KeyCode::Char('t') => {
            model.show_resolved = !model.show_resolved;
        }
        // Toggle bot-comment visibility (shown by default). Same derived
        // follow as `t`.
        KeyCode::Char('b') => {
            model.show_bot_comments = !model.show_bot_comments;
        }
        // Open the focused item in the browser: a thread/reply/comment opens
        // its deep link; the body falls back to the PR itself (mirrors the TS
        // fallback to `openInBrowser(pr)`). The stored focus is re-anchored
        // after every update, so its URL is never stale here.
        KeyCode::Char('o') => {
            if let ViewMode::Detail(detail) = &model.view_mode {
                let url = detail
                    .focus
                    .url()
                    .map_or_else(|| detail.key.html_url(), str::to_owned);
                return apply(model, Msg::OpenUrl(url));
            }
        }
        KeyCode::Char('w') => {
            if let ViewMode::Detail(detail) = &model.view_mode
                && let Some(pr) = model.list.pr(&detail.key).cloned()
            {
                return create_worktree_cmds(model, pr);
            }
        }
        // Toggle the focused card's long-body expansion (collapsed by
        // default; see `detail_layout::collapse_body`). Keyed by the comment's
        // URL, so the state survives filter toggles moving the indices. The
        // body (no URL) has nothing to toggle. Expansion grows the card in
        // place — identity and start line unchanged — so the follow anchor is
        // cleared explicitly to make `normalize_detail` pull the grown card
        // back into view.
        KeyCode::Enter => {
            if let ViewMode::Detail(detail) = &mut model.view_mode
                && let Some(url) = detail.focus.url().map(str::to_owned)
            {
                if !detail.expanded.remove(&url) {
                    detail.expanded.insert(url);
                }
                detail.followed = None;
            }
        }
        // Focus forward/back: j/k (and arrows) cycle the focusable items —
        // body, thread roots, replies, issue comments — not the raw scroll
        // offset (PageUp/PageDown still scroll). The moved focus changes the
        // `normalize_detail` anchor, so the newly-focused card scrolls into
        // view.
        KeyCode::Char('j') | KeyCode::Down => move_detail_focus(model, 1),
        KeyCode::Char('k') | KeyCode::Up => move_detail_focus(model, -1),
        // Page down. `normalize_detail` clamps the offset to the last
        // screenful, so a held PageDown can't drift past the end.
        KeyCode::PageDown => {
            if let ViewMode::Detail(detail) = &mut model.view_mode {
                detail.scroll = detail.scroll.saturating_add(DETAIL_SCROLL_PAGE);
            }
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

/// Re-establish the open detail view's invariants after a state change, in
/// one pass over at most one layout build:
///
/// - the focus re-anchors to its item's place in the fresh Focus Sequence
///   (falling back positionally when the item vanished);
/// - the focused card scrolls back into view — up to its first line when it
///   starts above the viewport, down to its last when it ends below, never
///   past its first so a card taller than the viewport shows its top — but
///   only when the follow anchor (focus identity + card start line) moved
///   since the last pass. Focus moves, filter toggles, and content arriving
///   above the card all move the anchor; raw PageUp/PageDown scrolling never
///   does. Mirrors the TS `scrollChildIntoView` on focus change.
/// - the scroll offset clamps so it never sits more than one screenful above
///   the last content line, measured against the full layout (a refresh
///   returning a shorter body, a shrinking sequence, or a taller terminal can
///   all strand it — and PageUp deliberately doesn't clamp, so a stranded
///   offset reads as dead keypresses). Mirrors the render-time backstop in
///   `view::detail::render_body`, but here `scroll` is the stored source of
///   intent.
///
/// Runs once at the end of every `update` (minus the `skips_normalize` fast
/// path) rather than at each mutation site, so no message path can forget it
/// — a missed pass self-heals on the next message. A no-op outside Detail
/// mode, and cheap while the body hasn't arrived.
fn normalize_detail(model: &mut Model) {
    resolve_detail_focus(model);
    let Some(content) = measured_detail_content(model) else {
        return;
    };
    let viewport = detail_viewport_rows(model);
    let max_scroll = content.lines.len().saturating_sub(viewport);
    let ViewMode::Detail(detail) = &mut model.view_mode else {
        return;
    };
    let scroll = detail.scroll.min(max_scroll);
    let Some(range) = content.item_ranges.get(detail.focus.index()) else {
        // Unreachable (the focus was just resolved against the same
        // derivation the layout walks), but degrade to the bare clamp.
        detail.scroll = scroll;
        return;
    };
    let anchor = (detail.focus.clone(), range.start);
    detail.scroll = if detail.followed.as_ref() != Some(&anchor) {
        if range.start < scroll {
            range.start
        } else if range.end > scroll + viewport {
            (range.end - viewport).min(range.start)
        } else {
            scroll
        }
    } else {
        scroll
    };
    detail.followed = Some(anchor);
}

/// True for messages that provably can't change the open detail view's
/// content, Focus Sequence, or viewport, letting `update` skip the
/// `normalize_detail` layout build. An explicit skip-list, so any new message
/// defaults to normalizing — the safe direction (a wasted pass costs one
/// build; a wrong skip strands the invariants until the next message). The
/// per-PR arrivals are the high-volume case: the enrichment fan-out lands
/// threads/reviews/comments for every listed PR while the user reads one of
/// them, and only the open PR's own arrivals can move its cards.
fn skips_normalize(msg: &Msg, model: &Model) -> bool {
    let ViewMode::Detail(detail) = &model.view_mode else {
        // normalize_detail is already a cheap no-op outside Detail mode.
        return false;
    };
    match msg {
        Msg::NetworkStatsChanged(_) | Msg::StatusCleared { .. } => true,
        Msg::ThreadsArrived { pr, .. }
        | Msg::ReviewsArrived { pr, .. }
        | Msg::IssueCommentsArrived { pr, .. }
        | Msg::ReviewStatusArrived { pr, .. }
        | Msg::FilesArrived { pr, .. }
        | Msg::FilesFetchFailed { pr }
        | Msg::PRDetailArrived { pr, .. } => *pr != detail.key,
        _ => false,
    }
}

pub fn update(model: &mut Model, msg: Msg) -> Vec<Cmd> {
    let skip_normalize = skips_normalize(&msg, model);
    let cmds = apply(model, msg);
    if !skip_normalize {
        normalize_detail(model);
    }
    cmds
}

/// The reducer's message dispatch. Detail-view invariant maintenance lives in
/// `update`'s `normalize_detail` pass, not in the individual arms.
fn apply(model: &mut Model, msg: Msg) -> Vec<Cmd> {
    match msg {
        Msg::TerminalEvent(Event::Key(key)) => {
            if key.kind != KeyEventKind::Press {
                return Vec::new();
            }
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                model.should_quit = true;
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
        // Wheel ticks scroll the viewport, never the focus — the wheel is not
        // a selection device (the runtime captures the mouse precisely so the
        // terminal can't translate ticks into arrow keys, which are focus
        // keys). The follow anchor is untouched, so `normalize_detail` only
        // clamps. In the list, wheel input scrolls the display window without
        // moving the selected PR or triggering selection-side effects.
        Msg::TerminalEvent(Event::Mouse(mouse))
            if matches!(
                mouse.kind,
                MouseEventKind::ScrollDown | MouseEventKind::ScrollUp
            ) =>
        {
            let down = mouse.kind == MouseEventKind::ScrollDown;
            match &mut model.view_mode {
                ViewMode::Detail(detail) => {
                    detail.scroll = if down {
                        detail.scroll.saturating_add(DETAIL_SCROLL_WHEEL)
                    } else {
                        detail.scroll.saturating_sub(DETAIL_SCROLL_WHEEL)
                    };
                    Vec::new()
                }
                ViewMode::List => {
                    if down {
                        model.list.scroll_down(LIST_SCROLL_WHEEL);
                    } else {
                        model.list.scroll_up(LIST_SCROLL_WHEEL);
                    }
                    Vec::new()
                }
            }
        }
        Msg::TerminalEvent(Event::Mouse(mouse))
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) =>
        {
            match model.view_mode {
                ViewMode::Detail(_) => handle_detail_left_click(model, mouse),
                ViewMode::List => handle_list_left_click(model, mouse),
            }
        }
        Msg::TerminalEvent(_) => Vec::new(),
        Msg::ConfigLoaded(config) => {
            model.config = config;
            // Releasing the fetch gate here lets the PR fetch fire if auth + repo
            // already landed — config (a local file read) usually wins the
            // startup race, but when it arrives last it must kick off the fetch.
            model.config_loaded = true;
            let mut cmds = maybe_fetch_open_prs(model);
            cmds.extend(list_worktree_cmds(model));
            cmds
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
            // Worktree listing stays config-driven: only configured repos can
            // declare a sourceClone, so ConfigLoaded is the event that has
            // enough information to list them.
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
                entry.review_status_loaded = true;
            } else {
                // PR no longer in the list (e.g. filtered/refetched); drop it.
                return Vec::new();
            }
            // review_decision/mergeable feed the blocker rules, so re-derive.
            model.refresh_blockers();
            maybe_fetch_checks(model, head_sha, &pr)
        }
        Msg::ThreadsArrived { pr, threads } => {
            model.enrichment.store_threads(pr, threads);
            model.refresh_blockers();
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
            model.enrichment.store_issue_comments(pr, comments);
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
        Msg::OpenUrl(url) => vec![browser::open_url(url)],
        Msg::OpenInBrowser(pr) => vec![browser::open_in_browser(&pr)],
        Msg::OpenInDevin(pr) => vec![browser::open_in_devin(&pr)],
        Msg::OpenUrlSucceeded { url } => set_status(
            model,
            StatusKind::Success,
            format!("Opened {}", browser::open_label(&url)),
        ),
        Msg::OpenUrlFailed { url, error } => set_status(
            model,
            StatusKind::Error,
            format!("Failed to open {}: {error}", browser::open_label(&url)),
        ),
        Msg::WorktreesArrived { repo_slug, entries } => {
            model.worktrees_by_repo.insert(repo_slug, entries);
            Vec::new()
        }
        Msg::WorktreeCreated { pr, path } => {
            let entries = model
                .worktrees_by_repo
                .entry(pr.repo_slug.clone())
                .or_default();
            entries.retain(|entry| entry.path != path);
            entries.push(worktree::WorktreeEntry {
                path: path.clone(),
                head: String::new(),
                branch_ref: None,
                branch_name: None,
                detached: true,
                bare: false,
                locked: None,
                prunable: None,
            });
            let mut cmds = set_status(
                model,
                StatusKind::Success,
                format!("Worktree created at {}", abbreviate_home(&path)),
            );
            cmds.extend(list_worktree_cmds(model));
            cmds
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
                detail.body = Some(detail_layout::render_description_lines(&body));
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
