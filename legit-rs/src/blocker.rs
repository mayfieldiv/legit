//! Blocker / Smart-status engine.
//!
//! Pure port of the TS `src/lib/blocker-engine.ts`: given a PR, the current
//! user, and any loaded enrichment (checks, reviews, threads), decide who is
//! blocking the PR and which Smart-status tier it belongs to. No IO, no async —
//! every input is passed explicitly so the engine is unit-tested synchronously.
//!
//! Tier priority order (highest -> lowest urgency to the current user):
//!   `MeBlocking` -> `NeedsReview` -> `WaitingOnAuthor`

use chrono::{DateTime, Utc};

use crate::github::{
    rest::PR,
    types::{CheckRun, FullReviewThread, Review},
};

// ── Public types ────────────────────────────────────────────────────────────

/// Smart-status: the categorisation that drives sort order and grouping.
/// Mirrors the TS `Tier`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    /// Current user must act.
    MeBlocking,
    /// Needs a reviewer's attention (no specific person required).
    NeedsReview,
    /// Author must act (hidden by default unless you're the author).
    WaitingOnAuthor,
}

impl Tier {
    /// Human-readable label, suitable for group headings.
    pub fn label(self) -> &'static str {
        match self {
            Tier::MeBlocking => "Me blocking",
            Tier::NeedsReview => "Needs review",
            Tier::WaitingOnAuthor => "Waiting on author",
        }
    }

    /// Display priority (lower = more urgent for the user). Used to sort
    /// grouped PR lists; mirrors the TS `TIER_ORDER`.
    pub fn order(self) -> u8 {
        match self {
            Tier::MeBlocking => 0,
            Tier::NeedsReview => 1,
            Tier::WaitingOnAuthor => 2,
        }
    }
}

/// The outcome of running the engine on one PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockerResult {
    /// Login of the person blocking the PR, or empty for `NeedsReview` with no
    /// specific reviewer.
    pub blocker: String,
    pub tier: Tier,
    pub reason: String,
}

/// Loaded enrichment passed to the engine. Each field is optional: the engine
/// degrades gracefully to the list-only rules when enrichment hasn't arrived.
#[derive(Debug, Clone, Default)]
pub struct BlockerOptions<'a> {
    /// Completed/in-progress check runs for this PR's head commit.
    pub checks: &'a [CheckRun],
    /// Individual reviewer states fetched from the Reviews API.
    pub reviews: &'a [Review],
    /// Full review threads. When non-empty, enables the unreplied vs
    /// awaiting-reviewer distinction. `None` means "not loaded yet" — distinct
    /// from `Some(&[])` ("loaded, no threads"), exactly as the TS `opts.threads`
    /// `undefined`-vs-`[]` distinction drives rule 5/7.
    pub threads: Option<&'a [FullReviewThread]>,
}

/// How a single unresolved review thread is waiting. Mirrors the TS
/// `classifyThread` return union. Consumed by `classify_thread`, which the
/// detail view will use to badge individual threads in a later milestone.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadKind {
    /// Closed; not blocking anyone.
    Resolved,
    /// Last non-bot comment is the thread starter's (or only bots replied):
    /// the author must respond.
    Unreplied,
    /// Someone other than the thread starter spoke last: the reviewer (thread
    /// starter) must resolve or reply.
    AwaitingReviewer,
}

/// Aggregate classification of a PR's unresolved threads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadClassification {
    /// Count of unresolved threads where the author must respond.
    pub unreplied: usize,
    /// Count of unresolved threads where a reviewer must act.
    pub awaiting_reviewer: usize,
    /// Per-reviewer breakdown of awaiting-reviewer threads, in first-seen order
    /// (mirrors JS `Map` insertion order, which the tie-break relies on).
    pub awaiting_by_reviewer: Vec<AwaitingReviewer>,
}

/// One reviewer's awaiting-reviewer tally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwaitingReviewer {
    pub reviewer: String,
    pub count: usize,
    /// Oldest reply date across this reviewer's awaiting threads, used to break
    /// count ties (longest-waiting reviewer wins).
    pub oldest_reply_date: DateTime<Utc>,
}

// ── CI helpers ──────────────────────────────────────────────────────────────

/// Conclusions that count as a failing check. Mirrors the TS
/// `FAILING_CONCLUSIONS` set.
const FAILING_CONCLUSIONS: [&str; 3] = ["failure", "timed_out", "cancelled"];

fn is_ci_failing(checks: &[CheckRun]) -> bool {
    checks.iter().any(|c| {
        c.status == "completed"
            && c.conclusion
                .as_deref()
                .is_some_and(|conclusion| FAILING_CONCLUSIONS.contains(&conclusion))
    })
}

// ── Review-state aggregation ──────────────────────────────────────────────────

/// Best-effort aggregate review state when GitHub's top-level `review_decision`
/// is empty or lagging behind the individual reviews we have already loaded.
/// Mirrors the TS `aggregateReviewState` in `src/lib/review-state.ts`. Returns
/// the borrowed `&str` so callers compare without allocating.
fn aggregate_review_state<'a>(pr: &'a PR, reviews: &'a [Review]) -> Option<&'a str> {
    if pr.review_decision == "CHANGES_REQUESTED" || pr.review_decision == "APPROVED" {
        return Some(&pr.review_decision);
    }
    if reviews.is_empty() {
        return None;
    }
    if reviews.iter().any(|r| r.state == "CHANGES_REQUESTED") {
        return Some("CHANGES_REQUESTED");
    }
    if reviews.iter().any(|r| r.state == "APPROVED") {
        return Some("APPROVED");
    }
    None
}

// ── Thread classification ─────────────────────────────────────────────────────

/// Last non-bot comment in a thread, scanning from the end. Mirrors the TS
/// `findLastNonBotComment`.
fn last_non_bot(thread: &FullReviewThread) -> Option<&crate::github::types::ReviewComment> {
    thread.comments.iter().rev().find(|c| !c.is_bot)
}

/// Classify a single unresolved thread. Mirrors the TS `classifyThread`.
/// Standalone API (the aggregate path uses `classify_threads`); the detail view
/// will badge per-thread state with it in a later milestone.
#[allow(dead_code)]
pub fn classify_thread(thread: &FullReviewThread) -> ThreadKind {
    if thread.is_resolved {
        return ThreadKind::Resolved;
    }
    let Some(starter) = thread.comments.first() else {
        // Empty thread: no comments to classify against. The TS treats this as
        // `unreplied`, though `classify_threads` skips empty threads entirely.
        return ThreadKind::Unreplied;
    };
    match last_non_bot(thread) {
        // Thread starter spoke last, or only bots replied -> author must respond.
        Some(last) if last.author != starter.author => ThreadKind::AwaitingReviewer,
        _ => ThreadKind::Unreplied,
    }
}

/// Classify all threads into the unreplied / awaiting-reviewer tallies. Mirrors
/// the TS `classifyThreads`. Resolved and empty threads are skipped.
pub fn classify_threads(threads: &[FullReviewThread]) -> ThreadClassification {
    let mut result = ThreadClassification::default();

    for thread in threads {
        if thread.is_resolved {
            continue;
        }
        let Some(starter) = thread.comments.first() else {
            continue; // empty thread is skipped (matches TS `comments.length === 0`)
        };
        let starter = &starter.author;
        let last_non_bot = last_non_bot(thread);

        match last_non_bot {
            // Someone other than the thread starter replied last -> reviewer
            // (thread starter) must act.
            Some(last) if &last.author != starter => {
                result.awaiting_reviewer += 1;
                match result
                    .awaiting_by_reviewer
                    .iter_mut()
                    .find(|entry| &entry.reviewer == starter)
                {
                    Some(entry) => {
                        entry.count += 1;
                        if last.created_at < entry.oldest_reply_date {
                            entry.oldest_reply_date = last.created_at;
                        }
                    }
                    None => result.awaiting_by_reviewer.push(AwaitingReviewer {
                        reviewer: starter.clone(),
                        count: 1,
                        oldest_reply_date: last.created_at,
                    }),
                }
            }
            // Thread starter spoke last, or only bots replied -> author must
            // respond.
            _ => result.unreplied += 1,
        }
    }

    result
}

/// Pick the reviewer with the most awaiting-reviewer threads. Ties are broken
/// by oldest reply date (longest-waiting reviewer wins). Mirrors the TS
/// `pickTopAwaitingReviewer` exactly, including its reliance on first-seen
/// iteration order: the very first reviewer always beats the `("", 0)` seed
/// because its count exceeds 0, so an empty input yields `""`.
fn pick_top_awaiting_reviewer(awaiting: &[AwaitingReviewer]) -> &str {
    let mut top: Option<&AwaitingReviewer> = None;
    for entry in awaiting {
        let wins = match top {
            None => true,
            Some(current) => {
                entry.count > current.count
                    || (entry.count == current.count
                        && entry.oldest_reply_date < current.oldest_reply_date)
            }
        };
        if wins {
            top = Some(entry);
        }
    }
    top.map(|entry| entry.reviewer.as_str()).unwrap_or("")
}

// ── Core algorithm ────────────────────────────────────────────────────────────

/// Compute who is blocking `pr` and why.
///
/// Decision order (first matching rule wins):
///  1. CI failing          -> waiting-on-author (fix CI before reviewing)
///  2. Draft               -> waiting-on-author (not ready for review)
///  3. Merge conflict      -> waiting-on-author (author must rebase)
///  4. Changes requested   -> waiting-on-author (via `review_decision` or a
///     review; author must respond before pending reviewers need to act)
///  5. Unreplied threads   -> waiting-on-author (only when thread data loaded)
///  6. Approved            -> waiting-on-author (author should merge)
///  7. All awaiting-reviewer -> needs-review / me-blocking for that reviewer
///  8. Current user requested reviewer -> me-blocking
///  9. Default             -> needs-review (with first other reviewer, if any)
///
/// Effective author: when the current user is an assignee but the PR author is
/// not, the current user is treated as the "effective author" throughout — all
/// waiting-on-author results point to them (modelling a PR takeover).
///
/// Post-processing: a waiting-on-author result whose blocker is the current
/// user is elevated to me-blocking, so the PR surfaces at the top of the list.
pub fn compute_blocker(pr: &PR, current_user: &str, opts: &BlockerOptions<'_>) -> BlockerResult {
    let result = compute_blocker_core(pr, current_user, opts);
    // Elevate to me-blocking when the current user is the one who must act —
    // whether as author (e.g. CI failing on their own PR) or as a reviewer.
    if result.tier == Tier::WaitingOnAuthor && result.blocker == current_user {
        return BlockerResult {
            tier: Tier::MeBlocking,
            ..result
        };
    }
    result
}

fn compute_blocker_core(pr: &PR, current_user: &str, opts: &BlockerOptions<'_>) -> BlockerResult {
    let checks = opts.checks;
    let reviews = opts.reviews;

    // Effective author: when the current user is an assignee but the original
    // author is not, the current user has taken over responsibility for the PR.
    // All "waiting-on-author" rules will point to the effective author instead.
    let effective_author: &str = if pr.assignees.iter().any(|a| a == current_user)
        && !pr.assignees.iter().any(|a| a == &pr.author)
    {
        current_user
    } else {
        &pr.author
    };

    // 1. CI failing -> waiting-on-author, regardless of reviewers.
    if is_ci_failing(checks) {
        return waiting(effective_author, "CI is failing");
    }

    // 2. Draft -> waiting-on-author (author isn't ready for review).
    if pr.is_draft {
        return waiting(effective_author, "Draft — not ready for review");
    }

    // 3. Merge conflict -> waiting-on-author (author must rebase).
    if pr.mergeable == "CONFLICTING" {
        return waiting(effective_author, "Merge conflict");
    }

    // 4. Changes requested — via `review_decision` OR an individual review.
    //    Checked before me-blocking so an existing change-request from another
    //    reviewer takes precedence over our pending review.
    let changes_requested = pr.review_decision == "CHANGES_REQUESTED"
        || reviews.iter().any(|r| r.state == "CHANGES_REQUESTED");
    if changes_requested {
        return waiting(effective_author, "Changes requested");
    }

    // 5. Unreplied review threads (when full thread data is available). Only
    //    threads where the author hasn't replied count against the author.
    let classification = opts.threads.map(classify_threads);
    if let Some(c) = &classification
        && c.unreplied > 0
    {
        let n = c.unreplied;
        return waiting(
            effective_author,
            &format!("{n} unreplied thread{}", plural(n)),
        );
    }

    // 6. Approved — GitHub's aggregate decision or the loaded reviews show the
    //    green light. The author's turn to merge.
    if aggregate_review_state(pr, reviews) == Some("APPROVED") {
        return waiting(effective_author, "Approved — ready to merge");
    }

    // 7. All unresolved threads are awaiting-reviewer (author replied to every
    //    one). Identify the reviewer who needs to act.
    if let Some(c) = &classification
        && c.awaiting_reviewer > 0
    {
        let reviewer = pick_top_awaiting_reviewer(&c.awaiting_by_reviewer);
        let n = c.awaiting_reviewer;
        let tier = if reviewer == current_user {
            Tier::MeBlocking
        } else {
            Tier::NeedsReview
        };
        return BlockerResult {
            blocker: reviewer.to_owned(),
            tier,
            reason: format!("{n} thread{} awaiting {reviewer}", plural(n)),
        };
    }

    // 8. Current user is a requested reviewer -> me-blocking.
    if pr.requested_reviewers.iter().any(|r| r == current_user) {
        return BlockerResult {
            blocker: current_user.to_owned(),
            tier: Tier::MeBlocking,
            reason: "You are a requested reviewer".to_owned(),
        };
    }

    // 9. Default — needs review (whether a specific reviewer is requested or
    //    not).
    let other_reviewer = pr.requested_reviewers.iter().find(|r| *r != current_user);
    BlockerResult {
        blocker: other_reviewer.cloned().unwrap_or_default(),
        tier: Tier::NeedsReview,
        reason: if other_reviewer.is_some() {
            "Awaiting reviewer".to_owned()
        } else {
            "Awaiting review".to_owned()
        },
    }
}

/// Build a `WaitingOnAuthor` result. The shared shape of rules 1-6.
fn waiting(blocker: &str, reason: &str) -> BlockerResult {
    BlockerResult {
        blocker: blocker.to_owned(),
        tier: Tier::WaitingOnAuthor,
        reason: reason.to_owned(),
    }
}

/// Pluralise English nouns: `""` for one, `"s"` for any other count.
fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests;
