/**
 * Domain types for legit.
 *
 * These are the core data structures consumed across the app —
 * components, formatters, engines, CLI output. They live here
 * (not in github-client) because they will grow with computed
 * fields (sizeBreakdown, blocker, tier) that have nothing to
 * do with the GitHub API.
 */

export interface PR {
	number: number;
	title: string;
	author: string;
	createdAt: string;
	updatedAt: string;
	additions: number;
	deletions: number;
	isDraft: boolean;
	labels: string[];
	requestedReviewers: string[];
	assignees: string[];
	reviewDecision: string;
	mergeable: string;
	lastCommitDate: string;
}

export interface PRDetail extends PR {
	body: string;
}
