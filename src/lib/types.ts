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
	lastCommitDate: string | null;
}

export interface PRDetail extends PR {
	body: string;
}

// ── File categorization ─────────────────────────────────────────────────────

export interface FileChange {
	path: string;
	additions: number;
	deletions: number;
}

export type FileCategory = "code" | "test" | "generated" | "docs" | "config";

export interface FileChangeWithCategory extends FileChange {
	category: FileCategory;
}

export interface CategoryStats {
	additions: number;
	deletions: number;
	files: number;
}

export type StatsByFileCategory = Record<FileCategory, CategoryStats> & {
	total: CategoryStats;
};

export interface FileCategorization {
	files: FileChangeWithCategory[];
	breakdown: StatsByFileCategory;
}
