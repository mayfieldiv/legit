use chrono::{TimeZone, Utc};

use crate::{
    app::detail_items::{DetailFilters, DetailItems, FocusableItem},
    github::types::{FullReviewThread, IssueComment, ReviewComment},
};

/// The flattened focus sequence for arrived `threads` + `comments` — the shape
/// every consumer reads, so the tests assert through it.
fn focusable_items<'a>(
    threads: &'a [FullReviewThread],
    comments: &'a [IssueComment],
    filters: DetailFilters,
) -> Vec<FocusableItem<'a>> {
    DetailItems::derive(Some(threads), Some(comments), filters).focusable()
}

/// Defaults matching a fresh `Model`: resolved hidden, bots shown.
const DEFAULTS: DetailFilters = DetailFilters {
    show_resolved: false,
    show_bot_comments: true,
};

fn review_comment(id: &str, author: &str, is_bot: bool) -> ReviewComment {
    ReviewComment {
        id: id.to_owned(),
        author: author.to_owned(),
        body: format!("body of {id}"),
        created_at: Utc.with_ymd_and_hms(2026, 5, 20, 12, 0, 0).unwrap(),
        url: format!("https://example.test/r/{id}"),
        is_bot,
    }
}

fn thread(id: &str, is_resolved: bool, comments: Vec<ReviewComment>) -> FullReviewThread {
    FullReviewThread {
        id: id.to_owned(),
        is_resolved,
        path: "src/lib.rs".to_owned(),
        line: Some(1),
        comments,
    }
}

fn issue_comment(id: u64, author: &str, is_bot: bool) -> IssueComment {
    IssueComment {
        id,
        author: author.to_owned(),
        body: format!("comment {id}"),
        created_at: Utc.with_ymd_and_hms(2026, 5, 20, 12, 0, 0).unwrap(),
        url: format!("https://example.test/c/{id}"),
        is_bot,
    }
}

/// A compact signature of the sequence for order assertions: B = body,
/// T:<root comment id> = thread root, R:<id> = reply, C:<id> = issue comment.
fn signature(items: &[FocusableItem<'_>]) -> Vec<String> {
    items
        .iter()
        .map(|item| match item {
            FocusableItem::Body => "B".to_owned(),
            FocusableItem::Thread { root } => format!("T:{}", root.id),
            FocusableItem::Reply { comment } => format!("R:{}", comment.id),
            FocusableItem::Comment { comment } => format!("C:{}", comment.id),
        })
        .collect()
}

#[test]
fn sequence_is_body_then_thread_roots_with_replies_then_comments() {
    let threads = vec![
        thread(
            "t1",
            false,
            vec![
                review_comment("c1", "alice", false),
                review_comment("c2", "bob", false),
            ],
        ),
        thread("t2", false, vec![review_comment("c3", "carol", false)]),
    ];
    let comments = vec![issue_comment(10, "dave", false)];

    let items = focusable_items(&threads, &comments, DEFAULTS);

    assert_eq!(signature(&items), ["B", "T:c1", "R:c2", "T:c3", "C:10"]);
}

#[test]
fn resolved_threads_hidden_by_default_and_shown_when_toggled() {
    let threads = vec![
        thread("open", false, vec![review_comment("c1", "alice", false)]),
        thread("done", true, vec![review_comment("c2", "bob", false)]),
    ];

    let hidden = focusable_items(&threads, &[], DEFAULTS);
    assert_eq!(signature(&hidden), ["B", "T:c1"]);

    let shown = focusable_items(
        &threads,
        &[],
        DetailFilters {
            show_resolved: true,
            ..DEFAULTS
        },
    );
    assert_eq!(signature(&shown), ["B", "T:c1", "T:c2"]);
}

#[test]
fn hiding_bots_drops_bot_replies_and_bot_only_threads() {
    let threads = vec![
        // Mixed thread: bot root, human reply — with bots hidden the human
        // comment becomes the root.
        thread(
            "mixed",
            false,
            vec![
                review_comment("bot1", "linter", true),
                review_comment("c1", "alice", false),
            ],
        ),
        // Bot-only thread: hidden entirely when bots are hidden.
        thread(
            "botonly",
            false,
            vec![review_comment("bot2", "linter", true)],
        ),
    ];
    let comments = vec![
        issue_comment(10, "dave", false),
        issue_comment(11, "ci", true),
    ];

    let bots_shown = focusable_items(&threads, &comments, DEFAULTS);
    assert_eq!(
        signature(&bots_shown),
        ["B", "T:bot1", "R:c1", "T:bot2", "C:10", "C:11"]
    );

    let bots_hidden = focusable_items(
        &threads,
        &comments,
        DetailFilters {
            show_bot_comments: false,
            ..DEFAULTS
        },
    );
    // The mixed thread's root is now the first *visible* comment (c1, not the
    // filtered bot root), and the bot-only thread disappears.
    assert_eq!(signature(&bots_hidden), ["B", "T:c1", "C:10"]);
}

#[test]
fn empty_threads_are_never_focusable() {
    let threads = vec![thread("empty", false, Vec::new())];

    let items = focusable_items(&threads, &[], DEFAULTS);

    assert_eq!(signature(&items), ["B"]);
    let derived = DetailItems::derive(Some(&threads), None, DEFAULTS);
    assert!(derived.threads.expect("threads arrived").groups.is_empty());
}

#[test]
fn urls_deep_link_to_the_specific_comment_and_body_has_none() {
    let threads = vec![thread(
        "t1",
        false,
        vec![
            review_comment("c1", "alice", false),
            review_comment("c2", "bob", false),
        ],
    )];
    let comments = vec![issue_comment(10, "dave", false)];

    let items = focusable_items(&threads, &comments, DEFAULTS);

    let urls: Vec<Option<&str>> = items.iter().map(FocusableItem::url).collect();
    assert_eq!(
        urls,
        [
            None,
            Some("https://example.test/r/c1"),
            Some("https://example.test/r/c2"),
            Some("https://example.test/c/10"),
        ]
    );
}
