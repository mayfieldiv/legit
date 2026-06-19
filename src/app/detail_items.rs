//! The detail view's derived content model: which threads, replies, and issue
//! comments are visible under the current filters, grouped into the sections
//! the body renders — and, flattened, the focus sequence `j`/`k` cycles
//! through. Mirrors the TS `DetailView` `focusableItems` memo: body first,
//! then each visible thread's root followed by its replies, then each visible
//! issue comment.
//!
//! Pure derivation — `update` (focus movement, `o` URL resolution) consumes
//! the flattened sequence and `detail_layout` (card rendering, focused border)
//! walks the sections in the same order, so what is focusable and what is
//! rendered can never drift apart.

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

/// One visible thread under the current filters: the thread plus its root
/// (first visible comment, carrying the card's byline, body, and URL) and the
/// remaining visible comments as replies.
#[derive(Debug)]
pub struct ThreadGroup<'a> {
    pub thread: &'a FullReviewThread,
    pub root: &'a ReviewComment,
    pub replies: Vec<&'a ReviewComment>,
}

/// The Review Threads section: every visible thread as a group, in arrival
/// order, plus the arrived total so the header can count hidden threads.
#[derive(Debug)]
pub struct ThreadsSection<'a> {
    /// How many threads arrived, hidden ones included.
    pub total: usize,
    /// The threads that render under the current filters: resolved threads
    /// drop unless `show_resolved`, and a thread whose every comment is
    /// filtered out (bot-only with bots hidden, or empty) drops entirely.
    pub groups: Vec<ThreadGroup<'a>>,
}

/// The Conversation section: the issue comments that render under the current
/// filters, in arrival order, plus the arrived total.
#[derive(Debug)]
pub struct CommentsSection<'a> {
    /// How many comments arrived, hidden ones included.
    pub total: usize,
    pub visible: Vec<&'a IssueComment>,
}

/// The detail body's content model under the current filters. `None` sections
/// haven't arrived yet (the body renders a loading placeholder); arrived-empty
/// sections render nothing.
#[derive(Debug)]
pub struct DetailItems<'a> {
    pub threads: Option<ThreadsSection<'a>>,
    pub comments: Option<CommentsSection<'a>>,
}

/// One entry in the detail view's focus sequence — `DetailItems::focusable`
/// flattened in section order. Borrows from the enrichment maps; rebuilt on
/// demand wherever it's consumed.
#[derive(Debug)]
pub enum FocusableItem<'a> {
    /// The PR description (always present, always first).
    Body,
    /// A visible thread's card, identified by its root comment (which carries
    /// the deep link `o` opens). The thread itself stays on `ThreadGroup` —
    /// only the layout needs it.
    Thread { root: &'a ReviewComment },
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
            FocusableItem::Thread { root } => Some(&root.url),
            FocusableItem::Reply { comment } => Some(&comment.url),
            FocusableItem::Comment { comment } => Some(&comment.url),
        }
    }
}

/// The detail view's focused item, identity-keyed. `Body` is the description
/// (always position 0); `Item` names a thread root / reply / issue comment by
/// its comment URL — the same stable key card expansion uses — plus the
/// position it resolved to in the Focus Sequence on the last `resolve_focus`.
/// Keying by identity instead of raw position means arriving threads/comments
/// or filter toggles that insert items above the focus move its *index*, never
/// *which card* is focused — so `o`/Enter can't act on a card the user didn't
/// select.
#[derive(Clone, Debug, PartialEq)]
pub enum DetailFocus {
    Body,
    Item { url: String, index: usize },
}

impl DetailFocus {
    /// The focused position in the Focus Sequence as of the last resolution
    /// (0 for the body). Read by the layout's focused-border check and as the
    /// fallback when the focused item vanishes.
    pub fn index(&self) -> usize {
        match self {
            DetailFocus::Body => 0,
            DetailFocus::Item { index, .. } => *index,
        }
    }

    /// The focused item's comment URL — `None` for the body (`o` falls back
    /// to the PR URL).
    pub fn url(&self) -> Option<&str> {
        match self {
            DetailFocus::Body => None,
            DetailFocus::Item { url, .. } => Some(url),
        }
    }
}

impl<'a> DetailItems<'a> {
    /// Derive the sections from the enrichment maps' entries (`None` = that
    /// fetch hasn't arrived) under `filters`. The single filtering pass both
    /// `update` and `detail_layout` build from.
    pub fn derive(
        threads: Option<&'a [FullReviewThread]>,
        comments: Option<&'a [IssueComment]>,
        filters: DetailFilters,
    ) -> Self {
        let visible_comment =
            move |comment: &&'a ReviewComment| filters.show_bot_comments || !comment.is_bot;
        let threads = threads.map(|threads| ThreadsSection {
            total: threads.len(),
            groups: threads
                .iter()
                .filter(|thread| filters.show_resolved || !thread.is_resolved)
                .filter_map(|thread| {
                    let mut comments = thread.comments.iter().filter(visible_comment);
                    // A thread with no visible comments has no root: drop it.
                    let root = comments.next()?;
                    Some(ThreadGroup {
                        thread,
                        root,
                        replies: comments.collect(),
                    })
                })
                .collect(),
        });
        let comments = comments.map(|comments| CommentsSection {
            total: comments.len(),
            visible: comments
                .iter()
                .filter(|comment| filters.show_bot_comments || !comment.is_bot)
                .collect(),
        });
        Self { threads, comments }
    }

    /// The flat focus sequence: body → each group's root → that group's
    /// replies → each visible issue comment. `detail_layout` pushes cards by
    /// walking the same sections in the same order, so a card's position in
    /// the layout and its index here agree by construction.
    pub fn focusable(&self) -> Vec<FocusableItem<'a>> {
        let mut items = vec![FocusableItem::Body];
        for group in self.threads.iter().flat_map(|section| &section.groups) {
            items.push(FocusableItem::Thread { root: group.root });
            items.extend(
                group
                    .replies
                    .iter()
                    .map(|&comment| FocusableItem::Reply { comment }),
            );
        }
        items.extend(
            self.comments
                .iter()
                .flat_map(|section| &section.visible)
                .map(|&comment| FocusableItem::Comment { comment }),
        );
        items
    }

    /// The identity-keyed focus for the item at `index` in this derivation's
    /// Focus Sequence, clamped into range. How a positional intent (a `j`/`k`
    /// step, a vanished-item fallback) is anchored to an identity to store.
    pub fn focus_at(&self, index: usize) -> DetailFocus {
        let focusable = self.focusable();
        let index = index.min(focusable.len() - 1);
        match focusable[index].url() {
            Some(url) => DetailFocus::Item {
                url: url.to_owned(),
                index,
            },
            None => DetailFocus::Body,
        }
    }

    /// Re-anchor a stored focus against this derivation: the URL wins while
    /// its item is still in the sequence (re-indexed to wherever it sits now);
    /// a vanished item falls back to its last position, clamped to the shrunk
    /// sequence and adopting whatever card sits there. Every update runs this,
    /// so a stored focus is never stale for longer than one message.
    pub fn resolve_focus(&self, focus: &DetailFocus) -> DetailFocus {
        let DetailFocus::Item { url, index } = focus else {
            return DetailFocus::Body;
        };
        let position = self
            .focusable()
            .iter()
            .position(|item| item.url() == Some(url));
        match position {
            Some(index) => DetailFocus::Item {
                url: url.clone(),
                index,
            },
            None => self.focus_at(*index),
        }
    }

    /// `focusable().len()` without building the sequence — focus clamping
    /// runs on every update, so it stays allocation-free.
    pub fn focusable_len(&self) -> usize {
        let thread_items: usize = self
            .threads
            .iter()
            .flat_map(|section| &section.groups)
            .map(|group| 1 + group.replies.len())
            .sum();
        let comment_items = self
            .comments
            .as_ref()
            .map_or(0, |section| section.visible.len());
        1 + thread_items + comment_items
    }
}

#[cfg(test)]
mod tests;
