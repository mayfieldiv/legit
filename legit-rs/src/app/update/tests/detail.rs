use chrono::TimeZone;
use ratatui::crossterm::event::KeyCode;

use ratatui::text::Line;

use crate::{
    app::{cmd::Cmd, model::ViewMode, msg::Msg, update::update},
    git_remote::RepoInfo,
    github::rest::PrKey,
    github::types::{FullReviewThread, IssueComment, ReviewComment},
    secret::Secret,
};

/// True when the open detail view has its rendered body cached. `false` if not
/// in Detail mode or the body hasn't arrived. Lets the tests assert on the
/// consolidated `ViewMode::Detail(DetailState)` shape without repeating the
/// match.
fn detail_has_body(model: &crate::app::model::Model) -> bool {
    matches!(&model.view_mode, ViewMode::Detail(detail) if detail.body.is_some())
}

/// The concatenated text of the open detail view's rendered body lines; panics
/// if not in Detail mode or the body hasn't arrived.
fn detail_body_text(model: &crate::app::model::Model) -> String {
    match &model.view_mode {
        ViewMode::Detail(detail) => detail
            .body
            .as_ref()
            .expect("body arrived")
            .iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.as_ref()))
            .collect(),
        ViewMode::List => panic!("expected Detail mode"),
    }
}

/// The focused item index of the open detail view; panics if not in Detail mode.
fn detail_focus(model: &crate::app::model::Model) -> usize {
    match &model.view_mode {
        ViewMode::Detail(detail) => detail.focused_index,
        ViewMode::List => panic!("expected Detail mode"),
    }
}

/// The scroll offset of the open detail view; panics if not in Detail mode.
fn detail_scroll(model: &crate::app::model::Model) -> u16 {
    match &model.view_mode {
        ViewMode::Detail(detail) => detail.scroll,
        ViewMode::List => panic!("expected Detail mode"),
    }
}

/// Set the open detail view's body to lines rendered from `body`; panics if not
/// in Detail mode. Mirrors how `Msg::PRDetailArrived` caches the description.
fn set_detail_body(model: &mut crate::app::model::Model, body: &str) {
    let lines: Vec<Line<'static>> = crate::view::detail::render_description_lines(body);
    match &mut model.view_mode {
        ViewMode::Detail(detail) => detail.body = Some(lines),
        ViewMode::List => panic!("expected Detail mode"),
    }
}

use super::{enriched_model, key_event};

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
    ReviewComment {
        id: id.to_owned(),
        author: author.to_owned(),
        body: format!("body of {id}"),
        created_at: chrono::Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap(),
        url: format!("https://example.test/r/{id}"),
        is_bot: false,
    }
}

/// Deliver one thread (root + one reply) and one issue comment to the open
/// detail PR via the real enrichment messages, yielding the focus sequence
/// body → thread root → reply → comment (4 items).
fn seed_detail_enrichment(model: &mut crate::app::model::Model) {
    let threads = vec![FullReviewThread {
        id: "t1".to_owned(),
        is_resolved: false,
        path: "src/lib.rs".to_owned(),
        line: Some(12),
        comments: vec![review_comment("c1", "alice"), review_comment("c2", "bob")],
    }];
    let comments = vec![IssueComment {
        id: 10,
        author: "carol".to_owned(),
        body: "Looks good.".to_owned(),
        created_at: chrono::Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap(),
        url: "https://example.test/c/10".to_owned(),
        is_bot: false,
    }];
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
fn r_in_list_mode_does_not_dispatch_fetch_pr_detail() {
    // 'r' in list mode is unbound (no handler). It must not accidentally
    // dispatch FetchPRDetail.
    let mut model = model_with_one_pr();
    assert_eq!(model.view_mode, ViewMode::List);

    let cmds = update(&mut model, key_event(KeyCode::Char('r')));

    assert!(
        !cmds.iter().any(|c| matches!(c, Cmd::FetchPRDetail { .. })),
        "r in list mode must not dispatch FetchPRDetail: {cmds:?}"
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
fn threads_arrival_clamps_an_out_of_range_focus() {
    // Focus the last item (the issue comment), then deliver a fresh thread
    // list that removes the thread (and its reply) — the rebuilt sequence is
    // body + comment (2 items), so focus must clamp into range.
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
        "focus must clamp to the last item of the rebuilt sequence"
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
                id: 11,
                author: "ci".to_owned(),
                body: "bot says hi".to_owned(),
                created_at: chrono::Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap(),
                url: "https://example.test/c/11".to_owned(),
                is_bot: true,
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

// ── Enter toggles card expansion ────────────────────────────────────────────

/// The expansion set of the open detail view; panics if not in Detail mode.
fn detail_expanded(model: &crate::app::model::Model) -> &std::collections::HashSet<String> {
    match &model.view_mode {
        ViewMode::Detail(detail) => &detail.expanded,
        ViewMode::List => panic!("expected Detail mode"),
    }
}

#[test]
fn enter_toggles_the_focused_cards_expansion() {
    let mut model = focusable_detail_model();
    update(&mut model, key_event(KeyCode::Char('j'))); // thread root (c1)

    update(&mut model, key_event(KeyCode::Enter));
    assert!(
        detail_expanded(&model).contains("https://example.test/r/c1"),
        "enter must mark the focused card expanded"
    );

    update(&mut model, key_event(KeyCode::Enter));
    assert!(
        detail_expanded(&model).is_empty(),
        "a second enter must collapse the card again"
    );
}

#[test]
fn enter_on_the_body_does_not_touch_expansion_state() {
    let mut model = focusable_detail_model();
    assert_eq!(detail_focus(&model), 0);

    let cmds = update(&mut model, key_event(KeyCode::Enter));

    assert!(cmds.is_empty(), "enter on the body dispatches nothing");
    assert!(
        detail_expanded(&model).is_empty(),
        "the body has no expansion state to toggle"
    );
}

// ── Scroll follows focus ────────────────────────────────────────────────────

/// The line range the focused item occupies, via the same layout the view
/// renders (the canonical `view::detail::detail_content`).
fn focused_item_range(model: &crate::app::model::Model) -> std::ops::Range<usize> {
    let ViewMode::Detail(detail) = &model.view_mode else {
        panic!("expected Detail mode");
    };
    let pr = model.list.pr(&detail.key).expect("pr in list");
    let content = crate::view::detail::detail_content(
        model,
        pr,
        detail.body.as_ref().expect("body arrived"),
        detail,
        model.terminal_width,
        chrono::DateTime::UNIX_EPOCH,
    );
    content.item_ranges[detail.focused_index].clone()
}

/// A focusable detail model with a body tall enough that the thread and
/// comment cards start below the fold of its small viewport.
fn tall_focusable_detail_model() -> crate::app::model::Model {
    let mut model = model_with_one_pr();
    model.terminal_height = crate::view::detail::CHROME_ROWS + 8;
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
    let viewport = (model.terminal_height - crate::view::detail::CHROME_ROWS) as usize;
    let scroll = detail_scroll(&model) as usize;
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
    let ViewMode::Detail(detail) = &model.view_mode else {
        panic!("expected Detail mode");
    };
    let pr = model.list.pr(&detail.key).expect("pr in list");
    let content_lines = crate::view::detail::detail_content(
        &model,
        pr,
        detail.body.as_ref().expect("body arrived"),
        detail,
        model.terminal_width,
        chrono::DateTime::UNIX_EPOCH,
    )
    .lines
    .len() as u16;
    let viewport = model.terminal_height - crate::view::detail::CHROME_ROWS;
    let max_scroll = content_lines - viewport;

    for _ in 0..50 {
        update(&mut model, key_event(KeyCode::PageDown));
    }

    assert_eq!(
        detail_scroll(&model),
        max_scroll,
        "PageDown must clamp at the bottom of the FULL content (threads + conversation included)"
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
    // Reference the canonical chrome-row count so a future layout change keeps
    // this regression test in sync with the clamp it guards (rather than a
    // hardcoded literal that would silently desync).
    let chrome_rows = crate::view::detail::CHROME_ROWS;
    let mut model = model_with_one_pr();
    model.terminal_height = chrome_rows + 6; // body viewport = 6 rows
    update(&mut model, key_event(KeyCode::Enter));
    let body: String = (1..=20).map(|n| format!("Line {n}\n\n")).collect();
    set_detail_body(&mut model, &body);

    // The true max scroll: the full body content (description plus the
    // threads/comments loading placeholders here) minus the viewport, measured
    // via the same layout the view renders.
    let ViewMode::Detail(detail) = &model.view_mode else {
        panic!("expected Detail mode");
    };
    let pr = model.list.pr(&detail.key).expect("pr in list");
    let content_lines = crate::view::detail::detail_content(
        &model,
        pr,
        detail.body.as_ref().expect("body arrived"),
        detail,
        model.terminal_width,
        chrono::DateTime::UNIX_EPOCH,
    )
    .lines
    .len() as u16;
    let viewport_rows = model.terminal_height - chrome_rows;
    let max_scroll = content_lines - viewport_rows;
    assert!(max_scroll > 1, "test body must be taller than the viewport");

    // Hold PageDown far past the end.
    for _ in 0..(content_lines + 50) {
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
