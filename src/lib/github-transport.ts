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
  head?: { ref: string };
  base?: { ref: string };
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

export interface RawReview {
  user: { login: string } | null;
  state: string;
  submitted_at: string;
}

export interface RawFullReviewThread {
  id: string;
  isResolved: boolean;
  path: string;
  line: number | null;
  comments: {
    nodes: Array<{
      id: string;
      author: { login: string; __typename?: string } | null;
      body: string;
      createdAt: string;
      url: string;
    }>;
  };
}

export interface RawIssueComment {
  id: number;
  user: { login: string; type?: string } | null;
  body: string;
  created_at: string;
  html_url: string;
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
  listReviews(
    owner: string,
    repo: string,
    prNumber: number,
    signal?: AbortSignal,
  ): AsyncIterable<RawReview>;
  fetchFullReviewThreads(
    owner: string,
    repo: string,
    prNumber: number,
    signal?: AbortSignal,
  ): AsyncIterable<RawFullReviewThread>;
  listIssueComments(
    owner: string,
    repo: string,
    prNumber: number,
    signal?: AbortSignal,
  ): AsyncIterable<RawIssueComment>;
}

// ── Implementation ──────────────────────────────────────────────────────────

const GITHUB_API = "https://api.github.com";
const PER_PAGE = 100;
const GRAPHQL_BATCH_SIZE = 25;

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
    const MAX_RETRIES = 2;
    let lastError: Error | undefined;
    for (let attempt = 0; attempt <= MAX_RETRIES; attempt++) {
      if (attempt > 0) {
        // Exponential backoff: 500ms, 1500ms
        await new Promise((r) => setTimeout(r, 500 * 2 ** (attempt - 1)));
      }
      signal?.throwIfAborted();
      const res = await httpFetch(`${GITHUB_API}/graphql`, {
        method: "POST",
        headers: { ...headers, "Content-Type": "application/json" },
        body: JSON.stringify({ query, variables }),
        signal,
      });
      if (res.ok) {
        return res.json();
      }
      lastError = new Error(`GitHub GraphQL error: ${res.status} ${res.statusText}`);
      // Only retry on server errors (5xx)
      if (res.status < 500) throw lastError;
    }
    throw lastError!;
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

      // Build all batch requests up front
      const batches: number[][] = [];
      for (let i = 0; i < prNumbers.length; i += GRAPHQL_BATCH_SIZE) {
        batches.push(prNumbers.slice(i, i + GRAPHQL_BATCH_SIZE));
      }

      // Fire all batches concurrently
      const results = await Promise.allSettled(
        batches.map((batch) => {
          signal?.throwIfAborted();
          const aliases = batch
            .map(
              (n, idx) =>
                `pr${idx}: pullRequest(number: ${n}) { number additions deletions reviewDecision mergeable commits(last: 1) { nodes { commit { committedDate oid } } } }`,
            )
            .join(" ");
          const query = `query($owner: String!, $repo: String!) { repository(owner: $owner, name: $repo) { ${aliases} } }`;
          return graphql(query, { owner, repo }, signal) as Promise<{
            data?: { repository?: Record<string, unknown> };
          }>;
        }),
      );

      // Yield results in order
      for (let b = 0; b < batches.length; b++) {
        const result = results[b]!;
        if (result.status !== "fulfilled") continue;

        const repoData = result.value.data?.repository;
        if (!repoData) continue;

        const batch = batches[b]!;
        for (let idx = 0; idx < batch.length; idx++) {
          const pr = repoData[`pr${idx}`] as
            | {
                number: number;
                additions?: number;
                deletions?: number;
                reviewDecision?: string;
                mergeable?: string;
                commits: RawPRReviewStatus["commits"];
              }
            | undefined;
          if (pr) {
            yield {
              prNumber: pr.number,
              additions: pr.additions ?? 0,
              deletions: pr.deletions ?? 0,
              reviewDecision: pr.reviewDecision ?? null,
              mergeable: pr.mergeable ?? "UNKNOWN",
              commits: pr.commits,
            } satisfies RawPRReviewStatus;
          }
        }
      }
    },

    async *listReviews(owner, repo, prNumber, signal?) {
      for await (const item of paginateRest(
        `${GITHUB_API}/repos/${owner}/${repo}/pulls/${prNumber}/reviews`,
        signal,
      )) {
        yield item as RawReview;
      }
    },

    async *fetchFullReviewThreads(owner, repo, prNumber, signal?) {
      let cursor: string | null = null;
      while (true) {
        const query = `query($owner: String!, $repo: String!, $number: Int!, $after: String) {
					repository(owner: $owner, name: $repo) {
						pullRequest(number: $number) {
							reviewThreads(first: 100, after: $after) {
								pageInfo { hasNextPage endCursor }
								nodes {
									id
									isResolved
									path
									line
									comments(first: 100) {
										nodes {
											id
											author { login __typename }
											body
											createdAt
											url
										}
									}
								}
							}
						}
					}
				}`;
        const result = (await graphql(
          query,
          { owner, repo, number: prNumber, after: cursor },
          signal,
        )) as {
          data?: {
            repository?: {
              pullRequest?: {
                reviewThreads?: {
                  pageInfo: { hasNextPage: boolean; endCursor: string | null };
                  nodes: RawFullReviewThread[];
                };
              };
            };
          };
        };
        const threads = result.data?.repository?.pullRequest?.reviewThreads;
        if (!threads) break;
        for (const thread of threads.nodes) {
          yield thread;
        }
        if (!threads.pageInfo.hasNextPage) break;
        cursor = threads.pageInfo.endCursor;
      }
    },

    async *listIssueComments(owner, repo, prNumber, signal?) {
      for await (const item of paginateRest(
        `${GITHUB_API}/repos/${owner}/${repo}/issues/${prNumber}/comments`,
        signal,
      )) {
        yield item as RawIssueComment;
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
