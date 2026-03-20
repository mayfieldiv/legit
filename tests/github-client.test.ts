import { describe, test, expect } from "bun:test";
import { createGitHubClient } from "../src/lib/github-client";
import { createMockFetch, SAMPLE_REST_PR, SAMPLE_GQL_META, makeGraphQLResponse } from "./helpers";

// ── Tests ───────────────────────────────────────────────────────────────────

const GRAPHQL_RESPONSE = makeGraphQLResponse([
	{ ...SAMPLE_GQL_META, reviewDecision: "REVIEW_REQUIRED" },
]);

describe("GitHubClient", () => {
	describe("fetchOpenPRs", () => {
		test("fetches and merges REST + GraphQL data into typed PR objects", async () => {
			const { fetch } = createMockFetch([
				{
					url: "https://api.github.com/repos/acme/widgets/pulls?state=open&per_page=100&page=1",
					response: { status: 200, body: [SAMPLE_REST_PR] },
				},
				{
					url: "https://api.github.com/graphql",
					method: "POST",
					response: { status: 200, body: GRAPHQL_RESPONSE },
				},
			]);

			const client = createGitHubClient("fake-token", fetch);
			const prs = await client.fetchOpenPRs("acme/widgets");

			expect(prs).toHaveLength(1);
			const pr = prs[0]!;
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
				...SAMPLE_REST_PR,
				number: i + 1,
				title: `PR ${i + 1}`,
			}));
			const page2 = [{ ...SAMPLE_REST_PR, number: 101, title: "PR 101" }];

			const gqlMetas = Array.from({ length: 101 }, (_, i) => ({
				...SAMPLE_GQL_META,
				number: i + 1,
				reviewDecision: "REVIEW_REQUIRED",
			}));

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
						body: makeGraphQLResponse(gqlMetas),
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
			const authHeader = (calls[0]!.init?.headers as Record<string, string>)?.[
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

		test("handles deleted user (null user) gracefully", async () => {
			const ghostPR = {
				...SAMPLE_REST_PR,
				user: null,
			};
			const { fetch } = createMockFetch([
				{
					url: /pulls.*state=open/,
					response: { status: 200, body: [ghostPR] },
				},
				{
					url: "https://api.github.com/graphql",
					method: "POST",
					response: { status: 200, body: GRAPHQL_RESPONSE },
				},
			]);

			const client = createGitHubClient("fake-token", fetch);
			const prs = await client.fetchOpenPRs("acme/widgets");
			expect(prs).toHaveLength(1);
			expect(prs[0]!.author).toBe("ghost");
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

		test("reports page and metadata batch progress while loading open PRs", async () => {
			const page1 = Array.from({ length: 100 }, (_, i) => ({
				...SAMPLE_REST_PR,
				number: i + 1,
				title: `PR ${i + 1}`,
			}));
			const page2 = [{ ...SAMPLE_REST_PR, number: 101, title: "PR 101" }];
			const gqlBatch1 = Array.from({ length: 50 }, (_, i) => ({
				...SAMPLE_GQL_META,
				number: i + 1,
			}));
			const gqlBatch2 = Array.from({ length: 50 }, (_, i) => ({
				...SAMPLE_GQL_META,
				number: i + 51,
			}));
			const gqlBatch3 = [{ ...SAMPLE_GQL_META, number: 101 }];

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
					response: { status: 200, body: makeGraphQLResponse(gqlBatch1) },
				},
				{
					url: "https://api.github.com/graphql",
					method: "POST",
					response: { status: 200, body: makeGraphQLResponse(gqlBatch2) },
				},
				{
					url: "https://api.github.com/graphql",
					method: "POST",
					response: { status: 200, body: makeGraphQLResponse(gqlBatch3) },
				},
			]);

			const progress: string[] = [];
			const client = createGitHubClient("fake-token", fetch);
			await client.fetchOpenPRs("acme/widgets", (message) => {
				progress.push(message);
			});

			expect(progress).toEqual([
				"Loading pull requests… page 1",
				"Loading pull requests… page 2",
				"Loading PR metadata… batch 1/3",
				"Loading PR metadata… batch 2/3",
				"Loading PR metadata… batch 3/3",
			]);
		});
	});

	describe("fetchPR", () => {
		test("fetches a single PR with full detail", async () => {
			const detailResponse = {
				...SAMPLE_REST_PR,
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
					response: { status: 200, body: GRAPHQL_RESPONSE },
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
