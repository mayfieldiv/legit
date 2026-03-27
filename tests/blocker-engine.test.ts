import { describe, test, expect } from "bun:test";
import { computeBlocker } from "../src/lib/blocker-engine";
import { makePR } from "./helpers";
import type { CheckRun, Review } from "../src/lib/types";

// ── Helpers ──────────────────────────────────────────────────────────────────

function failedCheck(name = "ci"): CheckRun {
	return { name, status: "completed", conclusion: "failure" };
}

function passedCheck(name = "ci"): CheckRun {
	return { name, status: "completed", conclusion: "success" };
}

function pendingCheck(name = "ci"): CheckRun {
	return { name, status: "in_progress", conclusion: null };
}

function review(user: string, state: Review["state"]): Review {
	return { user, state };
}

const ME = "alice";
const OTHER = "bob";
const AUTHOR = "charlie";

// ── Base tier logic (no extended data) ──────────────────────────────────────

describe("computeBlocker — basic (no checks, no reviews)", () => {
	test("current user is requested reviewer → me-blocking", () => {
		const pr = makePR({ author: AUTHOR, requestedReviewers: [ME] });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("me-blocking");
		expect(result.blocker).toBe(ME);
	});

	test("another reviewer requested (not current user) → waiting-on-other", () => {
		const pr = makePR({ author: AUTHOR, requestedReviewers: [OTHER] });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("waiting-on-other");
		expect(result.blocker).toBe(OTHER);
	});

	test("reviewDecision CHANGES_REQUESTED → waiting-on-author", () => {
		const pr = makePR({ author: AUTHOR, reviewDecision: "CHANGES_REQUESTED" });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("waiting-on-author");
		expect(result.blocker).toBe(AUTHOR);
	});

	test("no reviewers, no reviews, no CI data → needs-review", () => {
		const pr = makePR({ author: AUTHOR });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("needs-review");
	});

	test("reviewDecision REVIEW_REQUIRED → needs-review", () => {
		const pr = makePR({ author: AUTHOR, reviewDecision: "REVIEW_REQUIRED" });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("needs-review");
	});

	test("reviewDecision APPROVED → waiting-on-author (author should merge)", () => {
		const pr = makePR({ author: AUTHOR, reviewDecision: "APPROVED" });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("waiting-on-author");
		expect(result.blocker).toBe(AUTHOR);
		expect(result.reason.toLowerCase()).toContain("approved");
	});
});

// ── CI failing ───────────────────────────────────────────────────────────────

describe("computeBlocker — CI checks", () => {
	test("failing check → waiting-on-author, blocker is author, reason mentions CI", () => {
		const pr = makePR({ author: AUTHOR });
		const result = computeBlocker(pr, ME, { checks: [failedCheck()] });
		expect(result.tier).toBe("waiting-on-author");
		expect(result.blocker).toBe(AUTHOR);
		expect(result.reason.toLowerCase()).toContain("ci");
	});

	test("timed_out check → waiting-on-author", () => {
		const pr = makePR({ author: AUTHOR });
		const result = computeBlocker(pr, ME, {
			checks: [{ name: "build", status: "completed", conclusion: "timed_out" }],
		});
		expect(result.tier).toBe("waiting-on-author");
	});

	test("cancelled check → waiting-on-author", () => {
		const pr = makePR({ author: AUTHOR });
		const result = computeBlocker(pr, ME, {
			checks: [{ name: "build", status: "completed", conclusion: "cancelled" }],
		});
		expect(result.tier).toBe("waiting-on-author");
	});

	test("in_progress check (not completed) → not CI-failing", () => {
		const pr = makePR({ author: AUTHOR });
		const result = computeBlocker(pr, ME, { checks: [pendingCheck()] });
		// Should not be waiting-on-author due to CI
		expect(result.tier).not.toBe("waiting-on-author");
	});

	test("all checks passing → no CI-based waiting-on-author", () => {
		const pr = makePR({ author: AUTHOR });
		const result = computeBlocker(pr, ME, { checks: [passedCheck()] });
		expect(result.tier).toBe("needs-review");
	});

	test("mixed checks: one passing one failing → waiting-on-author", () => {
		const pr = makePR({ author: AUTHOR });
		const result = computeBlocker(pr, ME, {
			checks: [passedCheck("lint"), failedCheck("test")],
		});
		expect(result.tier).toBe("waiting-on-author");
	});

	test("CI failing overrides me-blocking (requestedReviewer = currentUser)", () => {
		const pr = makePR({ author: AUTHOR, requestedReviewers: [ME] });
		const result = computeBlocker(pr, ME, { checks: [failedCheck()] });
		expect(result.tier).toBe("waiting-on-author");
		expect(result.blocker).toBe(AUTHOR);
	});
});

// ── Individual reviews ────────────────────────────────────────────────────────

describe("computeBlocker — individual reviews", () => {
	test("CHANGES_REQUESTED review by anyone → waiting-on-author", () => {
		const pr = makePR({ author: AUTHOR });
		const result = computeBlocker(pr, ME, {
			reviews: [review(OTHER, "CHANGES_REQUESTED")],
		});
		expect(result.tier).toBe("waiting-on-author");
		expect(result.blocker).toBe(AUTHOR);
	});

	test("CHANGES_REQUESTED review by current user → waiting-on-author", () => {
		const pr = makePR({ author: AUTHOR });
		const result = computeBlocker(pr, ME, {
			reviews: [review(ME, "CHANGES_REQUESTED")],
		});
		expect(result.tier).toBe("waiting-on-author");
		expect(result.blocker).toBe(AUTHOR);
	});

	test("APPROVED review by current user, no other issues → needs-review", () => {
		const pr = makePR({ author: AUTHOR });
		const result = computeBlocker(pr, ME, {
			reviews: [review(ME, "APPROVED")],
		});
		expect(result.tier).toBe("needs-review");
	});

	test("COMMENTED review does not affect tier", () => {
		const pr = makePR({ author: AUTHOR });
		const result = computeBlocker(pr, ME, {
			reviews: [review(OTHER, "COMMENTED")],
		});
		expect(result.tier).toBe("needs-review");
	});

	test("individual CHANGES_REQUESTED overrides individual APPROVED from another user", () => {
		const pr = makePR({ author: AUTHOR });
		const result = computeBlocker(pr, ME, {
			reviews: [review("dave", "APPROVED"), review(OTHER, "CHANGES_REQUESTED")],
		});
		expect(result.tier).toBe("waiting-on-author");
	});
});

// ── Multiple reviewers ────────────────────────────────────────────────────────

describe("computeBlocker — multiple requestedReviewers", () => {
	test("current user plus others → me-blocking (current user takes priority)", () => {
		const pr = makePR({ author: AUTHOR, requestedReviewers: [OTHER, ME] });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("me-blocking");
		expect(result.blocker).toBe(ME);
	});

	test("multiple others → waiting-on-other, blocker is first reviewer", () => {
		const pr = makePR({ author: AUTHOR, requestedReviewers: ["dave", "eve"] });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("waiting-on-other");
		expect(result.blocker).toBe("dave");
	});
});

// ── Priority precedence ───────────────────────────────────────────────────────

describe("computeBlocker — precedence ordering", () => {
	test("CI failing beats CHANGES_REQUESTED review decision", () => {
		const pr = makePR({ author: AUTHOR, reviewDecision: "CHANGES_REQUESTED" });
		const result = computeBlocker(pr, ME, { checks: [failedCheck()] });
		// Both resolve to waiting-on-author, but reason should mention CI
		expect(result.tier).toBe("waiting-on-author");
		expect(result.reason.toLowerCase()).toContain("ci");
	});

	test("CHANGES_REQUESTED review beats waiting-on-other", () => {
		const pr = makePR({ author: AUTHOR, requestedReviewers: [OTHER] });
		const result = computeBlocker(pr, ME, {
			reviews: [review(OTHER, "CHANGES_REQUESTED")],
		});
		expect(result.tier).toBe("waiting-on-author");
	});

	test("me-blocking beats waiting-on-other when current user also requested", () => {
		const pr = makePR({ author: AUTHOR, requestedReviewers: [ME, OTHER] });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("me-blocking");
	});

	test("CHANGES_REQUESTED by other beats me-blocking (author must respond first)", () => {
		// Current user is a pending reviewer but another reviewer already requested
		// changes — the author must respond before we need to re-review.
		const pr = makePR({ author: AUTHOR, requestedReviewers: [ME] });
		const result = computeBlocker(pr, ME, {
			reviews: [review(OTHER, "CHANGES_REQUESTED")],
		});
		expect(result.tier).toBe("waiting-on-author");
		expect(result.blocker).toBe(AUTHOR);
	});
});

// ── Draft PRs ─────────────────────────────────────────────────────────────────

describe("computeBlocker — draft PRs", () => {
	test("draft PR → waiting-on-author, blocker is author", () => {
		const pr = makePR({ author: AUTHOR, isDraft: true });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("waiting-on-author");
		expect(result.blocker).toBe(AUTHOR);
	});

	test("draft reason mentions draft", () => {
		const pr = makePR({ author: AUTHOR, isDraft: true });
		const result = computeBlocker(pr, ME);
		expect(result.reason.toLowerCase()).toContain("draft");
	});

	test("draft overrides me-blocking (don't review a draft)", () => {
		const pr = makePR({ author: AUTHOR, isDraft: true, requestedReviewers: [ME] });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("waiting-on-author");
	});

	test("non-draft PR with same data is not waiting-on-author due to draft", () => {
		const pr = makePR({ author: AUTHOR, isDraft: false });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("needs-review");
	});

	test("draft PR where current user is author → me-blocking (blocker = self)", () => {
		// waiting-on-author with blocker === currentUser elevates to me-blocking.
		const pr = makePR({ author: ME, isDraft: true });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("me-blocking");
		expect(result.blocker).toBe(ME);
	});
});

// ── Merge conflicts ───────────────────────────────────────────────────────────

describe("computeBlocker — merge conflicts", () => {
	test("CONFLICTING → waiting-on-author, blocker is author", () => {
		const pr = makePR({ author: AUTHOR, mergeable: "CONFLICTING" });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("waiting-on-author");
		expect(result.blocker).toBe(AUTHOR);
	});

	test("conflict reason mentions conflict", () => {
		const pr = makePR({ author: AUTHOR, mergeable: "CONFLICTING" });
		const result = computeBlocker(pr, ME);
		expect(result.reason.toLowerCase()).toContain("conflict");
	});

	test("conflict overrides me-blocking", () => {
		const pr = makePR({ author: AUTHOR, mergeable: "CONFLICTING", requestedReviewers: [ME] });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("waiting-on-author");
	});

	test("MERGEABLE → not conflict-blocked", () => {
		const pr = makePR({ author: AUTHOR, mergeable: "MERGEABLE" });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("needs-review");
	});

	test("UNKNOWN mergeable → not conflict-blocked", () => {
		const pr = makePR({ author: AUTHOR, mergeable: "UNKNOWN" });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("needs-review");
	});

	test("CI failing + conflict: CI reason takes precedence", () => {
		const pr = makePR({ author: AUTHOR, mergeable: "CONFLICTING" });
		const result = computeBlocker(pr, ME, {
			checks: [{ name: "ci", status: "completed", conclusion: "failure" }],
		});
		expect(result.tier).toBe("waiting-on-author");
		expect(result.reason.toLowerCase()).toContain("ci");
	});
});

// ── Approved PRs ─────────────────────────────────────────────────────────────

describe("computeBlocker — approved review decision", () => {
	test("APPROVED with no pending reviewers → waiting-on-author", () => {
		const pr = makePR({ author: AUTHOR, reviewDecision: "APPROVED" });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("waiting-on-author");
		expect(result.blocker).toBe(AUTHOR);
	});

	test("APPROVED with pending reviewer → still waiting-on-author (not me-blocking)", () => {
		// Reviewer already approved; pending reviewers are irrelevant.
		const pr = makePR({ author: AUTHOR, reviewDecision: "APPROVED", requestedReviewers: [ME] });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("waiting-on-author");
		expect(result.blocker).toBe(AUTHOR);
	});

	test("APPROVED overrides waiting-on-other", () => {
		const pr = makePR({
			author: AUTHOR,
			reviewDecision: "APPROVED",
			requestedReviewers: [OTHER],
		});
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("waiting-on-author");
	});

	test("conflict beats APPROVED (author must rebase before merging)", () => {
		const pr = makePR({
			author: AUTHOR,
			reviewDecision: "APPROVED",
			mergeable: "CONFLICTING",
		});
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("waiting-on-author");
		expect(result.reason.toLowerCase()).toContain("conflict");
	});

	test("CI failing beats APPROVED", () => {
		const pr = makePR({ author: AUTHOR, reviewDecision: "APPROVED" });
		const result = computeBlocker(pr, ME, {
			checks: [{ name: "ci", status: "completed", conclusion: "failure" }],
		});
		expect(result.tier).toBe("waiting-on-author");
		expect(result.reason.toLowerCase()).toContain("ci");
	});

	test("unresolved threads beat APPROVED", () => {
		const pr = makePR({
			author: AUTHOR,
			reviewDecision: "APPROVED",
			comments: { total: 2, unresolved: 1, unresolvedHuman: 1, unresolvedBot: 0 },
		});
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("waiting-on-author");
		expect(result.reason.toLowerCase()).toContain("thread");
	});

	test("individual APPROVED review (not reviewDecision) does not trigger waiting-on-author", () => {
		// reviewDecision is empty — only one person reviewed but GitHub hasn't
		// set the overall decision to APPROVED yet.
		const pr = makePR({ author: AUTHOR });
		const result = computeBlocker(pr, ME, { reviews: [{ user: OTHER, state: "APPROVED" }] });
		expect(result.tier).toBe("needs-review");
	});
});

// ── Unresolved review threads ─────────────────────────────────────────────────

describe("computeBlocker — unresolved review threads", () => {
	test("human unresolved threads → waiting-on-author", () => {
		const pr = makePR({
			author: AUTHOR,
			comments: { total: 3, unresolved: 2, unresolvedHuman: 2, unresolvedBot: 0 },
		});
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("waiting-on-author");
		expect(result.blocker).toBe(AUTHOR);
		expect(result.reason).toContain("2");
		expect(result.reason.toLowerCase()).toContain("thread");
	});

	test("bot unresolved threads → waiting-on-author", () => {
		const pr = makePR({
			author: AUTHOR,
			comments: { total: 1, unresolved: 1, unresolvedHuman: 0, unresolvedBot: 1 },
		});
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("waiting-on-author");
		expect(result.blocker).toBe(AUTHOR);
	});

	test("mixed human + bot unresolved threads → waiting-on-author, reason shows total", () => {
		const pr = makePR({
			author: AUTHOR,
			comments: { total: 5, unresolved: 3, unresolvedHuman: 2, unresolvedBot: 1 },
		});
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("waiting-on-author");
		expect(result.reason).toContain("3");
	});

	test("zero unresolved threads → no effect on tier", () => {
		const pr = makePR({
			author: AUTHOR,
			comments: { total: 5, unresolved: 0, unresolvedHuman: 0, unresolvedBot: 0 },
		});
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("needs-review");
	});

	test("missing comments field (not yet loaded) → no effect on tier", () => {
		const pr = makePR({ author: AUTHOR });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("needs-review");
	});

	test("changes-requested beats unresolved threads in reason", () => {
		// Both apply — changes requested fires first (more specific feedback).
		const pr = makePR({
			author: AUTHOR,
			reviewDecision: "CHANGES_REQUESTED",
			comments: { total: 3, unresolved: 2, unresolvedHuman: 2, unresolvedBot: 0 },
		});
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("waiting-on-author");
		expect(result.reason.toLowerCase()).toContain("changes");
	});

	test("unresolved threads override me-blocking (author must resolve first)", () => {
		const pr = makePR({
			author: AUTHOR,
			requestedReviewers: [ME],
			comments: { total: 2, unresolved: 1, unresolvedHuman: 1, unresolvedBot: 0 },
		});
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("waiting-on-author");
		expect(result.blocker).toBe(AUTHOR);
	});

	test("unresolved threads override waiting-on-other (author must resolve first)", () => {
		const pr = makePR({
			author: AUTHOR,
			requestedReviewers: [OTHER],
			comments: { total: 2, unresolved: 1, unresolvedHuman: 0, unresolvedBot: 1 },
		});
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("waiting-on-author");
	});

	test("singular thread has correct grammar", () => {
		const pr = makePR({
			author: AUTHOR,
			comments: { total: 1, unresolved: 1, unresolvedHuman: 1, unresolvedBot: 0 },
		});
		const result = computeBlocker(pr, ME);
		expect(result.reason).toBe("1 unresolved thread");
	});

	test("plural threads has correct grammar", () => {
		const pr = makePR({
			author: AUTHOR,
			comments: { total: 3, unresolved: 2, unresolvedHuman: 2, unresolvedBot: 0 },
		});
		const result = computeBlocker(pr, ME);
		expect(result.reason).toBe("2 unresolved threads");
	});
});

// ── Edge cases ────────────────────────────────────────────────────────────────

describe("computeBlocker — edge cases", () => {
	test("current user is the author, no issues → needs-review", () => {
		const pr = makePR({ author: ME });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("needs-review");
	});

	test("current user is the author, CI failing → me-blocking (blocker = currentUser)", () => {
		// waiting-on-author with blocker === currentUser elevates to me-blocking.
		const pr = makePR({ author: ME });
		const result = computeBlocker(pr, ME, { checks: [failedCheck()] });
		expect(result.tier).toBe("me-blocking");
		expect(result.blocker).toBe(ME);
	});

	test("empty checks array → treated as no CI data", () => {
		const pr = makePR({ author: AUTHOR });
		const result = computeBlocker(pr, ME, { checks: [] });
		expect(result.tier).toBe("needs-review");
	});

	test("skipped/neutral checks do not count as failing", () => {
		const pr = makePR({ author: AUTHOR });
		const result = computeBlocker(pr, ME, {
			checks: [
				{ name: "skip", status: "completed", conclusion: "skipped" },
				{ name: "neutral", status: "completed", conclusion: "neutral" },
			],
		});
		expect(result.tier).not.toBe("waiting-on-author");
	});

	test("result always has a non-empty reason string", () => {
		const cases = [
			makePR({ author: AUTHOR, requestedReviewers: [ME] }),
			makePR({ author: AUTHOR }),
			makePR({ author: AUTHOR, reviewDecision: "CHANGES_REQUESTED" }),
			makePR({ author: AUTHOR, requestedReviewers: [OTHER] }),
		];
		for (const pr of cases) {
			const result = computeBlocker(pr, ME);
			expect(typeof result.reason).toBe("string");
			expect(result.reason.length).toBeGreaterThan(0);
		}
	});
});
