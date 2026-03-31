/**
 * GitHub Client — pure parsing + orchestration over a GitHubTransport.
 * Parsing functions are exported for direct testing.
 */

import type {
	RawRestPR,
	RawPRReviewStatus,
	RawFileChange,
	GitHubTransport,
} from "./github-transport";
import type {
	PR,
	PRDetail,
	FileChange,
	CheckRun,
	Review,
	CommentCounts,
	FullReviewThread,
	ReviewComment,
	IssueComment,
} from "./types";

// Re-export transport types that callers may need
export type {
	HttpFetch,
	GitHubTransport,
	RawRestPR,
	RawPRReviewStatus,
	RawFileChange,
	RawCheckRun,
} from "./github-transport";

// Re-export domain types for backward compatibility
export type {
	PR,
	PRDetail,
	FileChange,
	CheckRun,
	Review,
	CommentCounts,
	FullReviewThread,
	ReviewComment,
	IssueComment,
} from "./types";

// ── Intermediate parsed types ───────────────────────────────────────────────

export interface RestPR {
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
}

export interface ReviewStatus {
	additions: number;
	deletions: number;
	reviewDecision: string;
	mergeable: string;
	lastCommitDate: string | null;
	headCommitSha: string | null;
}

// ── Pure parsing functions ──────────────────────────────────────────────────

export function parseRestPR(raw: RawRestPR): RestPR {
	return {
		number: raw.number,
		title: raw.title,
		author: raw.user?.login ?? "ghost",
		createdAt: raw.created_at,
		updatedAt: raw.updated_at,
		additions: raw.additions ?? 0,
		deletions: raw.deletions ?? 0,
		isDraft: raw.draft ?? false,
		labels: (raw.labels ?? []).map((l) => l.name),
		requestedReviewers: (raw.requested_reviewers ?? []).map((r) => r.login),
		assignees: (raw.assignees ?? []).map((a) => a.login),
	};
}

export function parseReviewStatus(raw: RawPRReviewStatus): ReviewStatus {
	const commitNode = raw.commits.nodes[0]?.commit;
	return {
		additions: raw.additions ?? 0,
		deletions: raw.deletions ?? 0,
		reviewDecision: raw.reviewDecision ?? "",
		mergeable: raw.mergeable ?? "UNKNOWN",
		lastCommitDate: commitNode?.committedDate ?? null,
		headCommitSha: commitNode?.oid ?? null,
	};
}

export function parseFileChange(raw: RawFileChange): FileChange {
	return {
		path: raw.filename,
		additions: raw.additions ?? 0,
		deletions: raw.deletions ?? 0,
	};
}

export function mergePR(rest: RestPR, status?: ReviewStatus): PR {
	return {
		...rest,
		additions: status?.additions ?? rest.additions,
		deletions: status?.deletions ?? rest.deletions,
		reviewDecision: status?.reviewDecision ?? "",
		mergeable: status?.mergeable ?? "UNKNOWN",
		lastCommitDate: status?.lastCommitDate ?? null,
		headCommitSha: status?.headCommitSha ?? null,
	};
}

// ── Client interface ────────────────────────────────────────────────────────

export interface GitHubClient {
	fetchOpenPRs(repo: string, signal?: AbortSignal): AsyncIterable<PR[]>;
	fetchPR(repo: string, prNumber: number, signal?: AbortSignal): Promise<PRDetail>;
	fetchFiles(repo: string, prNumber: number, signal?: AbortSignal): AsyncIterable<FileChange[]>;
	fetchCheckRuns(repo: string, commitSha: string, signal?: AbortSignal): Promise<CheckRun[]>;
	fetchReviews(repo: string, prNumber: number, signal?: AbortSignal): Promise<Review[]>;
	fetchReviewComments(
		repo: string,
		prNumber: number,
		botLogins: string[],
		signal?: AbortSignal,
	): Promise<CommentCounts>;
	fetchFullReviewThreads(
		repo: string,
		prNumber: number,
		botLogins: string[],
		signal?: AbortSignal,
	): Promise<FullReviewThread[]>;
	fetchIssueComments(
		repo: string,
		prNumber: number,
		botLogins: string[],
		signal?: AbortSignal,
	): Promise<IssueComment[]>;
}

function parseOwnerRepo(repo: string): [string, string] {
	const parts = repo.split("/");
	if (parts.length !== 2 || !parts[0] || !parts[1])
		throw new Error(`Invalid repo format: ${repo}`);
	return [parts[0], parts[1]];
}

export function createGitHubClient(transport: GitHubTransport): GitHubClient {
	return {
		async *fetchOpenPRs(repo: string, signal?: AbortSignal) {
			const [owner, repoName] = parseOwnerRepo(repo);

			// Phase 1: yield PRs as they stream in from REST (no review status yet)
			const restPRs: RestPR[] = [];
			const prs: PR[] = [];

			for await (const raw of transport.listOpenPRs(owner, repoName, signal)) {
				const rest = parseRestPR(raw);
				restPRs.push(rest);
				prs.push(mergePR(rest));
				yield [...prs];
			}

			if (prs.length === 0) return;

			// Phase 2: enrich with review status as it streams in
			for await (const rawStatus of transport.fetchReviewStatus(
				owner,
				repoName,
				restPRs.map((r) => r.number),
				signal,
			)) {
				const status = parseReviewStatus(rawStatus);
				const idx = restPRs.findIndex((r) => r.number === rawStatus.prNumber);
				if (idx !== -1) {
					prs[idx] = mergePR(restPRs[idx]!, status);
					yield [...prs];
				}
			}
		},

		async fetchPR(repo: string, prNumber: number, signal?: AbortSignal): Promise<PRDetail> {
			const [owner, repoName] = parseOwnerRepo(repo);

			const raw = await transport.getPR(owner, repoName, prNumber, signal);
			const rest = parseRestPR(raw);

			let status: ReviewStatus | undefined;
			for await (const rawStatus of transport.fetchReviewStatus(
				owner,
				repoName,
				[prNumber],
				signal,
			)) {
				status = parseReviewStatus(rawStatus);
			}

			return {
				...mergePR(rest, status),
				body: raw.body ?? "",
			};
		},

		async *fetchFiles(repo: string, prNumber: number, signal?: AbortSignal) {
			const [owner, repoName] = parseOwnerRepo(repo);
			const files: FileChange[] = [];
			for await (const raw of transport.listPRFiles(owner, repoName, prNumber, signal)) {
				files.push(parseFileChange(raw));
				yield [...files];
			}
		},

		async fetchReviews(
			repo: string,
			prNumber: number,
			signal?: AbortSignal,
		): Promise<Review[]> {
			const [owner, repoName] = parseOwnerRepo(repo);
			const rawReviews: Array<{ user: string; state: string; submitted_at: string }> = [];
			for await (const raw of transport.listReviews(owner, repoName, prNumber, signal)) {
				if (raw.state === "PENDING") continue;
				const login = raw.user?.login;
				if (!login) continue;
				rawReviews.push({
					user: login,
					state: raw.state,
					submitted_at: raw.submitted_at,
				});
			}
			const byUser = new Map<string, { state: string; submitted_at: string }>();
			for (const r of rawReviews) {
				const existing = byUser.get(r.user);
				if (!existing || r.submitted_at > existing.submitted_at) {
					byUser.set(r.user, r);
				}
			}
			return Array.from(byUser.entries()).map(([user, r]) => ({
				user,
				state: r.state as Review["state"],
			}));
		},

		async fetchReviewComments(
			repo: string,
			prNumber: number,
			botLogins: string[],
			signal?: AbortSignal,
		): Promise<CommentCounts> {
			const [owner, repoName] = parseOwnerRepo(repo);
			const botSet = new Set(botLogins);
			let total = 0;
			let unresolved = 0;
			let unresolvedHuman = 0;
			let unresolvedBot = 0;

			for await (const thread of transport.fetchReviewThreads(
				owner,
				repoName,
				prNumber,
				signal,
			)) {
				total++;
				if (!thread.isResolved) {
					unresolved++;
					const authorNode = thread.comments.nodes[0]?.author;
					const isBot =
						authorNode != null &&
						(authorNode.__typename === "Bot" ||
							authorNode.login.endsWith("[bot]") ||
							botSet.has(authorNode.login));
					if (isBot) {
						unresolvedBot++;
					} else {
						unresolvedHuman++;
					}
				}
			}

			return { total, unresolved, unresolvedHuman, unresolvedBot };
		},

		async fetchFullReviewThreads(
			repo: string,
			prNumber: number,
			botLogins: string[],
			signal?: AbortSignal,
		): Promise<FullReviewThread[]> {
			const [owner, repoName] = parseOwnerRepo(repo);
			const botSet = new Set(botLogins);
			const threads: FullReviewThread[] = [];

			for await (const raw of transport.fetchFullReviewThreads(
				owner,
				repoName,
				prNumber,
				signal,
			)) {
				const comments: ReviewComment[] = raw.comments.nodes.map((c) => {
					const login = c.author?.login ?? "ghost";
					const isBot =
						c.author != null &&
						(c.author.__typename === "Bot" ||
							login.endsWith("[bot]") ||
							botSet.has(login));
					return {
						id: c.id,
						author: login,
						body: c.body,
						createdAt: c.createdAt,
						url: c.url,
						isBot,
					};
				});
				threads.push({
					id: raw.id,
					isResolved: raw.isResolved,
					path: raw.path,
					line: raw.line,
					comments,
				});
			}

			return threads;
		},

		async fetchIssueComments(
			repo: string,
			prNumber: number,
			botLogins: string[],
			signal?: AbortSignal,
		): Promise<IssueComment[]> {
			const [owner, repoName] = parseOwnerRepo(repo);
			const botSet = new Set(botLogins);
			const comments: IssueComment[] = [];

			for await (const raw of transport.listIssueComments(
				owner,
				repoName,
				prNumber,
				signal,
			)) {
				const login = raw.user?.login ?? "ghost";
				const isBot =
					raw.user != null &&
					(raw.user.type === "Bot" || login.endsWith("[bot]") || botSet.has(login));
				comments.push({
					id: raw.id,
					author: login,
					body: raw.body,
					createdAt: raw.created_at,
					url: raw.html_url,
					isBot,
				});
			}

			return comments;
		},

		async fetchCheckRuns(
			repo: string,
			commitSha: string,
			signal?: AbortSignal,
		): Promise<CheckRun[]> {
			const [owner, repoName] = parseOwnerRepo(repo);
			const checks: CheckRun[] = [];
			for await (const raw of transport.listCheckRuns(owner, repoName, commitSha, signal)) {
				checks.push({
					name: raw.name,
					status: raw.status as CheckRun["status"],
					conclusion: raw.conclusion as CheckRun["conclusion"],
				});
			}
			return checks;
		},
	};
}
