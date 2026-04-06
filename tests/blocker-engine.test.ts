import { describe, test, expect } from "bun:test";
import { computeBlocker, classifyThreads } from "../src/lib/blocker-engine";
import { makePR } from "./helpers";
import type { CheckRun, Review, FullReviewThread, ReviewComment } from "../src/lib/types";

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

// ── Thread helpers ───────────────────────────────────────────────────────────

function makeComment(
	author: string,
	opts?: { isBot?: boolean; createdAt?: string },
): ReviewComment {
	return {
		id: `comment-${Math.random().toString(36).slice(2, 8)}`,
		author,
		body: "comment body",
		createdAt: opts?.createdAt ?? "2026-03-15T00:00:00Z",
		url: "https://github.com/test",
		isBot: opts?.isBot ?? false,
	};
}

function makeThread(comments: ReviewComment[], opts?: { isResolved?: boolean }): FullReviewThread {
	return {
		id: `thread-${Math.random().toString(36).slice(2, 8)}`,
		isResolved: opts?.isResolved ?? false,
		path: "src/test.ts",
		line: 10,
		comments,
	};
}

// ── Base tier logic (no extended data) ──────────────────────────────────────

describe("computeBlocker — basic (no checks, no reviews)", () => {
	test("current user is requested reviewer → me-blocking", () => {
		const pr = makePR({ author: AUTHOR, requestedReviewers: [ME] });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("me-blocking");
		expect(result.blocker).toBe(ME);
	});

	test("another reviewer requested (not current user) → needs-review", () => {
		const pr = makePR({ author: AUTHOR, requestedReviewers: [OTHER] });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("needs-review");
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

	test("multiple others → needs-review, blocker is first reviewer", () => {
		const pr = makePR({ author: AUTHOR, requestedReviewers: ["dave", "eve"] });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("needs-review");
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

	test("CHANGES_REQUESTED review beats needs-review", () => {
		const pr = makePR({ author: AUTHOR, requestedReviewers: [OTHER] });
		const result = computeBlocker(pr, ME, {
			reviews: [review(OTHER, "CHANGES_REQUESTED")],
		});
		expect(result.tier).toBe("waiting-on-author");
	});

	test("me-blocking beats needs-review when current user also requested", () => {
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

	test("APPROVED overrides needs-review", () => {
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
		});
		const threads = [makeThread([makeComment(OTHER)])];
		const result = computeBlocker(pr, ME, { threads });
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

describe("computeBlocker — unresolved review threads (via opts.threads)", () => {
	test("unreplied threads → waiting-on-author", () => {
		const pr = makePR({ author: AUTHOR });
		const threads = [
			makeThread([makeComment(OTHER)]),
			makeThread([makeComment(OTHER)]),
		];
		const result = computeBlocker(pr, ME, { threads });
		expect(result.tier).toBe("waiting-on-author");
		expect(result.blocker).toBe(AUTHOR);
		expect(result.reason).toContain("2");
		expect(result.reason.toLowerCase()).toContain("thread");
	});

	test("all resolved threads → no effect on tier", () => {
		const pr = makePR({ author: AUTHOR });
		const threads = [
			makeThread([makeComment(OTHER)], { isResolved: true }),
			makeThread([makeComment(OTHER)], { isResolved: true }),
		];
		const result = computeBlocker(pr, ME, { threads });
		expect(result.tier).toBe("needs-review");
	});

	test("no threads provided → no effect on tier", () => {
		const pr = makePR({ author: AUTHOR });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("needs-review");
	});

	test("changes-requested beats unreplied threads", () => {
		const pr = makePR({ author: AUTHOR, reviewDecision: "CHANGES_REQUESTED" });
		const threads = [makeThread([makeComment(OTHER)])];
		const result = computeBlocker(pr, ME, { threads });
		expect(result.tier).toBe("waiting-on-author");
		expect(result.reason.toLowerCase()).toContain("changes");
	});

	test("unreplied threads override me-blocking (author must resolve first)", () => {
		const pr = makePR({ author: AUTHOR, requestedReviewers: [ME] });
		const threads = [makeThread([makeComment(OTHER)])];
		const result = computeBlocker(pr, ME, { threads });
		expect(result.tier).toBe("waiting-on-author");
		expect(result.blocker).toBe(AUTHOR);
	});

	test("singular thread has correct grammar", () => {
		const pr = makePR({ author: AUTHOR });
		const threads = [makeThread([makeComment(OTHER)])];
		const result = computeBlocker(pr, ME, { threads });
		expect(result.reason).toBe("1 unreplied thread");
	});

	test("plural threads has correct grammar", () => {
		const pr = makePR({ author: AUTHOR });
		const threads = [
			makeThread([makeComment(OTHER)]),
			makeThread([makeComment(OTHER)]),
		];
		const result = computeBlocker(pr, ME, { threads });
		expect(result.reason).toBe("2 unreplied threads");
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
});

// ── sole assignee ────────────────────────────────────────────────────────────

describe("computeBlocker — effective author (assignee takeover)", () => {
	// When the current user is an assignee but the original author is NOT,
	// the current user becomes the "effective author" — all waiting-on-author
	// results point to them and get elevated to me-blocking.

	test("assignee (author not assigned) with no issues → me-blocking via approved/needs-review", () => {
		// With no blockers, default is needs-review; effective author doesn't
		// change that since needs-review isn't a waiting-on-author result.
		const pr = makePR({ author: AUTHOR, assignees: [ME] });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("needs-review");
	});

	test("assignee (author not assigned) + CI failing → me-blocking", () => {
		const pr = makePR({ author: AUTHOR, assignees: [ME] });
		const result = computeBlocker(pr, ME, { checks: [failedCheck()] });
		expect(result.tier).toBe("me-blocking");
		expect(result.blocker).toBe(ME);
		expect(result.reason).toContain("CI");
	});

	test("assignee (author not assigned) + draft → me-blocking", () => {
		const pr = makePR({ author: AUTHOR, assignees: [ME], isDraft: true });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("me-blocking");
		expect(result.blocker).toBe(ME);
	});

	test("assignee (author not assigned) + merge conflict → me-blocking", () => {
		const pr = makePR({ author: AUTHOR, assignees: [ME], mergeable: "CONFLICTING" });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("me-blocking");
		expect(result.blocker).toBe(ME);
	});

	test("assignee (author not assigned) + changes requested → me-blocking", () => {
		const pr = makePR({ author: AUTHOR, assignees: [ME], reviewDecision: "CHANGES_REQUESTED" });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("me-blocking");
		expect(result.blocker).toBe(ME);
	});

	test("assignee (author not assigned) + approved → me-blocking (should merge)", () => {
		const pr = makePR({ author: AUTHOR, assignees: [ME], reviewDecision: "APPROVED" });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("me-blocking");
		expect(result.blocker).toBe(ME);
		expect(result.reason).toContain("merge");
	});

	test("assignee (author not assigned) + unreplied threads → me-blocking", () => {
		const pr = makePR({
			author: AUTHOR,
			assignees: [ME],
		});
		const threads = [makeThread([makeComment(OTHER)])];
		const result = computeBlocker(pr, ME, { threads });
		expect(result.tier).toBe("me-blocking");
		expect(result.blocker).toBe(ME);
	});

	test("both user and author assigned → normal rules (author stays author)", () => {
		const pr = makePR({ author: AUTHOR, assignees: [ME, AUTHOR] });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("needs-review");
	});

	test("both user and author assigned + CI failing → waiting-on-author (not me)", () => {
		const pr = makePR({ author: AUTHOR, assignees: [ME, AUTHOR] });
		const result = computeBlocker(pr, ME, { checks: [failedCheck()] });
		expect(result.tier).toBe("waiting-on-author");
		expect(result.blocker).toBe(AUTHOR);
	});

	test("another user assigned (not me) → no effect on current user", () => {
		const pr = makePR({ author: AUTHOR, assignees: [OTHER] });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("needs-review");
	});

	test("no assignees → normal rules apply", () => {
		const pr = makePR({ author: AUTHOR, assignees: [] });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("needs-review");
	});

	test("user assigned alongside other non-author user (author not assigned) → effective author", () => {
		// Both ME and OTHER are assigned, but AUTHOR is not → ME becomes effective author
		const pr = makePR({ author: AUTHOR, assignees: [ME, OTHER] });
		const result = computeBlocker(pr, ME, { checks: [failedCheck()] });
		expect(result.tier).toBe("me-blocking");
		expect(result.blocker).toBe(ME);
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

// ── classifyThreads ──────────────────────────────────────────────────────────

describe("classifyThreads", () => {
	test("resolved threads are ignored", () => {
		const threads = [
			makeThread([makeComment(OTHER), makeComment(AUTHOR)], { isResolved: true }),
		];
		const result = classifyThreads(threads);
		expect(result.unreplied).toBe(0);
		expect(result.awaitingReviewer).toBe(0);
	});

	test("thread with only reviewer comment → unreplied", () => {
		const threads = [makeThread([makeComment(OTHER)])];
		const result = classifyThreads(threads);
		expect(result.unreplied).toBe(1);
		expect(result.awaitingReviewer).toBe(0);
	});

	test("thread where author replied last → awaiting-reviewer", () => {
		const threads = [makeThread([makeComment(OTHER), makeComment(AUTHOR)])];
		const result = classifyThreads(threads);
		expect(result.unreplied).toBe(0);
		expect(result.awaitingReviewer).toBe(1);
		expect(result.awaitingByReviewer.get(OTHER)?.count).toBe(1);
	});

	test("ping-pong: reviewer replied after author → unreplied", () => {
		const threads = [makeThread([makeComment(OTHER), makeComment(AUTHOR), makeComment(OTHER)])];
		const result = classifyThreads(threads);
		expect(result.unreplied).toBe(1);
		expect(result.awaitingReviewer).toBe(0);
	});

	test("bot comment after author reply does not flip back to unreplied", () => {
		const threads = [
			makeThread([
				makeComment(OTHER),
				makeComment(AUTHOR),
				makeComment("github-actions[bot]", { isBot: true }),
			]),
		];
		const result = classifyThreads(threads);
		expect(result.unreplied).toBe(0);
		expect(result.awaitingReviewer).toBe(1);
	});

	test("thread with only bot comments → unreplied (no non-bot to identify)", () => {
		const threads = [
			makeThread([
				makeComment("bot1", { isBot: true }),
				makeComment("bot2", { isBot: true }),
			]),
		];
		const result = classifyThreads(threads);
		expect(result.unreplied).toBe(1);
		expect(result.awaitingReviewer).toBe(0);
	});

	test("empty thread (no comments) is skipped", () => {
		const threads = [makeThread([])];
		const result = classifyThreads(threads);
		expect(result.unreplied).toBe(0);
		expect(result.awaitingReviewer).toBe(0);
	});

	test("multiple reviewers tracked separately in awaitingByReviewer", () => {
		const threads = [
			makeThread([makeComment(OTHER), makeComment(AUTHOR)]),
			makeThread([makeComment("dave"), makeComment(AUTHOR)]),
			makeThread([makeComment(OTHER), makeComment(AUTHOR)]),
		];
		const result = classifyThreads(threads);
		expect(result.awaitingReviewer).toBe(3);
		expect(result.awaitingByReviewer.get(OTHER)?.count).toBe(2);
		expect(result.awaitingByReviewer.get("dave")?.count).toBe(1);
	});

	test("mixed unreplied and awaiting-reviewer", () => {
		const threads = [
			makeThread([makeComment(OTHER)]), // unreplied
			makeThread([makeComment(OTHER), makeComment(AUTHOR)]), // awaiting
		];
		const result = classifyThreads(threads);
		expect(result.unreplied).toBe(1);
		expect(result.awaitingReviewer).toBe(1);
	});

	test("oldest reply date tracked for tie-breaking", () => {
		const threads = [
			makeThread([
				makeComment(OTHER),
				makeComment(AUTHOR, { createdAt: "2026-03-10T00:00:00Z" }),
			]),
			makeThread([
				makeComment(OTHER),
				makeComment(AUTHOR, { createdAt: "2026-03-12T00:00:00Z" }),
			]),
		];
		const result = classifyThreads(threads);
		expect(result.awaitingByReviewer.get(OTHER)?.oldestReplyDate).toBe("2026-03-10T00:00:00Z");
	});
});

// ── computeBlocker — unreplied vs awaiting-reviewer threads ──────────────────

describe("computeBlocker — unreplied vs awaiting-reviewer threads", () => {
	test("unreplied threads → waiting-on-author", () => {
		const pr = makePR({ author: AUTHOR });
		const threads = [makeThread([makeComment(OTHER)])];
		const result = computeBlocker(pr, ME, { threads });
		expect(result.tier).toBe("waiting-on-author");
		expect(result.blocker).toBe(AUTHOR);
		expect(result.reason).toBe("1 unreplied thread");
	});

	test("plural unreplied threads", () => {
		const pr = makePR({ author: AUTHOR });
		const threads = [makeThread([makeComment(OTHER)]), makeThread([makeComment("dave")])];
		const result = computeBlocker(pr, ME, { threads });
		expect(result.reason).toBe("2 unreplied threads");
	});

	test("all threads awaiting-reviewer → needs-review with reviewer as blocker", () => {
		const pr = makePR({ author: AUTHOR });
		const threads = [makeThread([makeComment(OTHER), makeComment(AUTHOR)])];
		const result = computeBlocker(pr, ME, { threads });
		expect(result.tier).toBe("needs-review");
		expect(result.blocker).toBe(OTHER);
		expect(result.reason).toBe(`1 thread awaiting ${OTHER}`);
	});

	test("awaiting-reviewer with current user as reviewer → me-blocking", () => {
		const pr = makePR({ author: AUTHOR });
		const threads = [makeThread([makeComment(ME), makeComment(AUTHOR)])];
		const result = computeBlocker(pr, ME, { threads });
		expect(result.tier).toBe("me-blocking");
		expect(result.blocker).toBe(ME);
		expect(result.reason).toBe(`1 thread awaiting ${ME}`);
	});

	test("mixed unreplied + awaiting → waiting-on-author (unreplied takes priority)", () => {
		const pr = makePR({ author: AUTHOR });
		const threads = [
			makeThread([makeComment(OTHER)]), // unreplied
			makeThread([makeComment(OTHER), makeComment(AUTHOR)]), // awaiting
		];
		const result = computeBlocker(pr, ME, { threads });
		expect(result.tier).toBe("waiting-on-author");
		expect(result.blocker).toBe(AUTHOR);
		expect(result.reason).toBe("1 unreplied thread");
	});

	test("approved beats awaiting-reviewer (author should merge)", () => {
		const pr = makePR({ author: AUTHOR, reviewDecision: "APPROVED" });
		const threads = [makeThread([makeComment(OTHER), makeComment(AUTHOR)])];
		const result = computeBlocker(pr, ME, { threads });
		expect(result.tier).toBe("waiting-on-author");
		expect(result.blocker).toBe(AUTHOR);
		expect(result.reason).toContain("Approved");
	});

	test("unreplied threads beat approved", () => {
		const pr = makePR({ author: AUTHOR, reviewDecision: "APPROVED" });
		const threads = [makeThread([makeComment(OTHER)])];
		const result = computeBlocker(pr, ME, { threads });
		expect(result.tier).toBe("waiting-on-author");
		expect(result.reason).toContain("unreplied");
	});

	test("changes-requested beats unreplied threads", () => {
		const pr = makePR({ author: AUTHOR, reviewDecision: "CHANGES_REQUESTED" });
		const threads = [makeThread([makeComment(OTHER)])];
		const result = computeBlocker(pr, ME, { threads });
		expect(result.tier).toBe("waiting-on-author");
		expect(result.reason).toContain("Changes");
	});

	test("CI failing beats unreplied threads", () => {
		const pr = makePR({ author: AUTHOR });
		const threads = [makeThread([makeComment(OTHER)])];
		const result = computeBlocker(pr, ME, { threads, checks: [failedCheck()] });
		expect(result.tier).toBe("waiting-on-author");
		expect(result.reason).toContain("CI");
	});

	test("empty threads array → no thread-based blocking, falls through", () => {
		const pr = makePR({ author: AUTHOR });
		const result = computeBlocker(pr, ME, { threads: [] });
		expect(result.tier).toBe("needs-review");
	});

	test("all resolved threads → no thread-based blocking", () => {
		const pr = makePR({ author: AUTHOR });
		const threads = [
			makeThread([makeComment(OTHER), makeComment(AUTHOR)], { isResolved: true }),
		];
		const result = computeBlocker(pr, ME, { threads });
		expect(result.tier).toBe("needs-review");
	});

	test("reviewer with most awaiting threads becomes blocker", () => {
		const pr = makePR({ author: AUTHOR });
		const threads = [
			makeThread([makeComment(OTHER), makeComment(AUTHOR)]),
			makeThread([makeComment(OTHER), makeComment(AUTHOR)]),
			makeThread([makeComment("dave"), makeComment(AUTHOR)]),
		];
		const result = computeBlocker(pr, ME, { threads });
		expect(result.blocker).toBe(OTHER);
		expect(result.reason).toBe(`3 threads awaiting ${OTHER}`);
	});

	test("tie-break: reviewer waiting longest becomes blocker", () => {
		const pr = makePR({ author: AUTHOR });
		const threads = [
			makeThread([
				makeComment(OTHER),
				makeComment(AUTHOR, { createdAt: "2026-03-12T00:00:00Z" }),
			]),
			makeThread([
				makeComment("dave"),
				makeComment(AUTHOR, { createdAt: "2026-03-10T00:00:00Z" }),
			]),
		];
		const result = computeBlocker(pr, ME, { threads });
		// dave has been waiting since 03-10, bob since 03-12 → dave wins tie
		expect(result.blocker).toBe("dave");
	});

	test("effective author (assignee) with unreplied threads → me-blocking", () => {
		const pr = makePR({ author: AUTHOR, assignees: [ME] });
		const threads = [makeThread([makeComment(OTHER)])];
		const result = computeBlocker(pr, ME, { threads });
		expect(result.tier).toBe("me-blocking");
		expect(result.blocker).toBe(ME);
	});

	test("no threads → needs-review (thread-agnostic path)", () => {
		const pr = makePR({ author: AUTHOR });
		const result = computeBlocker(pr, ME);
		expect(result.tier).toBe("needs-review");
	});

	test("all threads awaiting-reviewer → needs-review for that reviewer", () => {
		const pr = makePR({ author: AUTHOR });
		const threads = [makeThread([makeComment(OTHER), makeComment(AUTHOR)])];
		const result = computeBlocker(pr, ME, { threads });
		expect(result.tier).toBe("needs-review");
		expect(result.blocker).toBe(OTHER);
	});

	test("PR #568 scenario: author replied to all threads → awaiting cmbankester", () => {
		// Simulates the real-world case that motivated this feature
		const pr = makePR({ author: "mayfieldiv" });
		const threads = [
			// Thread 1: cmbankester → cmbankester → mayfieldiv
			makeThread([
				makeComment("cmbankester", { createdAt: "2026-04-01T00:00:00Z" }),
				makeComment("cmbankester", { createdAt: "2026-04-02T00:00:00Z" }),
				makeComment("mayfieldiv", { createdAt: "2026-04-02T12:00:00Z" }),
			]),
			// Thread 2: cmbankester only (unreplied)
			makeThread([makeComment("cmbankester", { createdAt: "2026-04-02T00:00:00Z" })]),
		];
		// With one unreplied thread, still waiting on author
		const result = computeBlocker(pr, "someuser", { threads });
		expect(result.tier).toBe("waiting-on-author");
		expect(result.blocker).toBe("mayfieldiv");
		expect(result.reason).toBe("1 unreplied thread");
	});

	test("PR #568 scenario: author replied to ALL threads → awaiting cmbankester", () => {
		const pr = makePR({ author: "mayfieldiv" });
		const threads = [
			makeThread([
				makeComment("cmbankester"),
				makeComment("cmbankester"),
				makeComment("mayfieldiv"),
			]),
			makeThread([makeComment("cmbankester"), makeComment("mayfieldiv")]),
		];
		const result = computeBlocker(pr, "someuser", { threads });
		expect(result.tier).toBe("needs-review");
		expect(result.blocker).toBe("cmbankester");
		expect(result.reason).toBe("2 threads awaiting cmbankester");
	});
});
