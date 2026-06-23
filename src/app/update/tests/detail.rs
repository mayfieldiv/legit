use ratatui::crossterm::event::KeyCode;

use crate::{
    app::{cmd::Cmd, model::ViewMode, msg::Msg, update::update},
    git_remote::RepoInfo,
    github::rest::PrKey,
    github::types::{IssueComment, PRState, ReviewComment, ReviewStatus},
    secret::Secret,
    test_fixtures::{self, issue_comment, thread},
};

/// True when the open detail view has its rendered body cached. `false` if not
/// in Detail mode or the body hasn't arrived. Lets the tests assert on the
/// consolidated `ViewMode::Detail(DetailState)` shape without repeating the
/// match.
fn detail_has_body(model: &crate::app::model::Model) -> bool {
    matches!(&model.view_mode, ViewMode::Detail(detail) if detail.body.is_some())
}

/// The concatenated text of the open detail view's rendered body, flattened
/// with `<details>` collapsed (the default the view shows); panics if not in
/// Detail mode or the body hasn't arrived.
fn detail_body_text(model: &crate::app::model::Model) -> String {
    match &model.view_mode {
        ViewMode::Detail(detail) => {
            let blocks = detail.body.as_ref().expect("body arrived");
            crate::markdown::flatten_blocks(blocks, false)
                .iter()
                .flat_map(|line| {
                    line.spans
                        .iter()
                        .map(|s| s.content.as_ref().to_owned())
                        .collect::<Vec<_>>()
                })
                .collect()
        }
        ViewMode::List => panic!("expected Detail mode"),
    }
}

/// The focused item's resolved index in the open detail view; panics if not
/// in Detail mode.
fn detail_focus(model: &crate::app::model::Model) -> usize {
    match &model.view_mode {
        ViewMode::Detail(detail) => detail.focus.index(),
        ViewMode::List => panic!("expected Detail mode"),
    }
}

/// The focused item's identity URL (`None` = the body); panics if not in
/// Detail mode.
fn detail_focus_url(model: &crate::app::model::Model) -> Option<String> {
    match &model.view_mode {
        ViewMode::Detail(detail) => detail.focus.url().map(str::to_owned),
        ViewMode::List => panic!("expected Detail mode"),
    }
}

/// The scroll offset of the open detail view; panics if not in Detail mode.
fn detail_scroll(model: &crate::app::model::Model) -> usize {
    match &model.view_mode {
        ViewMode::Detail(detail) => detail.scroll,
        ViewMode::List => panic!("expected Detail mode"),
    }
}

/// Deliver `body` to the open detail view through the real
/// `Msg::PRDetailArrived` path, so the normalize pass runs exactly as it
/// would in production (recording the follow anchor included); panics if not
/// in Detail mode.
fn set_detail_body(model: &mut crate::app::model::Model, body: &str) {
    let ViewMode::Detail(detail) = &model.view_mode else {
        panic!("expected Detail mode");
    };
    let pr = detail.key.clone();
    update(
        model,
        Msg::PRDetailArrived {
            pr,
            body: body.to_owned(),
        },
    );
}

use super::{enriched_model, key_event, mouse_down_event, wheel_event};

/// A model with auth + repo detected and one PR streamed in and selected.
fn model_with_one_pr() -> crate::app::model::Model {
    let mut model = enriched_model(&[42]);
    model.config_loaded = true;
    model.auth_token = Some(Secret::new("ghp_test".to_owned()));
    model.repo = crate::app::model::RepoDetection::Detected(RepoInfo {
        owner: "mayfieldiv".to_owned(),
        repo: "legit".to_owned(),
    });
    model.list.complete_fetch("mayfieldiv/legit");
    model.relayout();
    model
}

fn pr_key_42() -> PrKey {
    PrKey {
        repo_slug: "mayfieldiv/legit".to_owned(),
        number: 42,
    }
}

/// A model entered into Detail with a tall body and a small viewport, so there
/// is ample room to scroll down before hitting the clamp. Used by the scroll
/// tests so they exercise pure step arithmetic, not the bottom-of-content
/// clamp (which has its own dedicated test).
fn scrollable_detail_model() -> crate::app::model::Model {
    let mut model = model_with_one_pr();
    // A short viewport so the 100-line body leaves a large max scroll.
    model.terminal_height = 10;
    update(&mut model, key_event(KeyCode::Enter));
    let body: String = (1..=100).map(|n| format!("Line {n}\n\n")).collect();
    set_detail_body(&mut model, &body);
    model
}

fn review_comment(id: &str, author: &str) -> ReviewComment {
    test_fixtures::review_comment(id, author, &format!("body of {id}"))
}

/// Deliver one thread (root + one reply) and one issue comment to the open
/// detail PR via the real enrichment messages, yielding the focus sequence
/// body → thread root → reply → comment (4 items).
fn seed_detail_enrichment(model: &mut crate::app::model::Model) {
    let threads = vec![thread(
        "t1",
        false,
        vec![review_comment("c1", "alice"), review_comment("c2", "bob")],
    )];
    let comments = vec![issue_comment(10, "carol", "Looks good.")];
    update(
        model,
        Msg::ThreadsArrived {
            pr: pr_key_42(),
            threads,
        },
    );
    update(
        model,
        Msg::IssueCommentsArrived {
            pr: pr_key_42(),
            comments,
        },
    );
}

/// A model in Detail with body arrived and the 4-item focus sequence seeded
/// (body, thread root, reply, issue comment).
fn focusable_detail_model() -> crate::app::model::Model {
    let mut model = model_with_one_pr();
    model.terminal_height = 30;
    update(&mut model, key_event(KeyCode::Enter));
    set_detail_body(&mut model, "The description.");
    seed_detail_enrichment(&mut model);
    model
}

#[test]
fn y_in_detail_copies_the_pr_url_not_the_focused_deep_link() {
    let mut model = focusable_detail_model();
    // Move focus off the body onto a thread root, which has its own deep link.
    // `y` must still copy the PR URL (unlike `o`, which deep-links the focus).
    update(&mut model, key_event(KeyCode::Char('j')));
    assert!(detail_focus_url(&model).is_some(), "focus has a deep link");

    let cmds = update(&mut model, key_event(KeyCode::Char('y')));

    match cmds.as_slice() {
        [Cmd::CopyToClipboard { text }] => {
            assert_eq!(text, "https://github.com/mayfieldiv/legit/pull/42");
        }
        other => panic!("expected one CopyToClipboard, got {other:?}"),
    }
}

#[test]
fn enter_on_selected_pr_transitions_to_detail_and_dispatches_fetch() {
    let mut model = model_with_one_pr();
    assert_eq!(model.view_mode, ViewMode::List);

    let cmds = update(&mut model, key_event(KeyCode::Enter));

    match &model.view_mode {
        ViewMode::Detail(detail) => assert_eq!(
            detail.key,
            pr_key_42(),
            "view must switch to Detail for the selected PR"
        ),
        other => panic!("view must switch to Detail, got {other:?}"),
    }
    // The fetch command must be dispatched
    assert!(
        cmds.iter().any(|c| matches!(c, Cmd::FetchPRDetail { .. })),
        "FetchPRDetail must be dispatched on Enter: {cmds:?}"
    );
}

#[test]
fn enter_constructs_a_fresh_detail_state_with_no_body() {
    // Entering Detail builds a fresh `DetailState`, so there is no stale body to
    // clear by hand — the body starts `None` and the loading placeholder shows.
    let mut model = model_with_one_pr();

    update(&mut model, key_event(KeyCode::Enter));

    assert!(
        !detail_has_body(&model),
        "a freshly-entered detail view must have no body yet"
    );
}

#[test]
fn enter_into_detail_does_not_dispatch_list_files_fetch() {
    // The keypress starts in List but ends in Detail; the just-in-time files
    // fetch is a list-mode concern and must not fire for this keypress, even
    // though the selected PR's files were never requested.
    let mut model = model_with_one_pr();
    assert!(
        !model.enrichment.files.contains_key(&pr_key_42()),
        "precondition: files must not be requested yet"
    );

    let cmds = update(&mut model, key_event(KeyCode::Enter));

    assert!(
        matches!(model.view_mode, ViewMode::Detail(_)),
        "precondition: Enter must have entered Detail"
    );
    assert!(
        !cmds.iter().any(|c| matches!(c, Cmd::FetchFiles { .. })),
        "files fetch must not fire for a keypress that ended in Detail: {cmds:?}"
    );
}

#[test]
fn esc_in_detail_returns_to_list() {
    let mut model = model_with_one_pr();
    update(&mut model, key_event(KeyCode::Enter));
    assert!(matches!(model.view_mode, ViewMode::Detail(_)));

    let cmds = update(&mut model, key_event(KeyCode::Esc));

    assert_eq!(model.view_mode, ViewMode::List, "Esc must return to List");
    assert!(cmds.is_empty(), "Esc should not dispatch any command");
}

#[test]
fn esc_in_detail_drops_the_detail_state() {
    let mut model = model_with_one_pr();
    update(&mut model, key_event(KeyCode::Enter));
    set_detail_body(&mut model, "Some body");

    update(&mut model, key_event(KeyCode::Esc));

    assert_eq!(
        model.view_mode,
        ViewMode::List,
        "Esc must drop the whole DetailState (body included) and return to List"
    );
}

#[test]
fn pr_detail_arrived_stores_detail_when_still_in_detail_view() {
    let mut model = model_with_one_pr();
    update(&mut model, key_event(KeyCode::Enter));
    assert!(!detail_has_body(&model), "detail not yet arrived");

    update(
        &mut model,
        Msg::PRDetailArrived {
            pr: pr_key_42(),
            body: "The body".to_owned(),
        },
    );

    assert!(detail_has_body(&model), "arrived body must be stored");
    assert!(
        detail_body_text(&model).contains("The body"),
        "stored body lines must render the arrived text: {:?}",
        detail_body_text(&model)
    );
}

#[test]
fn pr_detail_arrived_discarded_after_navigating_back() {
    let mut model = model_with_one_pr();
    update(&mut model, key_event(KeyCode::Enter));
    // Navigate back before the fetch completes
    update(&mut model, key_event(KeyCode::Esc));
    assert_eq!(model.view_mode, ViewMode::List);

    update(
        &mut model,
        Msg::PRDetailArrived {
            pr: pr_key_42(),
            body: "The body".to_owned(),
        },
    );

    assert_eq!(
        model.view_mode,
        ViewMode::List,
        "a late-arriving body for a closed view must be discarded"
    );
}

#[test]
fn r_in_detail_dispatches_refetch_and_clears_detail() {
    let mut model = model_with_one_pr();
    update(&mut model, key_event(KeyCode::Enter));
    set_detail_body(&mut model, "current body");

    let cmds = update(&mut model, key_event(KeyCode::Char('r')));

    // Detail cleared to show loading state again
    assert!(
        !detail_has_body(&model),
        "r must clear the detail to show the loading placeholder during refresh"
    );
    // Refetch dispatched
    assert!(
        cmds.iter().any(|c| matches!(c, Cmd::FetchPRDetail { .. })),
        "r must dispatch FetchPRDetail: {cmds:?}"
    );
    // Still in detail view
    assert!(
        matches!(model.view_mode, ViewMode::Detail(_)),
        "r must not exit the detail view"
    );
}

#[test]
fn r_in_detail_dispatches_a_refresh_and_refetches_the_conversation() {
    // `r` dispatches the open PR's review-status / threads / reviews / files
    // refresh as one `Cmd::RefreshPr`, and additionally refetches the body +
    // issue comments the detail view shows on top of that. Enrichment otherwise
    // fetches once per list load, so this doubles as the retry path when an
    // initial fetch left a section stuck.
    let mut model = model_with_one_pr();
    update(&mut model, key_event(KeyCode::Enter));

    let cmds = update(&mut model, key_event(KeyCode::Char('r')));

    assert!(
        cmds.iter().any(|c| matches!(
            c,
            Cmd::RefreshPr {
                key: PrKey { number: 42, .. },
                include_files: true,
                ..
            }
        )),
        "r must enqueue a refresh of the open PR (threads/reviews/files): {cmds:?}"
    );
    assert!(
        cmds.iter().any(|c| matches!(c, Cmd::FetchPRDetail { .. })),
        "r must refetch the open PR's body: {cmds:?}"
    );
    assert!(
        cmds.iter()
            .any(|c| matches!(c, Cmd::FetchIssueComments { number: 42, .. })),
        "r must refetch the open PR's issue comments: {cmds:?}"
    );
    // The selected PR is now marked refreshing for the list-row indicator.
    assert!(model.is_refreshing(&model.list.prs()[0]));
}

#[test]
fn shift_r_in_detail_refreshes_the_open_pr_like_r() {
    // `R` is no longer a no-op in the detail view: with a single focused PR it
    // collapses to the same refresh as `r`, so it too picks up a MERGED/CLOSED
    // transition. It must not exit the detail view (no re-list / prune here).
    let mut model = model_with_one_pr();
    update(&mut model, key_event(KeyCode::Enter));

    let cmds = update(&mut model, key_event(KeyCode::Char('R')));

    assert!(
        cmds.iter().any(|c| matches!(
            c,
            Cmd::RefreshPr {
                key: PrKey { number: 42, .. },
                include_files: true,
                ..
            }
        )),
        "R must enqueue a refresh of the open PR: {cmds:?}"
    );
    assert!(
        matches!(model.view_mode, ViewMode::Detail(_)),
        "R must not exit the detail view"
    );
}

#[test]
fn refresh_in_detail_relabels_a_merged_pr_without_leaving_the_view() {
    // The header reads its merge-status slot off the pooled PR's lifecycle state.
    // A refresh that finds the PR merged applies that state, so the slot relabels
    // from "? merge unknown" to "merged" — and the detail view stays open (the
    // PR is relabeled in place, not pruned), so the header actually shows it.
    let mut model = model_with_one_pr();
    update(&mut model, key_event(KeyCode::Enter));
    update(&mut model, key_event(KeyCode::Char('R')));

    update(
        &mut model,
        Msg::ReviewStatusArrived {
            pr: pr_key_42(),
            status: ReviewStatus {
                additions: 0,
                deletions: 0,
                review_decision: String::new(),
                mergeable: "UNKNOWN".to_owned(),
                state: PRState::Merged,
                last_commit_date: None,
                head_commit_sha: Some("abc123".to_owned()),
            },
        },
    );

    assert_eq!(
        model.list.pr(&pr_key_42()).expect("PR stays pooled").state,
        PRState::Merged,
        "the in-detail refresh applied the merged state for the header to show",
    );
    assert!(
        matches!(model.view_mode, ViewMode::Detail(_)),
        "the view stays on the relabeled PR rather than being pruned out",
    );
}

#[test]
fn r_keeps_existing_threads_and_comments_until_fresh_ones_arrive() {
    // Unlike the body (cleared to show the loading placeholder), the already-
    // rendered thread/comment cards stay up during a refresh — the arriving
    // lists overwrite them, so there is no flicker through "Loading threads…".
    let mut model = focusable_detail_model();

    update(&mut model, key_event(KeyCode::Char('r')));

    assert!(
        model.enrichment.threads_for(&pr_key_42()).is_some(),
        "r must not clear the threads already on screen"
    );
    assert!(
        model.enrichment.comments_for(&pr_key_42()).is_some(),
        "r must not clear the comments already on screen"
    );
}

#[test]
fn r_in_list_mode_refreshes_without_opening_the_detail() {
    // 'r' in list mode refreshes the selected PR through the queue (a
    // `Cmd::RefreshPr`). It must not refetch the detail body — that PR's detail
    // view isn't open — so no `FetchPRDetail` is dispatched.
    let mut model = model_with_one_pr();
    assert_eq!(model.view_mode, ViewMode::List);

    let cmds = update(&mut model, key_event(KeyCode::Char('r')));

    assert!(
        !cmds.iter().any(|c| matches!(c, Cmd::FetchPRDetail { .. })),
        "r in list mode must not dispatch FetchPRDetail: {cmds:?}"
    );
    assert!(
        cmds.iter().any(|c| matches!(c, Cmd::RefreshPr { .. })),
        "r in list mode must enqueue and dispatch a refresh: {cmds:?}"
    );
    assert!(
        matches!(model.view_mode, ViewMode::List),
        "r in list mode must not enter the detail view"
    );
}

#[test]
fn entering_detail_starts_scroll_at_zero() {
    let mut model = model_with_one_pr();

    update(&mut model, key_event(KeyCode::Enter));

    assert_eq!(
        detail_scroll(&model),
        0,
        "a freshly-entered detail view must start scrolled to the top"
    );
}

// ── Focus selection (j/k cycles the focusable items) ────────────────────────

#[test]
fn entering_detail_starts_focus_on_the_body() {
    let mut model = model_with_one_pr();

    update(&mut model, key_event(KeyCode::Enter));

    assert_eq!(
        detail_focus(&model),
        0,
        "a freshly-entered detail view must focus the body (item 0)"
    );
}

#[test]
fn j_advances_focus_through_items_and_clamps_at_the_last() {
    // Sequence: body(0) -> thread root(1) -> reply(2) -> issue comment(3).
    let mut model = focusable_detail_model();
    assert_eq!(detail_focus(&model), 0);

    update(&mut model, key_event(KeyCode::Char('j')));
    assert_eq!(detail_focus(&model), 1, "j must focus the thread root");

    update(&mut model, key_event(KeyCode::Char('j')));
    assert_eq!(detail_focus(&model), 2, "j must focus the reply");

    update(&mut model, key_event(KeyCode::Char('j')));
    assert_eq!(detail_focus(&model), 3, "j must focus the issue comment");

    update(&mut model, key_event(KeyCode::Char('j')));
    assert_eq!(detail_focus(&model), 3, "j must clamp at the last item");
}

#[test]
fn k_retreats_focus_and_clamps_at_the_body() {
    let mut model = focusable_detail_model();
    update(&mut model, key_event(KeyCode::Char('j')));
    update(&mut model, key_event(KeyCode::Char('j')));
    assert_eq!(detail_focus(&model), 2);

    update(&mut model, key_event(KeyCode::Char('k')));
    assert_eq!(detail_focus(&model), 1, "k must move focus back");

    update(&mut model, key_event(KeyCode::Char('k')));
    update(&mut model, key_event(KeyCode::Char('k')));
    assert_eq!(detail_focus(&model), 0, "k must clamp at the body");
}

#[test]
fn arrow_keys_move_focus_like_j_and_k() {
    let mut model = focusable_detail_model();

    update(&mut model, key_event(KeyCode::Down));
    assert_eq!(detail_focus(&model), 1, "Down must advance focus");

    update(&mut model, key_event(KeyCode::Up));
    assert_eq!(detail_focus(&model), 0, "Up must retreat focus");
}

#[test]
fn left_click_on_a_visible_detail_card_focuses_that_card() {
    let mut model = focusable_detail_model();
    model.terminal_width = 80;
    assert_eq!(detail_focus(&model), 0);
    let thread_root = measured_content(&model).item_ranges[1].clone();
    let click_row = crate::app::detail_layout::HEADER_BASE_HEIGHT + thread_root.start as u16;

    update(&mut model, mouse_down_event(0, click_row));

    assert_eq!(detail_focus(&model), 1);
    assert_eq!(
        detail_focus_url(&model),
        Some("https://example.test/r/c1".to_owned())
    );
}

#[test]
fn j_with_no_threads_or_comments_keeps_focus_on_the_body() {
    // Only the body is focusable while enrichment hasn't arrived.
    let mut model = model_with_one_pr();
    update(&mut model, key_event(KeyCode::Enter));
    set_detail_body(&mut model, "The description.");

    update(&mut model, key_event(KeyCode::Char('j')));

    assert_eq!(
        detail_focus(&model),
        0,
        "with a lone body item, j has nowhere to go"
    );
}

#[test]
fn threads_arrival_follows_the_focused_comment_to_its_new_index() {
    // Focus the last item (the issue comment), then deliver a fresh thread
    // list that removes the thread (and its reply) — the comment is still in
    // the rebuilt sequence, so the focus follows its identity to index 1.
    let mut model = focusable_detail_model();
    for _ in 0..3 {
        update(&mut model, key_event(KeyCode::Char('j')));
    }
    assert_eq!(detail_focus(&model), 3);

    update(
        &mut model,
        Msg::ThreadsArrived {
            pr: pr_key_42(),
            threads: Vec::new(),
        },
    );

    assert_eq!(
        detail_focus(&model),
        1,
        "the focus must follow the comment into the rebuilt sequence"
    );
    assert_eq!(
        detail_focus_url(&model).as_deref(),
        Some("https://example.test/c/10"),
        "the focused card must still be the comment the user selected"
    );
}

#[test]
fn late_thread_arrival_keeps_the_focused_comments_identity() {
    // Focus the trailing issue comment, then deliver a fresh thread list that
    // inserts a second thread above it. A positional focus would silently
    // retarget to whatever card landed at the old index; the identity-keyed
    // focus must follow the comment to its new position so `o` still opens
    // the card the user selected.
    let mut model = focusable_detail_model();
    for _ in 0..3 {
        update(&mut model, key_event(KeyCode::Char('j')));
    }
    assert_eq!(
        detail_focus_url(&model).as_deref(),
        Some("https://example.test/c/10"),
        "precondition: the issue comment is focused"
    );

    update(
        &mut model,
        Msg::ThreadsArrived {
            pr: pr_key_42(),
            threads: vec![
                thread("t0", false, vec![review_comment("c0", "dave")]),
                thread(
                    "t1",
                    false,
                    vec![review_comment("c1", "alice"), review_comment("c2", "bob")],
                ),
            ],
        },
    );

    assert_eq!(
        detail_focus(&model),
        4,
        "the focused comment must move down one slot with the inserted thread"
    );
    let cmds = update(&mut model, key_event(KeyCode::Char('o')));
    assert_eq!(
        open_url(&cmds),
        "https://example.test/c/10",
        "o must open the card the user focused, not the card at the old index"
    );
}

#[test]
fn wheel_in_detail_scrolls_the_viewport_without_moving_focus() {
    // The wheel is not a selection device: ticks move the viewport only,
    // leaving the focused card (and the follow anchor) untouched — unlike
    // the arrow keys the terminal would synthesize without mouse capture.
    let mut model = tall_focusable_detail_model();
    update(&mut model, key_event(KeyCode::Char('j')));
    let focus_before = detail_focus_url(&model);
    let scroll_before = detail_scroll(&model);

    update(&mut model, wheel_event(true));
    update(&mut model, wheel_event(true));

    assert_eq!(
        detail_scroll(&model),
        scroll_before + 6,
        "two wheel-down ticks must scroll the viewport by 3 lines each"
    );
    assert_eq!(
        detail_focus_url(&model),
        focus_before,
        "wheel scrolling must not move the focus"
    );

    update(&mut model, wheel_event(false));
    update(&mut model, wheel_event(false));
    assert_eq!(
        detail_scroll(&model),
        scroll_before,
        "wheel-up ticks must scroll back without yanking to the focused card"
    );
}

#[test]
fn page_down_in_detail_scrolls_by_ten() {
    let mut model = scrollable_detail_model();

    update(&mut model, key_event(KeyCode::PageDown));
    assert_eq!(detail_scroll(&model), 10, "PageDown must scroll down by 10");
}

#[test]
fn page_up_in_detail_scrolls_by_ten_and_clamps_at_zero() {
    let mut model = scrollable_detail_model();
    update(&mut model, key_event(KeyCode::PageDown));
    assert_eq!(detail_scroll(&model), 10);

    update(&mut model, key_event(KeyCode::PageUp));
    assert_eq!(detail_scroll(&model), 0, "PageUp must scroll up by 10");

    // Another PageUp from zero must not underflow.
    update(&mut model, key_event(KeyCode::PageUp));
    assert_eq!(
        detail_scroll(&model),
        0,
        "PageUp must clamp at zero, not underflow"
    );
}

// ── o opens the focused item's URL ──────────────────────────────────────────

/// The URL of the single `Cmd::OpenUrl` in `cmds`; panics otherwise.
fn open_url(cmds: &[Cmd]) -> &str {
    match cmds {
        [Cmd::OpenUrl { url }] => url,
        other => panic!("expected exactly one OpenUrl, got {other:?}"),
    }
}

#[test]
fn o_on_a_focused_thread_root_opens_its_deep_link() {
    let mut model = focusable_detail_model();
    update(&mut model, key_event(KeyCode::Char('j'))); // thread root

    let cmds = update(&mut model, key_event(KeyCode::Char('o')));

    assert_eq!(
        open_url(&cmds),
        "https://example.test/r/c1",
        "o must deep-link to the thread's root comment"
    );
}

#[test]
fn o_on_a_focused_reply_opens_the_reply_deep_link() {
    let mut model = focusable_detail_model();
    update(&mut model, key_event(KeyCode::Char('j')));
    update(&mut model, key_event(KeyCode::Char('j'))); // the reply

    let cmds = update(&mut model, key_event(KeyCode::Char('o')));

    assert_eq!(
        open_url(&cmds),
        "https://example.test/r/c2",
        "o must deep-link to the specific reply"
    );
}

#[test]
fn o_on_a_focused_issue_comment_opens_its_url() {
    let mut model = focusable_detail_model();
    for _ in 0..3 {
        update(&mut model, key_event(KeyCode::Char('j'))); // the issue comment
    }

    let cmds = update(&mut model, key_event(KeyCode::Char('o')));

    assert_eq!(open_url(&cmds), "https://example.test/c/10");
}

#[test]
fn o_on_the_body_falls_back_to_the_pr_url() {
    let mut model = focusable_detail_model();
    assert_eq!(detail_focus(&model), 0);

    let cmds = update(&mut model, key_event(KeyCode::Char('o')));

    assert_eq!(
        open_url(&cmds),
        "https://github.com/mayfieldiv/legit/pull/42",
        "o on the body (focus 0) must open the PR itself"
    );
}

// ── t / b filter toggles ────────────────────────────────────────────────────

#[test]
fn t_toggles_resolved_thread_visibility() {
    let mut model = focusable_detail_model();
    assert!(!model.show_resolved, "resolved threads hidden by default");

    update(&mut model, key_event(KeyCode::Char('t')));
    assert!(model.show_resolved, "t must reveal resolved threads");

    update(&mut model, key_event(KeyCode::Char('t')));
    assert!(!model.show_resolved, "t must toggle back off");
}

#[test]
fn b_toggles_bot_comment_visibility() {
    let mut model = focusable_detail_model();
    assert!(model.show_bot_comments, "bot comments shown by default");

    update(&mut model, key_event(KeyCode::Char('b')));
    assert!(!model.show_bot_comments, "b must hide bot comments");

    update(&mut model, key_event(KeyCode::Char('b')));
    assert!(model.show_bot_comments, "b must toggle back on");
}

#[test]
fn hiding_bots_clamps_a_focus_stranded_past_the_shrunk_sequence() {
    // Make the trailing issue comment a bot, focus it, then hide bots: the
    // sequence loses its last item and the focus must clamp back into range.
    let mut model = focusable_detail_model();
    update(
        &mut model,
        Msg::IssueCommentsArrived {
            pr: pr_key_42(),
            comments: vec![IssueComment {
                is_bot: true,
                ..issue_comment(11, "ci", "bot says hi")
            }],
        },
    );
    for _ in 0..3 {
        update(&mut model, key_event(KeyCode::Char('j')));
    }
    assert_eq!(detail_focus(&model), 3, "focused on the bot comment");

    update(&mut model, key_event(KeyCode::Char('b')));

    assert_eq!(
        detail_focus(&model),
        2,
        "hiding bots must clamp the focus to the shrunk sequence"
    );
}

#[test]
fn filter_toggles_persist_across_detail_views() {
    // The filters live on the Model, not the DetailState: leaving and
    // re-entering detail must keep the user's t/b preferences.
    let mut model = focusable_detail_model();
    update(&mut model, key_event(KeyCode::Char('t')));
    update(&mut model, key_event(KeyCode::Char('b')));

    update(&mut model, key_event(KeyCode::Esc));
    update(&mut model, key_event(KeyCode::Enter));

    assert!(model.show_resolved, "t survives re-entering detail");
    assert!(!model.show_bot_comments, "b survives re-entering detail");
}

// ── Enter toggles card <details> ────────────────────────────────────────────

/// The expansion set of the open detail view; panics if not in Detail mode.
fn detail_expanded(model: &crate::app::model::Model) -> &std::collections::HashSet<String> {
    match &model.view_mode {
        ViewMode::Detail(detail) => &detail.expanded,
        ViewMode::List => panic!("expected Detail mode"),
    }
}

/// A markdown body that is a single `<details>` group, so the card holding it
/// has something for Enter to toggle.
const DETAILS_BODY: &str = "<details>\n<summary>AI Prompt</summary>\n\nhidden body\n\n</details>";

/// A model in Detail with one issue comment whose body is a `<details>` group,
/// focused on that comment (focus index 1: the body is 0, no threads seeded).
fn detail_model_focused_on_details_comment() -> crate::app::model::Model {
    let mut model = model_with_one_pr();
    model.terminal_height = 30;
    update(&mut model, key_event(KeyCode::Enter));
    set_detail_body(&mut model, "The description.");
    update(
        &mut model,
        Msg::IssueCommentsArrived {
            pr: pr_key_42(),
            comments: vec![test_fixtures::issue_comment(10, "carol", DETAILS_BODY)],
        },
    );
    update(&mut model, key_event(KeyCode::Char('j')));
    model
}

#[test]
fn enter_toggles_the_focused_cards_details() {
    let mut model = detail_model_focused_on_details_comment();
    assert_eq!(detail_focus(&model), 1, "focused on the details comment");

    update(&mut model, key_event(KeyCode::Enter));
    assert!(
        detail_expanded(&model).contains("https://example.test/c/10"),
        "enter must expand the focused card's <details>"
    );

    update(&mut model, key_event(KeyCode::Enter));
    assert!(
        detail_expanded(&model).is_empty(),
        "a second enter must collapse the card again"
    );
}

#[test]
fn enter_on_a_card_without_details_is_a_no_op() {
    // c1's body is plain text; with nothing to toggle, Enter must leave the
    // expansion set untouched (mirroring the TS toggleAll early-return).
    let mut model = focusable_detail_model();
    update(&mut model, key_event(KeyCode::Char('j'))); // thread root (c1)

    let cmds = update(&mut model, key_event(KeyCode::Enter));

    assert!(cmds.is_empty(), "enter dispatches nothing");
    assert!(
        detail_expanded(&model).is_empty(),
        "a detail-less card has nothing to toggle"
    );
}

#[test]
fn enter_on_the_body_toggles_its_details_via_the_sentinel_key() {
    // The description has no URL, so its <details> expansion is keyed by the
    // BODY_DETAILS_KEY sentinel.
    let mut model = model_with_one_pr();
    model.terminal_height = 30;
    update(&mut model, key_event(KeyCode::Enter));
    set_detail_body(&mut model, DETAILS_BODY);
    assert_eq!(detail_focus(&model), 0, "the body is focused");

    update(&mut model, key_event(KeyCode::Enter));
    assert!(
        detail_expanded(&model).contains(crate::app::detail_layout::BODY_DETAILS_KEY),
        "enter must expand the body's <details> under the sentinel key"
    );

    update(&mut model, key_event(KeyCode::Enter));
    assert!(
        detail_expanded(&model).is_empty(),
        "a second enter collapses the body's <details>"
    );
}

#[test]
fn enter_on_a_detail_less_body_is_a_no_op() {
    let mut model = focusable_detail_model();
    assert_eq!(detail_focus(&model), 0);

    let cmds = update(&mut model, key_event(KeyCode::Enter));

    assert!(cmds.is_empty(), "enter on the body dispatches nothing");
    assert!(
        detail_expanded(&model).is_empty(),
        "a detail-less body has nothing to toggle"
    );
}

// ── Scroll follows focus ────────────────────────────────────────────────────

/// The open detail view's measured layout, via the same canonical
/// `detail_layout::detail_content` the view renders (at the same fixed epoch
/// `update`'s own measurements use). Panics if not in Detail mode with an
/// arrived body.
fn measured_content(model: &crate::app::model::Model) -> crate::app::detail_layout::DetailContent {
    let ViewMode::Detail(detail) = &model.view_mode else {
        panic!("expected Detail mode");
    };
    let pr = model.list.pr(&detail.key).expect("pr in list");
    crate::app::detail_layout::detail_content(
        model,
        pr,
        detail.body.as_ref().expect("body arrived"),
        detail,
        model.terminal_width,
        chrono::DateTime::UNIX_EPOCH,
    )
}

/// The line range the focused item occupies in the measured layout.
fn focused_item_range(model: &crate::app::model::Model) -> std::ops::Range<usize> {
    let ViewMode::Detail(detail) = &model.view_mode else {
        panic!("expected Detail mode");
    };
    measured_content(model).item_ranges[detail.focus.index()].clone()
}

/// A focusable detail model with a body tall enough that the thread and
/// comment cards start below the fold of its small viewport.
fn tall_focusable_detail_model() -> crate::app::model::Model {
    let mut model = model_with_one_pr();
    model.terminal_height = (crate::app::detail_layout::HEADER_BASE_HEIGHT + 1) + 8;
    model.terminal_width = 80;
    update(&mut model, key_event(KeyCode::Enter));
    let body: String = (1..=30).map(|n| format!("Line {n}\n\n")).collect();
    set_detail_body(&mut model, &body);
    seed_detail_enrichment(&mut model);
    model
}

#[test]
fn focusing_an_offscreen_card_scrolls_it_into_view() {
    let mut model = tall_focusable_detail_model();
    assert_eq!(detail_scroll(&model), 0);

    // Focus the thread root, which sits far below the 8-row viewport.
    update(&mut model, key_event(KeyCode::Char('j')));

    let range = focused_item_range(&model);
    let viewport =
        (model.terminal_height - (crate::app::detail_layout::HEADER_BASE_HEIGHT + 1)) as usize;
    let scroll = detail_scroll(&model);
    assert!(
        scroll <= range.start && range.end <= scroll + viewport,
        "the focused card (lines {range:?}) must be fully inside the viewport \
         (scroll {scroll}, {viewport} rows)"
    );
}

#[test]
fn focusing_back_to_the_body_scrolls_to_its_top() {
    let mut model = tall_focusable_detail_model();
    update(&mut model, key_event(KeyCode::Char('j')));
    assert!(detail_scroll(&model) > 0, "precondition: scrolled down");

    update(&mut model, key_event(KeyCode::Char('k')));

    assert_eq!(
        detail_scroll(&model),
        0,
        "re-focusing the body (which starts at line 0) must scroll back to the top"
    );
}

#[test]
fn scroll_clamp_covers_the_thread_and_conversation_sections() {
    // PageDown far past the end: the clamp must allow reaching the bottom of
    // the conversation section, not stop at the description+checks height the
    // M7 clamp measured.
    let mut model = tall_focusable_detail_model();
    let max_scroll = max_detail_scroll(&model);

    for _ in 0..50 {
        update(&mut model, key_event(KeyCode::PageDown));
    }

    assert_eq!(
        detail_scroll(&model),
        max_scroll,
        "PageDown must clamp at the bottom of the FULL content (threads + conversation included)"
    );
}

/// The open detail view's true max scroll: full measured content minus the
/// body viewport.
fn max_detail_scroll(model: &crate::app::model::Model) -> usize {
    let viewport =
        usize::from(model.terminal_height - (crate::app::detail_layout::HEADER_BASE_HEIGHT + 1));
    measured_content(model).lines.len().saturating_sub(viewport)
}

#[test]
fn refreshing_to_a_shorter_body_reclamps_scroll_so_page_up_stays_live() {
    // `r` deliberately preserves the scroll offset across a refresh. When the
    // refetched body comes back shorter, the preserved offset can sit past the
    // new content's last screenful — it must re-clamp when the body arrives,
    // or the user's next PageUp presses are visually dead unwinding phantom
    // offset (the same drift bug the over-scroll test guards against on the
    // PageDown path).
    let mut model = model_with_one_pr();
    model.terminal_height = (crate::app::detail_layout::HEADER_BASE_HEIGHT + 1) + 6;
    update(&mut model, key_event(KeyCode::Enter));
    let tall: String = (1..=40).map(|n| format!("Line {n}\n\n")).collect();
    update(
        &mut model,
        Msg::PRDetailArrived {
            pr: pr_key_42(),
            body: tall,
        },
    );
    for _ in 0..50 {
        update(&mut model, key_event(KeyCode::PageDown));
    }
    assert!(
        detail_scroll(&model) > 20,
        "precondition: scrolled deep into the tall body"
    );

    update(&mut model, key_event(KeyCode::Char('r')));
    update(
        &mut model,
        Msg::PRDetailArrived {
            pr: pr_key_42(),
            body: "Short.".to_owned(),
        },
    );

    assert_eq!(
        detail_scroll(&model),
        max_detail_scroll(&model),
        "the preserved offset must re-clamp to the shorter refreshed content"
    );
}

#[test]
fn showing_resolved_threads_keeps_the_focused_card_in_view() {
    // Focus the issue comment at the bottom (the scroll followed it). Toggling
    // `t` reveals a resolved thread ABOVE it: the focus keeps the comment's
    // identity (its index shifts down by the inserted card), and the toggle
    // must scroll the card back into view, exactly like a j/k focus move
    // would.
    let mut model = tall_focusable_detail_model();
    let resolved_body: String = (1..=6).map(|n| format!("Resolved para {n}\n\n")).collect();
    update(
        &mut model,
        Msg::ThreadsArrived {
            pr: pr_key_42(),
            threads: vec![
                thread(
                    "done",
                    true,
                    vec![ReviewComment {
                        body: resolved_body,
                        ..review_comment("c9", "bob")
                    }],
                ),
                thread("t1", false, vec![review_comment("c1", "alice")]),
            ],
        },
    );
    // Walk the focus to the last item (the issue comment); the scroll follows.
    for _ in 0..5 {
        update(&mut model, key_event(KeyCode::Char('j')));
    }
    let focused_before = detail_focus(&model);
    let url_before = detail_focus_url(&model);

    update(&mut model, key_event(KeyCode::Char('t')));

    assert_eq!(
        detail_focus_url(&model),
        url_before,
        "revealing threads must not change which card is focused"
    );
    assert_eq!(
        detail_focus(&model),
        focused_before + 1,
        "the revealed resolved thread above shifts the focused card's index"
    );
    let range = focused_item_range(&model);
    let viewport =
        (model.terminal_height - (crate::app::detail_layout::HEADER_BASE_HEIGHT + 1)) as usize;
    let scroll = detail_scroll(&model);
    assert!(
        scroll <= range.start && range.end <= scroll + viewport,
        "the focused card (lines {range:?}) must stay fully inside the viewport \
         after a filter toggle (scroll {scroll}, {viewport} rows)"
    );
}

#[test]
fn body_arrival_scrolls_the_already_focused_card_into_view() {
    // While the body is still loading, j/k already move the focus (the
    // threads/comments arrived independently), but there is nothing to
    // measure, so the scroll stays at 0. When the body lands, the focused
    // card can sit far below the viewport — the arrival must scroll it into
    // view, not leave the focus border off-screen with o/Enter acting on an
    // invisible card.
    let mut model = model_with_one_pr();
    model.terminal_height = (crate::app::detail_layout::HEADER_BASE_HEIGHT + 1) + 8;
    model.terminal_width = 80;
    update(&mut model, key_event(KeyCode::Enter));
    seed_detail_enrichment(&mut model);
    for _ in 0..3 {
        update(&mut model, key_event(KeyCode::Char('j')));
    }
    assert_eq!(detail_focus(&model), 3, "precondition: comment focused");
    assert_eq!(detail_scroll(&model), 0, "precondition: nothing to scroll");

    let body: String = (1..=30).map(|n| format!("Line {n}\n\n")).collect();
    set_detail_body(&mut model, &body);

    let range = focused_item_range(&model);
    let viewport =
        (model.terminal_height - (crate::app::detail_layout::HEADER_BASE_HEIGHT + 1)) as usize;
    let scroll = detail_scroll(&model);
    assert!(
        scroll <= range.start && range.end <= scroll + viewport,
        "the focused card (lines {range:?}) must be scrolled into view when \
         the body arrives (scroll {scroll}, {viewport} rows)"
    );
}

#[test]
fn expanding_the_focused_card_brings_its_grown_tail_into_view() {
    // Expanding a card's <details> grows it in place (same identity, same first
    // line), so it is the one change the follow anchor can't see — Enter forces
    // the follow, which scrolls down to the expanded card's tail (capped at its
    // first line for cards taller than the viewport).
    let mut model = tall_focusable_detail_model();
    let inner: String = (1..=40).map(|n| format!("Inner {n}\n\n")).collect();
    let details_body = format!("<details>\n<summary>More</summary>\n\n{inner}</details>");
    update(
        &mut model,
        Msg::IssueCommentsArrived {
            pr: pr_key_42(),
            comments: vec![issue_comment(10, "carol", &details_body)],
        },
    );
    for _ in 0..3 {
        update(&mut model, key_event(KeyCode::Char('j')));
    }
    let collapsed_range = focused_item_range(&model);

    update(&mut model, key_event(KeyCode::Enter));

    let range = focused_item_range(&model);
    assert!(
        range.end > collapsed_range.end,
        "precondition: expanding the card's <details> must grow it"
    );
    let viewport =
        (model.terminal_height - (crate::app::detail_layout::HEADER_BASE_HEIGHT + 1)) as usize;
    let scroll = detail_scroll(&model);
    assert!(
        scroll <= range.start && (range.end <= scroll + viewport || scroll == range.start),
        "the expanded card (lines {range:?}) must be followed into view \
         (scroll {scroll}, {viewport} rows)"
    );
}

#[test]
fn threads_arrival_that_shrinks_the_content_reclamps_scroll() {
    // Scrolled to the bottom of a view whose threads section is long; a fresh
    // (empty) thread list shortens the content, so the offset must follow it
    // down — not strand past the new end.
    let mut model = tall_focusable_detail_model();
    for _ in 0..50 {
        update(&mut model, key_event(KeyCode::PageDown));
    }
    let deep_scroll = detail_scroll(&model);

    update(
        &mut model,
        Msg::ThreadsArrived {
            pr: pr_key_42(),
            threads: Vec::new(),
        },
    );

    let max_scroll = max_detail_scroll(&model);
    assert!(
        max_scroll < deep_scroll,
        "precondition: dropping the thread cards must shorten the content"
    );
    assert_eq!(
        detail_scroll(&model),
        max_scroll,
        "the scroll must re-clamp when arriving threads shrink the content"
    );
}

#[test]
fn growing_the_terminal_reclamps_the_detail_scroll() {
    // A taller terminal shrinks the max scroll (more content fits per
    // screenful); the stored offset must follow it down so the next PageUp
    // visibly moves.
    let mut model = tall_focusable_detail_model();
    for _ in 0..50 {
        update(&mut model, key_event(KeyCode::PageDown));
    }
    let small_viewport_scroll = detail_scroll(&model);

    update(
        &mut model,
        Msg::TerminalEvent(ratatui::crossterm::event::Event::Resize(80, 40)),
    );

    let max_scroll = max_detail_scroll(&model);
    assert!(
        max_scroll < small_viewport_scroll,
        "precondition: the taller viewport must lower the max scroll"
    );
    assert_eq!(
        detail_scroll(&model),
        max_scroll,
        "a resize must re-clamp the detail scroll to the new viewport"
    );
}

#[test]
fn esc_in_detail_drops_scroll_with_the_detail_state() {
    let mut model = scrollable_detail_model();
    update(&mut model, key_event(KeyCode::PageDown));
    assert_eq!(detail_scroll(&model), 10);

    update(&mut model, key_event(KeyCode::Esc));

    // Esc drops the whole DetailState, so the next open structurally starts at
    // the top — there is no scroll field to leak across opens.
    assert_eq!(model.view_mode, ViewMode::List, "Esc returns to List");
}

#[test]
fn over_scrolling_clamps_to_the_last_screenful_and_page_up_stays_live() {
    // Regression for the unbounded-drift bug: holding PageDown far past the end
    // must pin the offset at the last screenful, not let it accumulate —
    // otherwise the next PageUp presses are visually dead until the inflated
    // offset works back down into view.
    // Reference the base chrome-row count (the fixture PR has no labels, so its
    // Label Chip band is empty and the header stays at the base height) so a
    // future layout change keeps this regression test in sync with the clamp it
    // guards (rather than a hardcoded literal that would silently desync).
    let chrome_rows = crate::app::detail_layout::HEADER_BASE_HEIGHT + 1;
    let mut model = model_with_one_pr();
    model.terminal_height = chrome_rows + 6; // body viewport = 6 rows
    update(&mut model, key_event(KeyCode::Enter));
    let body: String = (1..=20).map(|n| format!("Line {n}\n\n")).collect();
    set_detail_body(&mut model, &body);

    // The true max scroll: the full body content (description plus the
    // threads/comments loading placeholders here) minus the viewport, measured
    // via the same layout the view renders.
    let max_scroll = max_detail_scroll(&model);
    assert!(max_scroll > 1, "test body must be taller than the viewport");

    // Hold PageDown far past the end.
    for _ in 0..(max_scroll + 50) {
        update(&mut model, key_event(KeyCode::PageDown));
    }
    assert_eq!(
        detail_scroll(&model),
        max_scroll,
        "over-scroll must clamp to the last screenful, not drift past it"
    );

    // A single PageUp must visibly move — it can't be eaten unwinding phantom
    // offset, because there is none.
    update(&mut model, key_event(KeyCode::PageUp));
    assert_eq!(
        detail_scroll(&model),
        max_scroll.saturating_sub(10),
        "one PageUp after over-scroll must step back by exactly one page"
    );
}
