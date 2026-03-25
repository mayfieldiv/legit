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
 *  4. Current user is a requested reviewer → me-blocking
 *  5. Changes requested (via reviewDecision or individual reviews)
 *                         → waiting-on-author
 *  6. Another reviewer requested → waiting-on-other
 *  7. Default             → needs-review
 */
export function computeBlocker(pr: PR, currentUser: string, opts?: BlockerOptions): BlockerResult {
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

	// 4. Current user is a requested reviewer → me-blocking
	if (pr.requestedReviewers.includes(currentUser)) {
		return {
			blocker: currentUser,
			tier: "me-blocking",
			reason: "You are a requested reviewer",
		};
	}

	// 3. Changes requested — via reviewDecision field OR individual reviews
	const changesRequestedByReview = reviews.some((r) => r.state === "CHANGES_REQUESTED");
	const changesRequestedByDecision = pr.reviewDecision === "CHANGES_REQUESTED";
	if (changesRequestedByDecision || changesRequestedByReview) {
		return {
			blocker: pr.author,
			tier: "waiting-on-author",
			reason: "Changes requested",
		};
	}

	// 4. Another (non-current-user) reviewer is requested → waiting-on-other
	const otherReviewers = pr.requestedReviewers.filter((r) => r !== currentUser);
	if (otherReviewers.length > 0) {
		return {
			blocker: otherReviewers[0]!,
			tier: "waiting-on-other",
			reason: "Awaiting reviewer",
		};
	}

	// 5. Default — no specific blocker identified
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
