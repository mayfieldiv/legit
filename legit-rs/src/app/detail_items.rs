//! The detail view's flat focus sequence: which threads, replies, and issue
//! comments are visible under the current filters, and the order `j`/`k`
//! cycles through them. Mirrors the TS `DetailView` `focusableItems` memo:
//! body first, then each visible thread's root followed by its replies, then
//! each visible issue comment.
//!
//! Pure derivation — both `update` (focus movement, `o` URL resolution) and
//! `view::detail` (card rendering, focused border) call it, so what is
//! focusable and what is rendered can never drift apart.

use crate::github::types::{FullReviewThread, IssueComment, ReviewComment};

/// The detail view's comment-visibility filters, copied from the `Model`
/// fields `t`/`b` toggle.
#[derive(Clone, Copy, Debug)]
pub struct DetailFilters {
    /// Show resolved threads (default false: resolved threads are hidden).
    pub show_resolved: bool,
    /// Show bot comments (default true). When false, bot comments disappear
    /// from threads and the conversation, and a thread left with no visible
    /// comments is hidden entirely.
    pub show_bot_comments: bool,
}

/// One entry in the detail view's focus sequence. Borrows from the enrichment
/// maps; rebuilt on demand wherever it's consumed.
#[derive(Debug)]
pub enum FocusableItem<'a> {
    /// The PR description (always present, always first).
    Body,
    /// A visible thread's card: the thread plus its root (first visible)
    /// comment, which carries the card's byline, body, and URL.
    Thread {
        thread: &'a FullReviewThread,
        root: &'a ReviewComment,
    },
    /// A reply row inside a thread (every visible comment after the root).
    Reply { comment: &'a ReviewComment },
    /// A top-level conversation comment.
    Comment { comment: &'a IssueComment },
}

impl FocusableItem<'_> {
    /// The GitHub URL `o` opens for this item — the deep link to the specific
    /// comment. `None` for the body (the caller falls back to the PR URL).
    pub fn url(&self) -> Option<&str> {
        match self {
            FocusableItem::Body => None,
            FocusableItem::Thread { root, .. } => Some(&root.url),
            FocusableItem::Reply { comment } => Some(&comment.url),
            FocusableItem::Comment { comment } => Some(&comment.url),
        }
    }
}

/// A thread's comments that survive the bot filter, in thread order. The first
/// one is the card's root; the rest are replies.
pub fn visible_thread_comments(
    thread: &FullReviewThread,
    filters: DetailFilters,
) -> Vec<&ReviewComment> {
    thread
        .comments
        .iter()
        .filter(|c| filters.show_bot_comments || !c.is_bot)
        .collect()
}

/// The threads that render under the current filters, in arrival order:
/// resolved threads drop unless `show_resolved`, and a thread whose every
/// comment is filtered out (bot-only with bots hidden, or empty) drops
/// entirely.
pub fn visible_threads(
    threads: &[FullReviewThread],
    filters: DetailFilters,
) -> Vec<&FullReviewThread> {
    threads
        .iter()
        .filter(|t| filters.show_resolved || !t.is_resolved)
        .filter(|t| !visible_thread_comments(t, filters).is_empty())
        .collect()
}

/// The issue comments that render under the current filters, in arrival order.
pub fn visible_comments(comments: &[IssueComment], filters: DetailFilters) -> Vec<&IssueComment> {
    comments
        .iter()
        .filter(|c| filters.show_bot_comments || !c.is_bot)
        .collect()
}

/// The full focus sequence: body → each visible thread's root → that thread's
/// replies → each visible issue comment.
pub fn focusable_items<'a>(
    threads: &'a [FullReviewThread],
    comments: &'a [IssueComment],
    filters: DetailFilters,
) -> Vec<FocusableItem<'a>> {
    let mut items = vec![FocusableItem::Body];
    for thread in visible_threads(threads, filters) {
        let mut thread_comments = visible_thread_comments(thread, filters).into_iter();
        // `visible_threads` already dropped threads with no visible comments,
        // so the root is always present.
        if let Some(root) = thread_comments.next() {
            items.push(FocusableItem::Thread { thread, root });
        }
        items.extend(thread_comments.map(|comment| FocusableItem::Reply { comment }));
    }
    items.extend(
        visible_comments(comments, filters)
            .into_iter()
            .map(|comment| FocusableItem::Comment { comment }),
    );
    items
}

#[cfg(test)]
mod tests;
