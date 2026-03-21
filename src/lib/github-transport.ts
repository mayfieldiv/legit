/**
 * GitHub HTTP Transport — owns all GitHub API interaction.
 * Paginated and batched operations yield individual items via AsyncIterable.
 */

// ── Raw API shapes ──────────────────────────────────────────────────────────

export type HttpFetch = (url: string, init?: RequestInit) => Promise<Response>;

export interface RawRestPR {
	number: number;
	title: string;
	user: { login: string } | null;
	created_at: string;
	updated_at: string;
	draft: boolean;
	body?: string;
	additions?: number;
	deletions?: number;
	labels: Array<{ name: string }>;
	requested_reviewers: Array<{ login: string }>;
	assignees: Array<{ login: string }>;
}

export interface RawPRReviewStatus {
	prNumber: number;
	additions: number;
	deletions: number;
	reviewDecision: string | null;
	mergeable: string;
	commits: { nodes: Array<{ commit: { committedDate: string; oid?: string } }> };
}

export interface RawFileChange {
	filename: string;
	additions: number;
	deletions: number;
}

export interface RawCheckRun {
	name: string;
	status: string;
	conclusion: string | null;
}

// ── Transport interface ─────────────────────────────────────────────────────

export interface GitHubTransport {
	listOpenPRs(owner: string, repo: string, signal?: AbortSignal): AsyncIterable<RawRestPR>;
	getPR(owner: string, repo: string, prNumber: number, signal?: AbortSignal): Promise<RawRestPR>;
	listPRFiles(
		owner: string,
		repo: string,
		prNumber: number,
		signal?: AbortSignal,
	): AsyncIterable<RawFileChange>;
	fetchReviewStatus(
		owner: string,
		repo: string,
		prNumbers: number[],
		signal?: AbortSignal,
	): AsyncIterable<RawPRReviewStatus>;
	listCheckRuns(
		owner: string,
		repo: string,
		commitSha: string,
		signal?: AbortSignal,
	): AsyncIterable<RawCheckRun>;
}

// ── Implementation ──────────────────────────────────────────────────────────

const GITHUB_API = "https://api.github.com";
const PER_PAGE = 100;
const GRAPHQL_BATCH_SIZE = 50;

export function createGitHubTransport(
	token: string,
	httpFetch: HttpFetch = globalThis.fetch,
): GitHubTransport {
	const headers: Record<string, string> = {
		Authorization: `Bearer ${token}`,
		Accept: "application/vnd.github+json",
		"X-GitHub-Api-Version": "2022-11-28",
	};

	async function apiGet(url: string, signal?: AbortSignal): Promise<unknown> {
		const res = await httpFetch(url, { headers, signal });
		if (!res.ok) {
			throw new Error(`GitHub API error: ${res.status} ${res.statusText}`);
		}
		return res.json();
	}

	async function graphql(
		query: string,
		variables?: Record<string, unknown>,
		signal?: AbortSignal,
	): Promise<unknown> {
		const res = await httpFetch(`${GITHUB_API}/graphql`, {
			method: "POST",
			headers: { ...headers, "Content-Type": "application/json" },
			body: JSON.stringify({ query, variables }),
			signal,
		});
		if (!res.ok) {
			throw new Error(`GitHub GraphQL error: ${res.status} ${res.statusText}`);
		}
		return res.json();
	}

	async function* paginateRest(baseUrl: string, signal?: AbortSignal) {
		let page = 1;
		while (true) {
			const url = `${baseUrl}${baseUrl.includes("?") ? "&" : "?"}per_page=${PER_PAGE}&page=${page}`;
			const data = (await apiGet(url, signal)) as unknown[];
			for (const item of data) {
				yield item;
			}
			if (data.length < PER_PAGE) break;
			page++;
		}
	}

	return {
		async *listOpenPRs(owner, repo, signal?) {
			for await (const item of paginateRest(
				`${GITHUB_API}/repos/${owner}/${repo}/pulls?state=open`,
				signal,
			)) {
				yield item as RawRestPR;
			}
		},

		async getPR(owner, repo, prNumber, signal?) {
			return (await apiGet(
				`${GITHUB_API}/repos/${owner}/${repo}/pulls/${prNumber}`,
				signal,
			)) as RawRestPR;
		},

		async *listPRFiles(owner, repo, prNumber, signal?) {
			for await (const item of paginateRest(
				`${GITHUB_API}/repos/${owner}/${repo}/pulls/${prNumber}/files`,
				signal,
			)) {
				yield item as RawFileChange;
			}
		},

		async *fetchReviewStatus(owner, repo, prNumbers, signal?) {
			if (prNumbers.length === 0) return;

			for (let i = 0; i < prNumbers.length; i += GRAPHQL_BATCH_SIZE) {
				const batch = prNumbers.slice(i, i + GRAPHQL_BATCH_SIZE);
				const aliases = batch
					.map(
						(n, idx) =>
							`pr${idx}: pullRequest(number: ${n}) { number additions deletions reviewDecision mergeable commits(last: 1) { nodes { commit { committedDate oid } } } }`,
					)
					.join(" ");

				const query = `query($owner: String!, $repo: String!) { repository(owner: $owner, name: $repo) { ${aliases} } }`;
				const result = (await graphql(query, { owner, repo }, signal)) as {
					data?: { repository?: Record<string, any> };
				};

				const repoData = result.data?.repository;
				if (!repoData) continue;

				for (let idx = 0; idx < batch.length; idx++) {
					const pr = repoData[`pr${idx}`];
					if (pr) {
						yield {
							prNumber: pr.number,
							additions: pr.additions ?? 0,
							deletions: pr.deletions ?? 0,
							reviewDecision: pr.reviewDecision ?? null,
							mergeable: pr.mergeable ?? "UNKNOWN",
							commits: pr.commits,
						} as RawPRReviewStatus;
					}
				}
			}
		},

		async *listCheckRuns(owner, repo, commitSha, signal?) {
			let page = 1;
			while (true) {
				const url = `${GITHUB_API}/repos/${owner}/${repo}/commits/${commitSha}/check-runs?per_page=${PER_PAGE}&page=${page}`;
				const data = (await apiGet(url, signal)) as {
					check_runs: RawCheckRun[];
				};
				for (const item of data.check_runs) {
					yield item;
				}
				if (data.check_runs.length < PER_PAGE) break;
				page++;
			}
		},
	};
}
