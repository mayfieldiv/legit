/**
 * Blocker/Priority Engine
 *
 * Pure function that determines who is blocking a PR and assigns a priority
 * tier. No side effects — all inputs are passed explicitly.
 *
 * Tier priority order (highest → lowest urgency to the current user):
 *   me-blocking → needs-review → waiting-on-author
 */

import type { PR, CheckRun, Review, FullReviewThread, ReviewComment } from "./types";

// ── Public types ─────────────────────────────────────────────────────────────

export type Tier =
	| "me-blocking" // current user must act
	| "needs-review" // needs a reviewer's attention (no specific person)
	| "waiting-on-author"; // author must act (hidden by default unless you're the author)

export interface BlockerResult {
	/** Login of the person blocking the PR, or empty string for needs-review. */
	blocker: string;
	tier: Tier;
	reason: string;
}

export interface BlockerOptions {
	/** Completed/in-progress check runs for this PR. */
	checks?: CheckRun[];
	/** Individual reviewer states fetched from the Reviews API. */
	reviews?: Review[];
	/** Full review threads. When provided, enables unreplied vs awaiting-reviewer distinction. */
	threads?: FullReviewThread[];
}

// ── CI helpers ────────────────────────────────────────────────────────────────

const FAILING_CONCLUSIONS = new Set<string>(["failure", "timed_out", "cancelled"]);

function isCiFailing(checks: CheckRun[]): boolean {
	return checks.some(
		(c) =>
			c.status === "completed" &&
			c.conclusion !== null &&
			FAILING_CONCLUSIONS.has(c.conclusion),
	);
}

// ── Thread classification ─────────────────────────────────────────────────────

export interface ThreadClassification {
	/** Unresolved threads where the thread starter's comment is the last non-bot comment (author must act). */
	unreplied: number;
	/** Unresolved threads where someone other than the thread starter replied last (reviewer must act). */
	awaitingReviewer: number;
	/** Map of reviewer login → count of threads awaiting them. */
	awaitingByReviewer: Map<string, { count: number; oldestReplyDate: string }>;
}

/**
 * Classify unresolved threads into unreplied vs awaiting-reviewer.
 *
 * For each unresolved thread, look at the last non-bot comment:
 * - If it's from the thread starter (the reviewer) → unreplied (author must respond)
 * - If it's from anyone else → awaiting-reviewer (reviewer must resolve/reply)
 */
export function classifyThreads(threads: FullReviewThread[]): ThreadClassification {
	let unreplied = 0;
	let awaitingReviewer = 0;
	const awaitingByReviewer = new Map<string, { count: number; oldestReplyDate: string }>();

	for (const thread of threads) {
		if (thread.isResolved) continue;
		if (thread.comments.length === 0) continue;

		const threadStarter = thread.comments[0]!.author;
		const lastNonBot = findLastNonBotComment(thread.comments);

		if (!lastNonBot || lastNonBot.author === threadStarter) {
			// Thread starter spoke last (or only bots replied) → author must respond
			unreplied++;
		} else {
			// Someone else replied last → reviewer (thread starter) must act
			awaitingReviewer++;
			const existing = awaitingByReviewer.get(threadStarter);
			if (existing) {
				existing.count++;
				if (lastNonBot.createdAt < existing.oldestReplyDate) {
					existing.oldestReplyDate = lastNonBot.createdAt;
				}
			} else {
				awaitingByReviewer.set(threadStarter, {
					count: 1,
					oldestReplyDate: lastNonBot.createdAt,
				});
			}
		}
	}

	return { unreplied, awaitingReviewer, awaitingByReviewer };
}

/**
 * Classify a single unresolved thread as "unreplied" or "awaiting-reviewer".
 * Returns "resolved" for resolved threads.
 */
export function classifyThread(
	thread: FullReviewThread,
): "resolved" | "unreplied" | "awaiting-reviewer" {
	if (thread.isResolved) return "resolved";
	if (thread.comments.length === 0) return "unreplied";

	const threadStarter = thread.comments[0]!.author;
	const lastNonBot = findLastNonBotComment(thread.comments);

	if (!lastNonBot || lastNonBot.author === threadStarter) {
		return "unreplied";
	}
	return "awaiting-reviewer";
}

function findLastNonBotComment(comments: ReviewComment[]): ReviewComment | undefined {
	for (let i = comments.length - 1; i >= 0; i--) {
		if (!comments[i]!.isBot) return comments[i]!;
	}
	return undefined;
}

/**
 * Pick the reviewer with the most awaiting-reviewer threads.
 * Ties broken by oldest reply date (longest-waiting reviewer wins).
 */
function pickTopAwaitingReviewer(
	awaitingByReviewer: Map<string, { count: number; oldestReplyDate: string }>,
): string {
	let topReviewer = "";
	let topCount = 0;
	let topOldest = "";

	for (const [reviewer, data] of awaitingByReviewer) {
		if (
			data.count > topCount ||
			(data.count === topCount && data.oldestReplyDate < topOldest)
		) {
			topReviewer = reviewer;
			topCount = data.count;
			topOldest = data.oldestReplyDate;
		}
	}

	return topReviewer;
}

// ── Core algorithm ────────────────────────────────────────────────────────────

/**
 * Compute who is blocking `pr` and why.
 *
 * Decision order (first matching rule wins):
 *  1. CI failing          → waiting-on-author (fix CI before reviewing)
 *  2. Draft               → waiting-on-author (not ready for review)
 *  3. Merge conflict      → waiting-on-author (author must rebase)
 *  4. Changes requested (via reviewDecision or individual reviews)
 *                         → waiting-on-author (author must respond before
 *                           pending reviewers need to act)
 *  5a. Unreplied threads  → waiting-on-author (author must respond to open
 *                           review comments; only when thread data is loaded)
 *  5b. (legacy) Unresolved threads without thread data → waiting-on-author
 *  6. Approved            → waiting-on-author (author should merge; no more
 *                           reviewer action needed regardless of pending requests)
 *  7. All threads awaiting-reviewer → needs-review/me-blocking for the
 *                           reviewer (author replied to every unresolved thread)
 *  8. Current user is a requested reviewer → me-blocking
 *  9. Another reviewer requested → needs-review
 * 10. Default             → needs-review
 *
 * Effective author: when the current user is an assignee but the PR author
 * is not, the current user is treated as the "effective author" throughout
 * the algorithm. This models the case where someone takes over responsibility
 * for a PR from the original author.
 *
 * Post-processing: if any waiting-on-author result has the current user as the
 * blocker (i.e. it's their own PR that needs attention), the tier is elevated
 * to me-blocking so the PR surfaces at the top of the list.
 */
export function computeBlocker(pr: PR, currentUser: string, opts?: BlockerOptions): BlockerResult {
	const result = _computeBlockerCore(pr, currentUser, opts);
	// Elevate to me-blocking when the current user is the one who must act —
	// whether they're the author (e.g. CI failing on their own PR) or a reviewer.
	if (result.tier === "waiting-on-author" && result.blocker === currentUser) {
		return { ...result, tier: "me-blocking" };
	}
	return result;
}

/** Internal implementation — call computeBlocker for the public API. */
function _computeBlockerCore(pr: PR, currentUser: string, opts?: BlockerOptions): BlockerResult {
	const checks = opts?.checks ?? [];
	const reviews = opts?.reviews ?? [];

	// Effective author: when the current user is an assignee but the original
	// author is not, the current user has taken over responsibility for the PR.
	// All "waiting-on-author" rules will point to the effective author instead.
	const effectiveAuthor =
		pr.assignees.includes(currentUser) && !pr.assignees.includes(pr.author)
			? currentUser
			: pr.author;

	// 1. CI failing → waiting-on-author, regardless of reviewers
	if (isCiFailing(checks)) {
		return {
			blocker: effectiveAuthor,
			tier: "waiting-on-author",
			reason: "CI is failing",
		};
	}

	// 2. Draft → waiting-on-author (author isn't ready for review)
	if (pr.isDraft) {
		return {
			blocker: effectiveAuthor,
			tier: "waiting-on-author",
			reason: "Draft — not ready for review",
		};
	}

	// 3. Merge conflict → waiting-on-author (author must rebase)
	if (pr.mergeable === "CONFLICTING") {
		return {
			blocker: effectiveAuthor,
			tier: "waiting-on-author",
			reason: "Merge conflict",
		};
	}

	// 4. Changes requested — via reviewDecision field OR individual reviews.
	//    Checked before "me-blocking" so that an existing change-request from
	//    another reviewer takes precedence over our pending review: the author
	//    must address the feedback before we need to re-review.
	const changesRequestedByReview = reviews.some((r) => r.state === "CHANGES_REQUESTED");
	const changesRequestedByDecision = pr.reviewDecision === "CHANGES_REQUESTED";
	if (changesRequestedByDecision || changesRequestedByReview) {
		return {
			blocker: effectiveAuthor,
			tier: "waiting-on-author",
			reason: "Changes requested",
		};
	}

	// 5a. Unreplied review threads (when full thread data is available).
	//     Only threads where the author hasn't replied count against the author.
	const threads = opts?.threads;
	let threadClassification: ThreadClassification | undefined;

	if (threads) {
		threadClassification = classifyThreads(threads);
		if (threadClassification.unreplied > 0) {
			const n = threadClassification.unreplied;
			return {
				blocker: effectiveAuthor,
				tier: "waiting-on-author",
				reason: `${n} unreplied thread${n === 1 ? "" : "s"}`,
			};
		}
	} else {
		// 5b. Legacy fallback: when only CommentCounts are available (no full thread data),
		//     treat all unresolved threads as waiting-on-author.
		const unresolvedThreads =
			(pr.comments?.unresolvedHuman ?? 0) + (pr.comments?.unresolvedBot ?? 0);
		if (unresolvedThreads > 0) {
			return {
				blocker: effectiveAuthor,
				tier: "waiting-on-author",
				reason: `${unresolvedThreads} unresolved thread${unresolvedThreads === 1 ? "" : "s"}`,
			};
		}
	}

	// 6. Approved — the PR has the green light; author's turn to merge (or fix
	//    whatever is blocking the merge, e.g. a conflict that appeared after approval).
	if (pr.reviewDecision === "APPROVED") {
		return {
			blocker: effectiveAuthor,
			tier: "waiting-on-author",
			reason: "Approved — ready to merge",
		};
	}

	// 7. All unresolved threads are awaiting-reviewer (author replied to every one).
	//    Identify the reviewer who needs to act.
	if (threadClassification && threadClassification.awaitingReviewer > 0) {
		const reviewer = pickTopAwaitingReviewer(threadClassification.awaitingByReviewer);
		const n = threadClassification.awaitingReviewer;
		const tier = reviewer === currentUser ? "me-blocking" : "needs-review";
		return {
			blocker: reviewer,
			tier,
			reason: `${n} thread${n === 1 ? "" : "s"} awaiting ${reviewer}`,
		};
	}

	// 8. Current user is a requested reviewer → me-blocking
	if (pr.requestedReviewers.includes(currentUser)) {
		return {
			blocker: currentUser,
			tier: "me-blocking",
			reason: "You are a requested reviewer",
		};
	}

	// 9. Default — needs review (whether a specific reviewer is requested or not)
	const otherReviewers = pr.requestedReviewers.filter((r) => r !== currentUser);
	return {
		blocker: otherReviewers[0] ?? "",
		tier: "needs-review",
		reason: otherReviewers.length > 0 ? "Awaiting reviewer" : "Awaiting review",
	};
}

// ── Tier sort order ───────────────────────────────────────────────────────────

const TIER_ORDER: Record<Tier, number> = {
	"me-blocking": 0,
	"needs-review": 1,
	"waiting-on-author": 2,
};

/**
 * Compare two tiers by display priority (lower = more urgent for the user).
 * Useful for sorting grouped PR lists.
 */
export function compareTiers(a: Tier, b: Tier): number {
	return TIER_ORDER[a] - TIER_ORDER[b];
}

/**
 * Human-readable label for a tier, suitable for group headings.
 */
export function tierLabel(tier: Tier): string {
	switch (tier) {
		case "me-blocking":
			return "Me blocking";
		case "needs-review":
			return "Needs review";
		case "waiting-on-author":
			return "Waiting on author";
	}
}
