/**
 * Blocker/Priority Engine
 *
 * Pure function that determines who is blocking a PR and assigns a priority
 * tier. No side effects — all inputs are passed explicitly.
 *
 * Tier priority order (highest → lowest urgency to the current user):
 *   me-blocking → needs-review → waiting-on-other → waiting-on-author
 */

import type { PR, CheckRun, Review } from "./types";

// ── Public types ─────────────────────────────────────────────────────────────

export type Tier =
	| "me-blocking" // current user must act
	| "needs-review" // needs a reviewer's attention (no specific person)
	| "waiting-on-other" // a different reviewer must act
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
 *  5. Unresolved threads  → waiting-on-author (author must resolve open
 *                           review comments; only when data is loaded)
 *  6. Approved            → waiting-on-author (author should merge; no more
 *                           reviewer action needed regardless of pending requests)
 *  7. Current user is a requested reviewer → me-blocking
 *  8. Another reviewer requested → waiting-on-other
 *  9. Default             → needs-review
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

	// 1. CI failing → waiting-on-author, regardless of reviewers
	if (isCiFailing(checks)) {
		return {
			blocker: pr.author,
			tier: "waiting-on-author",
			reason: "CI is failing",
		};
	}

	// 2. Draft → waiting-on-author (author isn't ready for review)
	if (pr.isDraft) {
		return {
			blocker: pr.author,
			tier: "waiting-on-author",
			reason: "Draft — not ready for review",
		};
	}

	// 3. Merge conflict → waiting-on-author (author must rebase)
	if (pr.mergeable === "CONFLICTING") {
		return {
			blocker: pr.author,
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
			blocker: pr.author,
			tier: "waiting-on-author",
			reason: "Changes requested",
		};
	}

	// 5. Unresolved review threads → author must address open comments before
	//    reviewers need to re-examine. Only fires when comment data is available
	//    (lazily populated after the PR summary is fetched).
	const unresolvedThreads =
		(pr.comments?.unresolvedHuman ?? 0) + (pr.comments?.unresolvedBot ?? 0);
	if (unresolvedThreads > 0) {
		return {
			blocker: pr.author,
			tier: "waiting-on-author",
			reason: `${unresolvedThreads} unresolved thread${unresolvedThreads === 1 ? "" : "s"}`,
		};
	}

	// 6. Approved — the PR has the green light; author's turn to merge (or fix
	//    whatever is blocking the merge, e.g. a conflict that appeared after approval).
	if (pr.reviewDecision === "APPROVED") {
		return {
			blocker: pr.author,
			tier: "waiting-on-author",
			reason: "Approved — ready to merge",
		};
	}

	// 7. Current user is a requested reviewer → me-blocking
	if (pr.requestedReviewers.includes(currentUser)) {
		return {
			blocker: currentUser,
			tier: "me-blocking",
			reason: "You are a requested reviewer",
		};
	}

	// 8. Another (non-current-user) reviewer is requested → waiting-on-other
	const otherReviewers = pr.requestedReviewers.filter((r) => r !== currentUser);
	if (otherReviewers.length > 0) {
		return {
			blocker: otherReviewers[0]!,
			tier: "waiting-on-other",
			reason: "Awaiting reviewer",
		};
	}

	// 7. Default — no specific blocker identified
	return {
		blocker: "",
		tier: "needs-review",
		reason: "Awaiting review",
	};
}

// ── Tier sort order ───────────────────────────────────────────────────────────

const TIER_ORDER: Record<Tier, number> = {
	"me-blocking": 0,
	"needs-review": 1,
	"waiting-on-other": 2,
	"waiting-on-author": 3,
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
		case "waiting-on-other":
			return "Waiting on other";
		case "waiting-on-author":
			return "Waiting on author";
	}
}
