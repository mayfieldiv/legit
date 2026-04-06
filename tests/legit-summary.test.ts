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

describe("Legit individual data fetching", () => {
	test("fetchCheckRuns returns check runs for a commit", async () => {
		const { fetch } = createMockFetch([
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
		]);

		const app = createTestLegit({ httpFetch: fetch });
		const checks = await app.fetchCheckRuns(app.repoSlug, "abc123");

		expect(checks).toEqual([
			{ name: "build", status: "completed", conclusion: "success" },
		]);
	});

	test("fetchReviews returns deduplicated reviews", async () => {
		const { fetch } = createMockFetch([
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
		]);

		const app = createTestLegit({ httpFetch: fetch });
		const reviews = await app.fetchReviews(app.repoSlug, 42);

		expect(reviews).toEqual([{ user: "bob", state: "APPROVED" }]);
	});

	test("fetchCategorizedFiles returns categorized file changes", async () => {
		const { fetch } = createMockFetch([
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
		const files = await app.fetchCategorizedFiles(app.repoSlug, 42);

		expect(files.files).toHaveLength(1);
		expect(files.files[0]!.category).toBe("code");
	});

	test("fetchFullReviewThreads returns thread details", async () => {
		const { fetch } = createMockFetch([
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
												id: "RT_1",
												isResolved: false,
												path: "src/app.ts",
												line: 10,
												comments: {
													nodes: [
														{
															id: "RC_1",
															author: {
																login: "alice",
																__typename: "User",
															},
															body: "comment",
															createdAt: "2026-03-01T00:00:00Z",
															url: "https://github.com/test",
														},
													],
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
		]);

		const app = createTestLegit({ httpFetch: fetch });
		const threads = await app.fetchFullReviewThreads(app.repoSlug, 42);

		expect(threads).toHaveLength(1);
		expect(threads[0]!.isResolved).toBe(false);
		expect(threads[0]!.comments[0]!.author).toBe("alice");
	});
});
