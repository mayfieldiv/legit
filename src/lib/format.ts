/**
 * Pure formatting helpers — no side effects, easily testable.
 */

import type { CheckRun, ReviewState } from "./types";

/**
 * Format a date string as relative age (e.g. "15m", "3h", "2d", "3mo", "1y2mo").
 */
export function formatAge(dateStr: string): string {
	const now = Date.now();
	const then = new Date(dateStr).getTime();
	const seconds = Math.floor((now - then) / 1000);

	if (seconds < 60) return "now";

	const minutes = Math.floor(seconds / 60);
	if (minutes < 60) return `${minutes}m`;

	const hours = Math.floor(minutes / 60);
	if (hours < 24) return `${hours}h`;

	const days = Math.floor(hours / 24);
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
 * Format a repo slug for compact display.
 * "owner/repo" → "repo", undefined → "".
 */
export function formatRepoShort(slug?: string): string {
	if (!slug) return "";
	const parts = slug.split("/");
	return parts[parts.length - 1] ?? slug;
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
			return "";
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
		case "skipped":
		case "stale":
		case "success":
		case "neutral":
			return 2;
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
