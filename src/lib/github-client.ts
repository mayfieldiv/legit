/**
 * GitHub API Client — direct HTTP to REST + GraphQL.
 * HTTP transport is injected for testability.
 */

import type { PR, PRDetail } from "./types";

// Re-export domain types for backward compatibility
export type { PR, PRDetail } from "./types";

export type HttpFetch = (
	url: string,
	init?: RequestInit,
) => Promise<Response>;

export interface GitHubClient {
	fetchOpenPRs(repo: string): Promise<PR[]>;
	fetchPR(repo: string, number: number): Promise<PRDetail>;
}

const GITHUB_API = "https://api.github.com";
const PER_PAGE = 100;
const GRAPHQL_BATCH_SIZE = 50;

export function createGitHubClient(
	token: string,
	httpFetch: HttpFetch = globalThis.fetch,
): GitHubClient {
	const headers: Record<string, string> = {
		Authorization: `Bearer ${token}`,
		Accept: "application/vnd.github+json",
		"X-GitHub-Api-Version": "2022-11-28",
	};

	// ── Transport ───────────────────────────────────────────────────────

	async function apiGet(url: string): Promise<unknown> {
		const res = await httpFetch(url, { headers });
		if (!res.ok) {
			throw new Error(`GitHub API error: ${res.status} ${res.statusText}`);
		}
		return res.json();
	}

	async function graphql(
		query: string,
		variables?: Record<string, unknown>,
	): Promise<unknown> {
		const res = await httpFetch(`${GITHUB_API}/graphql`, {
			method: "POST",
			headers: { ...headers, "Content-Type": "application/json" },
			body: JSON.stringify({ query, variables }),
		});
		if (!res.ok) {
			throw new Error(
				`GitHub GraphQL error: ${res.status} ${res.statusText}`,
			);
		}
		return res.json();
	}

	async function paginateRest(baseUrl: string): Promise<unknown[]> {
		const results: unknown[] = [];
		let page = 1;
		while (true) {
			const url = `${baseUrl}${baseUrl.includes("?") ? "&" : "?"}per_page=${PER_PAGE}&page=${page}`;
			const data = (await apiGet(url)) as unknown[];
			results.push(...data);
			if (data.length < PER_PAGE) break;
			page++;
		}
		return results;
	}

	// ── REST parsing ────────────────────────────────────────────────────

	interface RestPR {
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

	function parseRestPR(raw: any): RestPR {
		return {
			number: raw.number,
			title: raw.title,
			author: raw.user.login,
			createdAt: raw.created_at,
			updatedAt: raw.updated_at,
			additions: raw.additions ?? 0,
			deletions: raw.deletions ?? 0,
			isDraft: raw.draft ?? false,
			labels: (raw.labels ?? []).map((l: any) => l.name),
			requestedReviewers: (raw.requested_reviewers ?? []).map(
				(r: any) => r.login,
			),
			assignees: (raw.assignees ?? []).map((a: any) => a.login),
		};
	}

	// ── GraphQL metadata ────────────────────────────────────────────────

	interface GraphQLMeta {
		additions: number;
		deletions: number;
		reviewDecision: string;
		mergeable: string;
		lastCommitDate: string;
	}

	async function fetchGraphQLMeta(
		owner: string,
		repo: string,
		numbers: number[],
	): Promise<Map<number, GraphQLMeta>> {
		const meta = new Map<number, GraphQLMeta>();
		if (numbers.length === 0) return meta;

		for (let i = 0; i < numbers.length; i += GRAPHQL_BATCH_SIZE) {
			const batch = numbers.slice(i, i + GRAPHQL_BATCH_SIZE);
			const aliases = batch
				.map(
					(n, idx) =>
						`pr${idx}: pullRequest(number: ${n}) { number additions deletions reviewDecision mergeable commits(last: 1) { nodes { commit { committedDate } } } }`,
				)
				.join(" ");

			const query = `query($owner: String!, $repo: String!) { repository(owner: $owner, name: $repo) { ${aliases} } }`;
			const result = (await graphql(query, { owner, repo })) as {
				data: { repository: Record<string, any> };
			};

			const repoData = result.data.repository;
			for (let idx = 0; idx < batch.length; idx++) {
				const pr = repoData[`pr${idx}`];
				if (pr) {
					meta.set(pr.number, {
						additions: pr.additions ?? 0,
						deletions: pr.deletions ?? 0,
						reviewDecision: pr.reviewDecision ?? "",
						mergeable: pr.mergeable ?? "UNKNOWN",
						lastCommitDate:
							pr.commits.nodes[0]?.commit?.committedDate ?? "",
					});
				}
			}
		}

		return meta;
	}

	// ── Merge REST + GraphQL ────────────────────────────────────────────

	function mergePR(rest: RestPR, meta?: GraphQLMeta): PR {
		return {
			...rest,
			additions: meta?.additions ?? rest.additions,
			deletions: meta?.deletions ?? rest.deletions,
			reviewDecision: meta?.reviewDecision ?? "",
			mergeable: meta?.mergeable ?? "UNKNOWN",
			lastCommitDate: meta?.lastCommitDate ?? "",
		};
	}

	function parseOwnerRepo(repo: string): [string, string] {
		const parts = repo.split("/");
		if (parts.length !== 2) throw new Error(`Invalid repo format: ${repo}`);
		return [parts[0], parts[1]];
	}

	// ── Public API ──────────────────────────────────────────────────────

	return {
		async fetchOpenPRs(repo: string): Promise<PR[]> {
			const [owner, repoName] = parseOwnerRepo(repo);

			const rawPRs = await paginateRest(
				`${GITHUB_API}/repos/${owner}/${repoName}/pulls?state=open`,
			);
			if (rawPRs.length === 0) return [];

			const restPRs = rawPRs.map(parseRestPR);
			const meta = await fetchGraphQLMeta(
				owner,
				repoName,
				restPRs.map((pr) => pr.number),
			);

			return restPRs.map((pr) => mergePR(pr, meta.get(pr.number)));
		},

		async fetchPR(repo: string, number: number): Promise<PRDetail> {
			const [owner, repoName] = parseOwnerRepo(repo);

			const raw = (await apiGet(
				`${GITHUB_API}/repos/${owner}/${repoName}/pulls/${number}`,
			)) as any;

			const restPR = parseRestPR(raw);
			const meta = await fetchGraphQLMeta(owner, repoName, [number]);

			return {
				...mergePR(restPR, meta.get(number)),
				body: raw.body ?? "",
			};
		},
	};
}
