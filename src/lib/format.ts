/**
 * Pure formatting helpers — no side effects, easily testable.
 */

import type { CheckRun, ReviewState } from "./types";

/**
 * Format a date string as relative age (e.g. "2d", "3mo", "1y2mo").
 */
export function formatAge(dateStr: string): string {
	const now = Date.now();
	const then = new Date(dateStr).getTime();
	const days = Math.floor((now - then) / (1000 * 60 * 60 * 24));

	if (days === 0) return "today";
	if (days === 1) return "1d";
	if (days < 30) return `${days}d`;

	const months = Math.floor(days / 30);
	if (months < 12) return `${months}mo`;

	const years = Math.floor(months / 12);
	const rem = months % 12;
	return rem > 0 ? `${years}y${rem}mo` : `${years}y`;
}

/**
 * Format additions/deletions as "+N/-M".
 */
export function formatSize(additions: number, deletions: number): string {
	return `+${additions}/-${deletions}`;
}

/**
 * Format review decision for display.
 */
export function formatReviewDecision(decision: string): string {
	switch (decision) {
		case "APPROVED":
			return "approved";
		case "CHANGES_REQUESTED":
			return "changes requested";
		case "REVIEW_REQUIRED":
			return "review required";
		default:
			return decision ? decision.toLowerCase() : "";
	}
}

export function formatReviewState(state: ReviewState): string {
	switch (state) {
		case "APPROVED":
			return "approved";
		case "CHANGES_REQUESTED":
			return "changes requested";
		case "COMMENTED":
			return "commented";
		case "DISMISSED":
			return "dismissed";
	}
}

export function checkSortGroup(check: CheckRun): number {
	if (check.status !== "completed") return 1;
	switch (check.conclusion) {
		case "failure":
		case "timed_out":
		case "cancelled":
		case "action_required":
			return 0;
		default:
			return 2;
	}
}

export function sortCheckRuns(checks: CheckRun[]): CheckRun[] {
	return checks.toSorted((a, b) => {
		const groupDiff = checkSortGroup(a) - checkSortGroup(b);
		if (groupDiff !== 0) return groupDiff;
		return a.name.localeCompare(b.name);
	});
}
