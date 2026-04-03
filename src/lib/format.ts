/**
 * Pure formatting helpers — no side effects, easily testable.
 */

import type { CheckRun, ReviewState } from "./types";
import type { Tier } from "./blocker-engine";
import { theme } from "./theme";

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

// ── Status → icon/color mappings ────────────────────────────────────────────

/**
 * Map a CI check run to its display icon and color.
 */
export function checkIcon(check: CheckRun): { icon: string; fg: string } {
	if (check.status !== "completed") {
		return { icon: "●", fg: theme.warning };
	}
	switch (check.conclusion) {
		case "success":
			return { icon: "✓", fg: theme.success };
		case "failure":
		case "timed_out":
		case "cancelled":
			return { icon: "✗", fg: theme.error };
		case "action_required":
			return { icon: "✗", fg: theme.warning };
		case "neutral":
			return { icon: "–", fg: theme.neutral };
		case "skipped":
			return { icon: "⊘", fg: theme.muted };
		case "stale":
			return { icon: "⟳", fg: theme.warning };
		default:
			return { icon: "?", fg: theme.neutral };
	}
}

/**
 * Map a review state to its display icon and color.
 */
export function reviewIcon(state: string): { icon: string; fg: string } {
	switch (state) {
		case "APPROVED":
			return { icon: "✓", fg: theme.success };
		case "CHANGES_REQUESTED":
			return { icon: "✗", fg: theme.error };
		case "COMMENTED":
			return { icon: "●", fg: theme.accent };
		case "DISMISSED":
			return { icon: "–", fg: theme.muted };
		default:
			return { icon: "?", fg: theme.neutral };
	}
}

/**
 * Map mergeable status to display text and color.
 */
export function formatMergeable(status: string): { text: string; fg: string } {
	switch (status) {
		case "CONFLICTING":
			return { text: "! conflict", fg: theme.error };
		case "MERGEABLE":
			return { text: "✓ mergeable", fg: theme.success };
		default:
			return { text: "? merge unknown", fg: theme.muted };
	}
}

/**
 * Map a blocker tier to its theme color.
 */
export function blockerTierColor(tier: Tier): string {
	switch (tier) {
		case "me-blocking":
			return theme.selfHighlight;
		case "waiting-on-author":
			return theme.warning;
		case "needs-review":
			return theme.muted;
	}
}

/**
 * Summarize check run counts by outcome.
 */
export function checksSummary(checks: CheckRun[]): {
	passed: number;
	failed: number;
	pending: number;
	total: number;
} {
	let passed = 0;
	let failed = 0;
	let pending = 0;
	for (const c of checks) {
		if (c.status !== "completed") {
			pending++;
		} else if (c.conclusion === "success") {
			passed++;
		} else if (
			c.conclusion === "failure" ||
			c.conclusion === "timed_out" ||
			c.conclusion === "cancelled"
		) {
			failed++;
		} else {
			passed++; // neutral, skipped, etc. count as non-failures
		}
	}
	return { passed, failed, pending, total: checks.length };
}
