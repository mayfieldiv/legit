import { describe, test, expect } from "bun:test";
import { formatAge, formatSize, formatReviewDecision, formatRepoShort } from "../src/lib/format";

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
