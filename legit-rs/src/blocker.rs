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

/// How a single review thread is waiting. Mirrors the TS `classifyThread`
/// return union. Consumed by `classify_thread`, which the detail view uses to
/// badge individual threads.
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

/// Conclusions that count as hard CI failures.
const HARD_FAILING_CONCLUSIONS: [&str; 3] = ["failure", "timed_out", "cancelled"];

fn has_hard_ci_failure(checks: &[CheckRun]) -> bool {
    checks.iter().any(|c| {
        c.status == "completed"
            && c.conclusion
                .as_deref()
                .is_some_and(|conclusion| HARD_FAILING_CONCLUSIONS.contains(&conclusion))
    })
}

fn has_action_required_check(checks: &[CheckRun]) -> bool {
    checks
        .iter()
        .any(|c| c.status == "completed" && c.conclusion.as_deref() == Some("action_required"))
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

/// Classify a single thread. Mirrors the TS `classifyThread`. Standalone API
/// (the aggregate path uses `classify_threads`); the detail view badges
/// per-thread state with it.
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
///  2. Check action needed -> waiting-on-author (integration requires action)
///  3. Draft               -> waiting-on-author (not ready for review)
///  4. Merge conflict      -> waiting-on-author (author must rebase)
///  5. Changes requested   -> waiting-on-author (via `review_decision` or a
///     review; author must respond before pending reviewers need to act)
///  6. Author reply needed -> waiting-on-author (only when thread data loaded)
///  7. Approved            -> waiting-on-author (author should merge)
///  8. All reviewer-waiting threads -> needs-review / me-blocking for that
///     reviewer
///  9. Current user requested reviewer -> me-blocking
/// 10. Default             -> needs-review (with first other reviewer, if any)
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

    // 1. Hard CI failure -> waiting-on-author, regardless of reviewers.
    if has_hard_ci_failure(checks) {
        return waiting(effective_author, "CI is failing");
    }

    // 2. A completed check is asking for an explicit follow-up action.
    if has_action_required_check(checks) {
        return waiting(effective_author, "Check action required");
    }

    // 3. Draft -> waiting-on-author (author isn't ready for review).
    if pr.is_draft {
        return waiting(effective_author, "Draft - not ready for review");
    }

    // 4. Merge conflict -> waiting-on-author (author must rebase).
    if pr.mergeable == "CONFLICTING" {
        return waiting(effective_author, "Resolve merge conflict");
    }

    // 5. Changes requested — via `review_decision` OR an individual review.
    //    Checked before me-blocking so an existing change-request from another
    //    reviewer takes precedence over our pending review.
    let changes_requested = pr.review_decision == "CHANGES_REQUESTED"
        || reviews.iter().any(|r| r.state == "CHANGES_REQUESTED");
    if changes_requested {
        return waiting(effective_author, "Respond to requested changes");
    }

    // 6. Author-reply-needed review threads (when full thread data is
    //    available). Only threads where the author hasn't replied count
    //    against the author.
    let classification = opts.threads.map(classify_threads);
    if let Some(c) = &classification
        && c.unreplied > 0
    {
        let n = c.unreplied;
        return waiting(
            effective_author,
            &author_reply_needed_reason(n, effective_author, current_user),
        );
    }

    // 7. Approved — GitHub's aggregate decision or the loaded reviews show the
    //    green light. The author's turn to merge.
    if aggregate_review_state(pr, reviews) == Some("APPROVED") {
        return waiting(
            effective_author,
            &ready_to_merge_reason(effective_author, current_user),
        );
    }

    // 8. All unresolved threads are waiting on a reviewer (author replied to every
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
            reason: waiting_on_reviewer_reason(n, reviewer, current_user),
        };
    }

    // 9. Current user is a requested reviewer -> me-blocking.
    if !current_user.is_empty() && pr.requested_reviewers.iter().any(|r| r == current_user) {
        return BlockerResult {
            blocker: current_user.to_owned(),
            tier: Tier::MeBlocking,
            reason: "Review requested from you".to_owned(),
        };
    }

    // 10. Default — needs review (whether a specific reviewer is requested or
    //    not).
    let other_reviewer = pr.requested_reviewers.iter().find(|r| *r != current_user);
    BlockerResult {
        blocker: other_reviewer.cloned().unwrap_or_default(),
        tier: Tier::NeedsReview,
        reason: if let Some(reviewer) = other_reviewer {
            format!("Review requested from {reviewer}")
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

fn thread_noun(n: usize) -> &'static str {
    if n == 1 { "thread" } else { "threads" }
}

fn need_verb(n: usize) -> &'static str {
    if n == 1 { "needs" } else { "need" }
}

fn author_reply_needed_reason(n: usize, effective_author: &str, current_user: &str) -> String {
    let owner = if !current_user.is_empty() && effective_author == current_user {
        "your"
    } else {
        "author"
    };
    format!("{n} {} {} {owner} reply", thread_noun(n), need_verb(n))
}

fn waiting_on_reviewer_reason(n: usize, reviewer: &str, current_user: &str) -> String {
    let who = if !current_user.is_empty() && reviewer == current_user {
        "you"
    } else {
        reviewer
    };
    format!("{n} {} waiting on {who}", thread_noun(n))
}

fn ready_to_merge_reason(effective_author: &str, current_user: &str) -> String {
    if !current_user.is_empty() && effective_author == current_user {
        "Ready for you to merge".to_owned()
    } else {
        "Ready to merge".to_owned()
    }
}

/// Compact list-cell form of the full Next Action stored in `BlockerResult`.
pub fn compact_next_action(result: &BlockerResult) -> String {
    let reason = result.reason.as_str();
    match reason {
        "CI is failing" => "CI failing".to_owned(),
        "Check action required" => "check action required".to_owned(),
        "Draft - not ready for review" => "draft".to_owned(),
        "Resolve merge conflict" => "conflict".to_owned(),
        "Respond to requested changes" => "changes requested".to_owned(),
        "Ready for you to merge" | "Ready to merge" => "ready to merge".to_owned(),
        "Awaiting review" => "awaiting review".to_owned(),
        "Review requested from you" => "review from you".to_owned(),
        _ => compact_patterned_next_action(reason).unwrap_or_else(|| reason.to_owned()),
    }
}

fn compact_patterned_next_action(reason: &str) -> Option<String> {
    if let Some(reviewer) = reason.strip_prefix("Review requested from ") {
        return Some(format!("review from {reviewer}"));
    }
    if let Some(n) = reason.strip_suffix(" thread needs your reply") {
        return Some(format!("{n} needs your reply"));
    }
    if let Some(n) = reason.strip_suffix(" threads need your reply") {
        return Some(format!("{n} need your reply"));
    }
    if let Some(n) = reason.strip_suffix(" thread needs author reply") {
        return Some(format!("{n} needs reply"));
    }
    if let Some(n) = reason.strip_suffix(" threads need author reply") {
        return Some(format!("{n} need reply"));
    }
    if let Some(rest) = reason.strip_suffix(" thread waiting on you") {
        return Some(format!("{rest} waiting on you"));
    }
    if let Some(rest) = reason.strip_suffix(" threads waiting on you") {
        return Some(format!("{rest} waiting on you"));
    }
    if let Some((n, reviewer)) = reason.split_once(" thread waiting on ") {
        return Some(format!("{n} waiting on {reviewer}"));
    }
    if let Some((n, reviewer)) = reason.split_once(" threads waiting on ") {
        return Some(format!("{n} waiting on {reviewer}"));
    }
    None
}

#[cfg(test)]
mod tests;
