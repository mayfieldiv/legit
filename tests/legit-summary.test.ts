import { describe, test, expect, afterAll } from "bun:test";
import {
	cleanupTmpDirs,
	createTestLegit,
	createMockFetch,
	makeSampleRestPR,
	makeGraphQLResponse,
	SAMPLE_GQL_META,
} from "./helpers";

afterAll(cleanupTmpDirs);

describe("Legit.fetchPRSummary", () => {
	test("composes PR detail, checks, reviews, comments, and files", async () => {
		const { fetch } = createMockFetch([
			// fetchPR: REST detail
			{
				url: /\/pulls\/42$/,
				response: {
					status: 200,
					body: { ...makeSampleRestPR(42), body: "PR body" },
				},
			},
			// fetchPR: GraphQL review status
			{
				url: /\/graphql/,
				method: "POST",
				response: {
					status: 200,
					body: makeGraphQLResponse([{ ...SAMPLE_GQL_META, number: 42 }]),
				},
			},
			// fetchCheckRuns
			{
				url: /\/check-runs/,
				response: {
					status: 200,
					body: {
						total_count: 1,
						check_runs: [{ name: "build", status: "completed", conclusion: "success" }],
					},
				},
			},
			// fetchReviews
			{
				url: /\/reviews/,
				response: {
					status: 200,
					body: [
						{
							user: { login: "bob" },
							state: "APPROVED",
							submitted_at: "2026-03-01T00:00:00Z",
						},
					],
				},
			},
			// fetchReviewComments (GraphQL)
			{
				url: /\/graphql/,
				method: "POST",
				response: {
					status: 200,
					body: {
						data: {
							repository: {
								pullRequest: {
									reviewThreads: {
										pageInfo: { hasNextPage: false, endCursor: null },
										nodes: [
											{
												isResolved: false,
												comments: {
													nodes: [{ author: { login: "alice" } }],
												},
											},
										],
									},
								},
							},
						},
					},
				},
			},
			// fetchFiles
			{
				url: /\/files/,
				response: {
					status: 200,
					body: [
						{
							filename: "src/app.ts",
							additions: 10,
							deletions: 5,
							changes: 15,
							status: "modified",
						},
					],
				},
			},
		]);

		const app = createTestLegit({ httpFetch: fetch });
		const summary = await app.fetchPRSummary(app.repoSlug, 42);

		expect(summary.number).toBe(42);
		expect(summary.body).toBe("PR body");
		expect(summary.checks).toEqual([
			{ name: "build", status: "completed", conclusion: "success" },
		]);
		expect(summary.reviews).toEqual([{ user: "bob", state: "APPROVED" }]);
		expect(summary.comments.total).toBe(1);
		expect(summary.comments.unresolved).toBe(1);
		expect(summary.comments.unresolvedHuman).toBe(1);
		expect(summary.files.files).toHaveLength(1);
		expect(summary.files.files[0]!.category).toBe("code");
	});

	test("returns empty checks when headCommitSha is null", async () => {
		const gqlMetaNoOid = { ...SAMPLE_GQL_META, commits: { nodes: [] } };
		const { fetch } = createMockFetch([
			// fetchPR: REST detail
			{
				url: /\/pulls\/42$/,
				response: {
					status: 200,
					body: { ...makeSampleRestPR(42), body: "PR body" },
				},
			},
			// fetchPR: GraphQL review status (no commit node → null SHA)
			{
				url: /\/graphql/,
				method: "POST",
				response: {
					status: 200,
					body: makeGraphQLResponse([{ ...gqlMetaNoOid, number: 42 }]),
				},
			},
			// fetchReviews
			{ url: /\/reviews/, response: { status: 200, body: [] } },
			// fetchReviewComments (GraphQL)
			{
				url: /\/graphql/,
				method: "POST",
				response: {
					status: 200,
					body: {
						data: {
							repository: {
								pullRequest: {
									reviewThreads: {
										pageInfo: { hasNextPage: false, endCursor: null },
										nodes: [],
									},
								},
							},
						},
					},
				},
			},
			// fetchFiles
			{ url: /\/files/, response: { status: 200, body: [] } },
		]);

		const app = createTestLegit({ httpFetch: fetch });
		const summary = await app.fetchPRSummary(app.repoSlug, 42);

		expect(summary.headCommitSha).toBeNull();
		expect(summary.checks).toEqual([]);
	});
});
