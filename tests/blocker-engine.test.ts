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

	test("reviewDecision APPROVED, no requested reviewers → needs-review", () => {
		const pr = makePR({ author: AUTHOR, reviewDecision: "APPROVED" });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("needs-review");
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

	test("draft PR where current user is author → waiting-on-author (blocker = self)", () => {
		const pr = makePR({ author: ME, isDraft: true });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("waiting-on-author");
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

// ── Edge cases ────────────────────────────────────────────────────────────────

describe("computeBlocker — edge cases", () => {
	test("current user is the author, no issues → needs-review", () => {
		const pr = makePR({ author: ME });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("needs-review");
	});

	test("current user is the author, CI failing → waiting-on-author (blocker = currentUser)", () => {
		const pr = makePR({ author: ME });
		const result = computeBlocker(pr, ME, { checks: [failedCheck()] });
		expect(result.tier).toBe("waiting-on-author");
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
