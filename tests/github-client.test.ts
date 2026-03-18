import { describe, test, expect } from "bun:test";
import {
	createGitHubClient,
	type GitHubClient,
	type HttpFetch,
	type PR,
} from "../src/lib/github-client";

// ── Mock HTTP transport ─────────────────────────────────────────────────────

interface MockRoute {
	url: string | RegExp;
	method?: string;
	body?: unknown;
	response: { status: number; body: unknown };
}

function createMockFetch(routes: MockRoute[]): {
	fetch: HttpFetch;
	calls: Array<{ url: string; init?: RequestInit }>;
} {
	const calls: Array<{ url: string; init?: RequestInit }> = [];

	const fetch: HttpFetch = async (url, init) => {
		calls.push({ url, init });
		const method = init?.method ?? "GET";

		for (const route of routes) {
			const urlMatch =
				typeof route.url === "string" ? url === route.url : route.url.test(url);
			const methodMatch = !route.method || route.method === method;

			if (urlMatch && methodMatch) {
				return new Response(JSON.stringify(route.response.body), {
					status: route.response.status,
					headers: { "Content-Type": "application/json" },
				});
			}
		}

		return new Response(JSON.stringify({ message: "Not Found" }), {
			status: 404,
		});
	};

	return { fetch, calls };
}

// ── Test data ───────────────────────────────────────────────────────────────

// REST list endpoint does NOT include additions/deletions
const SAMPLE_PR_REST = {
	number: 42,
	title: "Fix the thing",
	user: { login: "alice", type: "User" },
	created_at: "2026-03-01T00:00:00Z",
	updated_at: "2026-03-15T00:00:00Z",
	draft: false,
	labels: [{ name: "bug" }],
	requested_reviewers: [{ login: "bob" }],
	assignees: [{ login: "alice" }],
};

// GraphQL provides additions/deletions along with other metadata
const SAMPLE_GRAPHQL_RESPONSE = {
	data: {
		repository: {
			pr0: {
				number: 42,
				additions: 50,
				deletions: 10,
				reviewDecision: "REVIEW_REQUIRED",
				mergeable: "MERGEABLE",
				commits: {
					nodes: [
						{ commit: { committedDate: "2026-03-14T00:00:00Z" } },
					],
				},
			},
		},
	},
};

// ── Tests ───────────────────────────────────────────────────────────────────

describe("GitHubClient", () => {
	describe("fetchOpenPRs", () => {
		test("fetches and merges REST + GraphQL data into typed PR objects", async () => {
			const { fetch } = createMockFetch([
				{
					url: "https://api.github.com/repos/acme/widgets/pulls?state=open&per_page=100&page=1",
					response: { status: 200, body: [SAMPLE_PR_REST] },
				},
				{
					url: "https://api.github.com/graphql",
					method: "POST",
					response: { status: 200, body: SAMPLE_GRAPHQL_RESPONSE },
				},
			]);

			const client = createGitHubClient("fake-token", fetch);
			const prs = await client.fetchOpenPRs("acme/widgets");

			expect(prs).toHaveLength(1);
			const pr = prs[0];
			expect(pr.number).toBe(42);
			expect(pr.title).toBe("Fix the thing");
			expect(pr.author).toBe("alice");
			expect(pr.createdAt).toBe("2026-03-01T00:00:00Z");
			expect(pr.updatedAt).toBe("2026-03-15T00:00:00Z");
			expect(pr.additions).toBe(50);
			expect(pr.deletions).toBe(10);
			expect(pr.isDraft).toBe(false);
			expect(pr.labels).toEqual(["bug"]);
			expect(pr.requestedReviewers).toEqual(["bob"]);
			expect(pr.assignees).toEqual(["alice"]);
			// From GraphQL
			expect(pr.reviewDecision).toBe("REVIEW_REQUIRED");
			expect(pr.mergeable).toBe("MERGEABLE");
			expect(pr.lastCommitDate).toBe("2026-03-14T00:00:00Z");
		});

		test("paginates when response has 100 items", async () => {
			const page1 = Array.from({ length: 100 }, (_, i) => ({
				...SAMPLE_PR_REST,
				number: i + 1,
				title: `PR ${i + 1}`,
			}));
			const page2 = [{ ...SAMPLE_PR_REST, number: 101, title: "PR 101" }];

			// GraphQL for 101 PRs
			const gqlData: Record<string, unknown> = {};
			for (let i = 0; i < 101; i++) {
				gqlData[`pr${i}`] = {
					number: i + 1,
					additions: 10,
					deletions: 5,
					reviewDecision: "REVIEW_REQUIRED",
					mergeable: "MERGEABLE",
					commits: {
						nodes: [
							{ commit: { committedDate: "2026-03-14T00:00:00Z" } },
						],
					},
				};
			}

			const { fetch } = createMockFetch([
				{
					url: "https://api.github.com/repos/acme/widgets/pulls?state=open&per_page=100&page=1",
					response: { status: 200, body: page1 },
				},
				{
					url: "https://api.github.com/repos/acme/widgets/pulls?state=open&per_page=100&page=2",
					response: { status: 200, body: page2 },
				},
				{
					url: "https://api.github.com/graphql",
					method: "POST",
					response: {
						status: 200,
						body: { data: { repository: gqlData } },
					},
				},
			]);

			const client = createGitHubClient("fake-token", fetch);
			const prs = await client.fetchOpenPRs("acme/widgets");
			expect(prs).toHaveLength(101);
		});

		test("sends correct Authorization header", async () => {
			const { fetch, calls } = createMockFetch([
				{
					url: /pulls/,
					response: { status: 200, body: [] },
				},
			]);

			const client = createGitHubClient("my-secret-token", fetch);
			await client.fetchOpenPRs("acme/widgets");

			expect(calls.length).toBeGreaterThan(0);
			const authHeader = (calls[0].init?.headers as Record<string, string>)?.[
				"Authorization"
			];
			expect(authHeader).toBe("Bearer my-secret-token");
		});

		test("returns empty array for repo with no open PRs", async () => {
			const { fetch } = createMockFetch([
				{
					url: /pulls/,
					response: { status: 200, body: [] },
				},
			]);

			const client = createGitHubClient("fake-token", fetch);
			const prs = await client.fetchOpenPRs("acme/widgets");
			expect(prs).toEqual([]);
		});

		test("throws on API error", async () => {
			const { fetch } = createMockFetch([
				{
					url: /pulls/,
					response: {
						status: 403,
						body: { message: "API rate limit exceeded" },
					},
				},
			]);

			const client = createGitHubClient("fake-token", fetch);
			expect(client.fetchOpenPRs("acme/widgets")).rejects.toThrow(/403/);
		});
	});

	describe("fetchPR", () => {
		test("fetches a single PR with full detail", async () => {
			const detailResponse = {
				...SAMPLE_PR_REST,
				body: "## Description\n\nFixes the thing.",
			};

			const { fetch } = createMockFetch([
				{
					url: "https://api.github.com/repos/acme/widgets/pulls/42",
					response: { status: 200, body: detailResponse },
				},
				{
					url: "https://api.github.com/graphql",
					method: "POST",
					response: { status: 200, body: SAMPLE_GRAPHQL_RESPONSE },
				},
			]);

			const client = createGitHubClient("fake-token", fetch);
			const pr = await client.fetchPR("acme/widgets", 42);

			expect(pr.number).toBe(42);
			expect(pr.title).toBe("Fix the thing");
			expect(pr.body).toBe("## Description\n\nFixes the thing.");
			expect(pr.reviewDecision).toBe("REVIEW_REQUIRED");
		});

		test("throws on 404", async () => {
			const { fetch } = createMockFetch([
				{
					url: /pulls\/999/,
					response: { status: 404, body: { message: "Not Found" } },
				},
			]);

			const client = createGitHubClient("fake-token", fetch);
			expect(client.fetchPR("acme/widgets", 999)).rejects.toThrow(/404/);
		});
	});
});
