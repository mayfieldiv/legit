import { describe, test, expect } from "bun:test";
import {
	createGitHubClient,
	parseRestPR,
	parseReviewStatus,
	parseFileChange,
	mergePR,
	type RawRestPR,
	type RawPRReviewStatus,
	type RawFileChange,
} from "../src/lib/github-client";
import {
	createMockFetch,
	SAMPLE_REST_PR,
	SAMPLE_GQL_META,
	makeGraphQLResponse,
	SAMPLE_FILE,
	makeSampleFile,
} from "./helpers";

// ── Parsing tests ───────────────────────────────────────────────────────────

describe("parsing", () => {
	describe("parseRestPR", () => {
		test("maps snake_case REST fields to camelCase", () => {
			const raw: RawRestPR = {
				number: 42,
				title: "Fix bug",
				user: { login: "alice" },
				created_at: "2026-03-01T00:00:00Z",
				updated_at: "2026-03-15T00:00:00Z",
				draft: false,
				labels: [{ name: "bug" }],
				requested_reviewers: [{ login: "bob" }],
				assignees: [{ login: "alice" }],
			};
			const parsed = parseRestPR(raw);
			expect(parsed).toEqual({
				number: 42,
				title: "Fix bug",
				author: "alice",
				createdAt: "2026-03-01T00:00:00Z",
				updatedAt: "2026-03-15T00:00:00Z",
				additions: 0,
				deletions: 0,
				isDraft: false,
				labels: ["bug"],
				requestedReviewers: ["bob"],
				assignees: ["alice"],
			});
		});

		test("null user maps to ghost", () => {
			const raw: RawRestPR = {
				number: 1,
				title: "X",
				user: null,
				created_at: "",
				updated_at: "",
				draft: false,
				labels: [],
				requested_reviewers: [],
				assignees: [],
			};
			expect(parseRestPR(raw).author).toBe("ghost");
		});

		test("defaults additions/deletions to 0 when absent", () => {
			const raw: RawRestPR = {
				number: 1,
				title: "X",
				user: { login: "a" },
				created_at: "",
				updated_at: "",
				draft: false,
				labels: [],
				requested_reviewers: [],
				assignees: [],
			};
			const parsed = parseRestPR(raw);
			expect(parsed.additions).toBe(0);
			expect(parsed.deletions).toBe(0);
		});

		test("includes additions/deletions from detail endpoint", () => {
			const raw: RawRestPR = {
				number: 1,
				title: "X",
				user: { login: "a" },
				created_at: "",
				updated_at: "",
				draft: false,
				additions: 30,
				deletions: 10,
				labels: [],
				requested_reviewers: [],
				assignees: [],
			};
			const parsed = parseRestPR(raw);
			expect(parsed.additions).toBe(30);
			expect(parsed.deletions).toBe(10);
		});
	});

	describe("parseReviewStatus", () => {
		test("extracts last commit date from nested structure", () => {
			const raw: RawPRReviewStatus = {
				prNumber: 42,
				additions: 50,
				deletions: 10,
				reviewDecision: "APPROVED",
				mergeable: "MERGEABLE",
				commits: { nodes: [{ commit: { committedDate: "2026-03-14T00:00:00Z" } }] },
			};
			const parsed = parseReviewStatus(raw);
			expect(parsed).toEqual({
				additions: 50,
				deletions: 10,
				reviewDecision: "APPROVED",
				mergeable: "MERGEABLE",
				lastCommitDate: "2026-03-14T00:00:00Z",
			});
		});

		test("null reviewDecision maps to empty string", () => {
			const raw: RawPRReviewStatus = {
				prNumber: 1,
				additions: 0,
				deletions: 0,
				reviewDecision: null,
				mergeable: "UNKNOWN",
				commits: { nodes: [] },
			};
			expect(parseReviewStatus(raw).reviewDecision).toBe("");
		});

		test("empty commits array maps to empty lastCommitDate", () => {
			const raw: RawPRReviewStatus = {
				prNumber: 1,
				additions: 0,
				deletions: 0,
				reviewDecision: null,
				mergeable: "UNKNOWN",
				commits: { nodes: [] },
			};
			expect(parseReviewStatus(raw).lastCommitDate).toBe("");
		});
	});

	describe("parseFileChange", () => {
		test("maps filename to path", () => {
			const raw: RawFileChange = { filename: "src/lib/foo.ts", additions: 25, deletions: 5 };
			expect(parseFileChange(raw)).toEqual({
				path: "src/lib/foo.ts",
				additions: 25,
				deletions: 5,
			});
		});
	});

	describe("mergePR", () => {
		test("prefers review status additions/deletions over REST", () => {
			const rest = parseRestPR({
				number: 1,
				title: "X",
				user: { login: "a" },
				created_at: "",
				updated_at: "",
				draft: false,
				additions: 5,
				deletions: 2,
				labels: [],
				requested_reviewers: [],
				assignees: [],
			});
			const status = {
				additions: 50,
				deletions: 10,
				reviewDecision: "APPROVED",
				mergeable: "MERGEABLE",
				lastCommitDate: "2026-03-14T00:00:00Z",
			};
			const merged = mergePR(rest, status);
			expect(merged.additions).toBe(50);
			expect(merged.deletions).toBe(10);
		});

		test("falls back to REST values when status is undefined", () => {
			const rest = parseRestPR({
				number: 1,
				title: "X",
				user: { login: "a" },
				created_at: "",
				updated_at: "",
				draft: false,
				additions: 5,
				deletions: 2,
				labels: [],
				requested_reviewers: [],
				assignees: [],
			});
			const merged = mergePR(rest);
			expect(merged.additions).toBe(5);
			expect(merged.deletions).toBe(2);
			expect(merged.reviewDecision).toBe("");
			expect(merged.mergeable).toBe("UNKNOWN");
			expect(merged.lastCommitDate).toBe("");
		});
	});
});

// ── Integration tests ────────────────────────────────────────────────────────

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

			const gqlBatch1 = Array.from({ length: 50 }, (_, i) => ({
				...SAMPLE_GQL_META,
				number: i + 1,
				reviewDecision: "REVIEW_REQUIRED",
			}));
			const gqlBatch2 = Array.from({ length: 50 }, (_, i) => ({
				...SAMPLE_GQL_META,
				number: i + 51,
				reviewDecision: "REVIEW_REQUIRED",
			}));
			const gqlBatch3 = [
				{ ...SAMPLE_GQL_META, number: 101, reviewDecision: "REVIEW_REQUIRED" as const },
			];

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
			const prs = await client.fetchOpenPRs("acme/widgets", (message) => {
				progress.push(message);
			});

			expect(progress).toEqual([
				"Loading pull requests… page 1",
				"Loading pull requests… page 2",
				"Loading PR metadata… batch 1/3",
				"Loading PR metadata… batch 2/3",
				"Loading PR metadata… batch 3/3",
			]);

			// Verify all 101 PRs received correct GraphQL metadata
			expect(prs).toHaveLength(101);
			expect(prs[0]!.reviewDecision).toBe("APPROVED");
			expect(prs[50]!.reviewDecision).toBe("APPROVED");
			expect(prs[100]!.reviewDecision).toBe("APPROVED");
		});
	});

	describe("fetchFiles", () => {
		test("fetches and maps file list for a PR", async () => {
			const { fetch } = createMockFetch([
				{
					url: "https://api.github.com/repos/acme/widgets/pulls/42/files?per_page=100&page=1",
					response: {
						status: 200,
						body: [
							SAMPLE_FILE,
							{
								...SAMPLE_FILE,
								filename: "tests/bar.test.ts",
								additions: 15,
								deletions: 2,
							},
						],
					},
				},
			]);

			const client = createGitHubClient("fake-token", fetch);
			const files = await client.fetchFiles("acme/widgets", 42);

			expect(files).toHaveLength(2);
			expect(files[0]).toEqual({ path: "src/lib/foo.ts", additions: 25, deletions: 5 });
			expect(files[1]).toEqual({ path: "tests/bar.test.ts", additions: 15, deletions: 2 });
		});

		test("paginates when response has 100 items", async () => {
			const page1 = Array.from({ length: 100 }, (_, i) => makeSampleFile(`file${i}.ts`));
			const page2 = [makeSampleFile("file100.ts")];

			const { fetch } = createMockFetch([
				{
					url: "https://api.github.com/repos/acme/widgets/pulls/42/files?per_page=100&page=1",
					response: { status: 200, body: page1 },
				},
				{
					url: "https://api.github.com/repos/acme/widgets/pulls/42/files?per_page=100&page=2",
					response: { status: 200, body: page2 },
				},
			]);

			const client = createGitHubClient("fake-token", fetch);
			const files = await client.fetchFiles("acme/widgets", 42);
			expect(files).toHaveLength(101);
		});

		test("returns empty array for PR with no files", async () => {
			const { fetch } = createMockFetch([
				{
					url: /\/files/,
					response: { status: 200, body: [] },
				},
			]);

			const client = createGitHubClient("fake-token", fetch);
			const files = await client.fetchFiles("acme/widgets", 42);
			expect(files).toEqual([]);
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
