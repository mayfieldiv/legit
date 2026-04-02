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
	repoSlug?: string;
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
	headCommitSha: string | null;
	/** Source branch name (e.g. "feature/my-branch"). */
	headRef: string;
	/** Target branch name (e.g. "main"). */
	baseRef: string;
	/** Unresolved review-thread counts. Populated by the background thread-count loader. */
	comments?: CommentCounts;
	/** True while the background fetch of thread counts is in-flight for this PR. */
	threadsLoading?: boolean;
}

export interface PRDetail extends PR {
	body: string;
}

// ── Check runs ──────────────────────────────────────────────────────────────

export type CheckConclusion =
	| "success"
	| "failure"
	| "neutral"
	| "cancelled"
	| "skipped"
	| "stale"
	| "timed_out"
	| "action_required";

export interface CheckRun {
	name: string;
	status: "completed" | "in_progress" | "queued";
	conclusion: CheckConclusion | null;
}

// ── Reviews ─────────────────────────────────────────────────────────────────

export type ReviewState = "APPROVED" | "CHANGES_REQUESTED" | "COMMENTED" | "DISMISSED";

export interface Review {
	user: string;
	state: ReviewState;
}

// ── Comment counts ──────────────────────────────────────────────────────────

export interface CommentCounts {
	total: number;
	unresolved: number;
	unresolvedHuman: number;
	unresolvedBot: number;
}

// ── Review threads (full) ────────────────────────────────────────────────────

export interface ReviewComment {
	id: string;
	author: string;
	body: string;
	createdAt: string;
	url: string;
	isBot: boolean;
}

export interface FullReviewThread {
	id: string;
	isResolved: boolean;
	path: string;
	line: number | null;
	comments: ReviewComment[];
}

// ── Issue comments ──────────────────────────────────────────────────────────

export interface IssueComment {
	id: number;
	author: string;
	body: string;
	createdAt: string;
	url: string;
	isBot: boolean;
}

// ── PR Summary ──────────────────────────────────────────────────────────────

export interface PRSummary extends PRDetail {
	checks: CheckRun[];
	reviews: Review[];
	comments: CommentCounts;
	files: FileCategorization;
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
