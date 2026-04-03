import { describe, test, expect } from "bun:test";
import {
	formatAge,
	formatSize,
	formatReviewDecision,
	formatRepoShort,
	checkIcon,
	reviewIcon,
	formatMergeable,
	blockerTierColor,
	checksSummary,
} from "../src/lib/format";
import { theme } from "../src/lib/theme";
import type { CheckRun } from "../src/lib/types";

describe("formatAge", () => {
	test("returns 'now' for dates less than a minute ago", () => {
		const now = new Date().toISOString();
		expect(formatAge(now)).toBe("now");
	});

	test("returns minutes for < 1 hour", () => {
		const fifteenMinAgo = new Date(Date.now() - 15 * 60 * 1000).toISOString();
		expect(formatAge(fifteenMinAgo)).toBe("15m");
	});

	test("returns '1m' for one minute ago", () => {
		const oneMinAgo = new Date(Date.now() - 60 * 1000).toISOString();
		expect(formatAge(oneMinAgo)).toBe("1m");
	});

	test("returns hours for < 1 day", () => {
		const threeHoursAgo = new Date(Date.now() - 3 * 60 * 60 * 1000).toISOString();
		expect(formatAge(threeHoursAgo)).toBe("3h");
	});

	test("returns '1h' for one hour ago", () => {
		const oneHourAgo = new Date(Date.now() - 60 * 60 * 1000).toISOString();
		expect(formatAge(oneHourAgo)).toBe("1h");
	});

	test("returns '1d' for yesterday", () => {
		const yesterday = new Date(Date.now() - 1 * 24 * 60 * 60 * 1000).toISOString();
		expect(formatAge(yesterday)).toBe("1d");
	});

	test("returns days for < 30 days", () => {
		const fifteenDaysAgo = new Date(Date.now() - 15 * 24 * 60 * 60 * 1000).toISOString();
		expect(formatAge(fifteenDaysAgo)).toBe("15d");
	});

	test("returns months for 30-365 days", () => {
		const ninetyDaysAgo = new Date(Date.now() - 90 * 24 * 60 * 60 * 1000).toISOString();
		expect(formatAge(ninetyDaysAgo)).toBe("3mo");
	});

	test("returns years and months for > 365 days", () => {
		const fourteenMonthsAgo = new Date(
			Date.now() - 14 * 30 * 24 * 60 * 60 * 1000,
		).toISOString();
		expect(formatAge(fourteenMonthsAgo)).toBe("1y2mo");
	});

	test("returns years only when no remainder months", () => {
		const twoYearsAgo = new Date(Date.now() - 24 * 30 * 24 * 60 * 60 * 1000).toISOString();
		expect(formatAge(twoYearsAgo)).toBe("2y");
	});
});

describe("formatSize", () => {
	test("formats additions and deletions", () => {
		expect(formatSize(123, 45)).toBe("+123/-45");
	});

	test("handles zero", () => {
		expect(formatSize(0, 0)).toBe("+0/-0");
	});
});

describe("formatRepoShort", () => {
	test("returns repo name from owner/repo slug", () => {
		expect(formatRepoShort("acme/widgets")).toBe("widgets");
	});

	test("returns empty string for undefined", () => {
		expect(formatRepoShort(undefined)).toBe("");
	});

	test("returns empty string for no argument", () => {
		expect(formatRepoShort()).toBe("");
	});

	test("returns the slug itself when no slash", () => {
		expect(formatRepoShort("widgets")).toBe("widgets");
	});
});

describe("formatReviewDecision", () => {
	test("formats APPROVED", () => {
		expect(formatReviewDecision("APPROVED")).toBe("approved");
	});

	test("formats CHANGES_REQUESTED", () => {
		expect(formatReviewDecision("CHANGES_REQUESTED")).toBe("changes requested");
	});

	test("formats REVIEW_REQUIRED", () => {
		expect(formatReviewDecision("REVIEW_REQUIRED")).toBe("");
	});

	test("lowercases unknown decisions", () => {
		expect(formatReviewDecision("SOMETHING_ELSE")).toBe("something_else");
	});

	test("returns empty string for empty input", () => {
		expect(formatReviewDecision("")).toBe("");
	});
});

// ── checkIcon ───────────────────────────────────────────────────────────────────

describe("checkIcon", () => {
	test("in-progress check returns pending icon with warning color", () => {
		const result = checkIcon({ name: "ci", status: "in_progress", conclusion: null });
		expect(result).toEqual({ icon: "●", fg: theme.warning });
	});

	test("queued check returns pending icon with warning color", () => {
		const result = checkIcon({ name: "ci", status: "queued", conclusion: null });
		expect(result).toEqual({ icon: "●", fg: theme.warning });
	});

	test("success returns check mark with success color", () => {
		const result = checkIcon({ name: "ci", status: "completed", conclusion: "success" });
		expect(result).toEqual({ icon: "✓", fg: theme.success });
	});

	test("failure returns X with error color", () => {
		const result = checkIcon({ name: "ci", status: "completed", conclusion: "failure" });
		expect(result).toEqual({ icon: "✗", fg: theme.error });
	});

	test("timed_out returns X with error color", () => {
		const result = checkIcon({ name: "ci", status: "completed", conclusion: "timed_out" });
		expect(result).toEqual({ icon: "✗", fg: theme.error });
	});

	test("cancelled returns X with error color", () => {
		const result = checkIcon({ name: "ci", status: "completed", conclusion: "cancelled" });
		expect(result).toEqual({ icon: "✗", fg: theme.error });
	});

	test("action_required returns X with warning color", () => {
		const result = checkIcon({
			name: "ci",
			status: "completed",
			conclusion: "action_required",
		});
		expect(result).toEqual({ icon: "✗", fg: theme.warning });
	});

	test("neutral returns dash with neutral color", () => {
		const result = checkIcon({ name: "ci", status: "completed", conclusion: "neutral" });
		expect(result).toEqual({ icon: "–", fg: theme.neutral });
	});

	test("skipped returns circle-slash with muted color", () => {
		const result = checkIcon({ name: "ci", status: "completed", conclusion: "skipped" });
		expect(result).toEqual({ icon: "⊘", fg: theme.muted });
	});

	test("stale returns refresh with warning color", () => {
		const result = checkIcon({ name: "ci", status: "completed", conclusion: "stale" });
		expect(result).toEqual({ icon: "⟳", fg: theme.warning });
	});

	test("unknown conclusion returns ? with neutral color", () => {
		const result = checkIcon({ name: "ci", status: "completed", conclusion: null });
		expect(result).toEqual({ icon: "?", fg: theme.neutral });
	});
});

// ── reviewIcon ─────────────────────────────────────────────────────────────────

describe("reviewIcon", () => {
	test("APPROVED returns check mark with success color", () => {
		expect(reviewIcon("APPROVED")).toEqual({ icon: "✓", fg: theme.success });
	});

	test("CHANGES_REQUESTED returns X with error color", () => {
		expect(reviewIcon("CHANGES_REQUESTED")).toEqual({ icon: "✗", fg: theme.error });
	});

	test("COMMENTED returns dot with accent color", () => {
		expect(reviewIcon("COMMENTED")).toEqual({ icon: "●", fg: theme.accent });
	});

	test("DISMISSED returns dash with muted color", () => {
		expect(reviewIcon("DISMISSED")).toEqual({ icon: "–", fg: theme.muted });
	});

	test("unknown state returns ? with neutral color", () => {
		expect(reviewIcon("SOMETHING_ELSE")).toEqual({ icon: "?", fg: theme.neutral });
	});
});

// ── formatMergeable ────────────────────────────────────────────────────────────

describe("formatMergeable", () => {
	test("CONFLICTING returns conflict text with error color", () => {
		expect(formatMergeable("CONFLICTING")).toEqual({ text: "! conflict", fg: theme.error });
	});

	test("MERGEABLE returns mergeable text with success color", () => {
		expect(formatMergeable("MERGEABLE")).toEqual({ text: "✓ mergeable", fg: theme.success });
	});

	test("UNKNOWN returns unknown text with muted color", () => {
		expect(formatMergeable("UNKNOWN")).toEqual({ text: "? merge unknown", fg: theme.muted });
	});

	test("empty string returns unknown text with muted color", () => {
		expect(formatMergeable("")).toEqual({ text: "? merge unknown", fg: theme.muted });
	});
});

// ── blockerTierColor ──────────────────────────────────────────────────────────

describe("blockerTierColor", () => {
	test("me-blocking returns selfHighlight", () => {
		expect(blockerTierColor("me-blocking")).toBe(theme.selfHighlight);
	});

	test("waiting-on-author returns warning", () => {
		expect(blockerTierColor("waiting-on-author")).toBe(theme.warning);
	});

	test("needs-review returns muted", () => {
		expect(blockerTierColor("needs-review")).toBe(theme.muted);
	});
});

// ── checksSummary ──────────────────────────────────────────────────────────────

const makeCheck = (status: CheckRun["status"], conclusion: CheckRun["conclusion"]): CheckRun => ({
	name: "test",
	status,
	conclusion,
});

describe("checksSummary", () => {
	test("empty array returns all zeros", () => {
		expect(checksSummary([])).toEqual({ passed: 0, failed: 0, pending: 0, total: 0 });
	});

	test("counts success as passed", () => {
		const checks = [makeCheck("completed", "success"), makeCheck("completed", "success")];
		expect(checksSummary(checks)).toEqual({ passed: 2, failed: 0, pending: 0, total: 2 });
	});

	test("counts failure, timed_out, cancelled as failed", () => {
		const checks = [
			makeCheck("completed", "failure"),
			makeCheck("completed", "timed_out"),
			makeCheck("completed", "cancelled"),
		];
		expect(checksSummary(checks)).toEqual({ passed: 0, failed: 3, pending: 0, total: 3 });
	});

	test("counts in_progress and queued as pending", () => {
		const checks = [makeCheck("in_progress", null), makeCheck("queued", null)];
		expect(checksSummary(checks)).toEqual({ passed: 0, failed: 0, pending: 2, total: 2 });
	});

	test("counts neutral, skipped as passed (non-failures)", () => {
		const checks = [makeCheck("completed", "neutral"), makeCheck("completed", "skipped")];
		expect(checksSummary(checks)).toEqual({ passed: 2, failed: 0, pending: 0, total: 2 });
	});

	test("mixed statuses are counted correctly", () => {
		const checks = [
			makeCheck("completed", "success"),
			makeCheck("completed", "failure"),
			makeCheck("in_progress", null),
			makeCheck("completed", "skipped"),
		];
		expect(checksSummary(checks)).toEqual({ passed: 2, failed: 1, pending: 1, total: 4 });
	});
});
