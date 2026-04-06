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
import { createMockTransport, SAMPLE_REST_PR } from "./helpers";

async function collectAll<T>(iter: AsyncIterable<T>): Promise<T[]> {
	const items: T[] = [];
	for await (const item of iter) items.push(item);
	return items;
}

// ── Parsing tests (pure, no mocks) ──────────────────────────────────────────

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
				head: { ref: "fix-bug" },
				base: { ref: "main" },
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
				headRef: "fix-bug",
				baseRef: "main",
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
		test("extracts last commit date and headCommitSha from nested structure", () => {
			const raw: RawPRReviewStatus = {
				prNumber: 42,
				additions: 50,
				deletions: 10,
				reviewDecision: "APPROVED",
				mergeable: "MERGEABLE",
				commits: {
					nodes: [
						{ commit: { committedDate: "2026-03-14T00:00:00Z", oid: "abc123def456" } },
					],
				},
			};
			const parsed = parseReviewStatus(raw);
			expect(parsed).toEqual({
				additions: 50,
				deletions: 10,
				reviewDecision: "APPROVED",
				mergeable: "MERGEABLE",
				lastCommitDate: "2026-03-14T00:00:00Z",
				headCommitSha: "abc123def456",
			});
		});

		test("empty commits array maps headCommitSha to null", () => {
			const raw: RawPRReviewStatus = {
				prNumber: 1,
				additions: 0,
				deletions: 0,
				reviewDecision: null,
				mergeable: "UNKNOWN",
				commits: { nodes: [] },
			};
			expect(parseReviewStatus(raw).headCommitSha).toBeNull();
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

		test("empty commits array maps lastCommitDate to null", () => {
			const raw: RawPRReviewStatus = {
				prNumber: 1,
				additions: 0,
				deletions: 0,
				reviewDecision: null,
				mergeable: "UNKNOWN",
				commits: { nodes: [] },
			};
			expect(parseReviewStatus(raw).lastCommitDate).toBeNull();
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
				headCommitSha: "abc123def456",
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
			expect(merged.lastCommitDate).toBeNull();
		});
	});
});

// ── Client orchestration tests (transport mock) ─────────────────────────────

describe("GitHubClient", () => {
	describe("fetchOpenPRs", () => {
		test("yields progressive snapshots: REST first, then enriched", async () => {
			const raw: RawRestPR = { ...SAMPLE_REST_PR, user: { login: "alice" } };
			const transport = createMockTransport({
				async *listOpenPRs() {
					yield raw;
				},
				async *fetchReviewStatus() {
					yield {
						prNumber: 42,
						additions: 50,
						deletions: 10,
						reviewDecision: "APPROVED",
						mergeable: "MERGEABLE",
						commits: {
							nodes: [
								{
									commit: {
										oid: "abc123def456",
										committedDate: "2026-03-14T00:00:00Z",
									},
								},
							],
						},
					};
				},
			});
			const client = createGitHubClient(transport);
			const snapshots = await collectAll(client.fetchOpenPRs("acme/widgets"));

			// First snapshot: REST-only (no review status)
			expect(snapshots[0]).toHaveLength(1);
			expect(snapshots[0]![0]!.reviewDecision).toBe("");
			expect(snapshots[0]![0]!.mergeable).toBe("UNKNOWN");

			// Second snapshot: enriched with review status
			expect(snapshots[1]).toHaveLength(1);
			expect(snapshots[1]![0]!.reviewDecision).toBe("APPROVED");
			expect(snapshots[1]![0]!.additions).toBe(50);
		});

		test("returns empty for repo with no PRs", async () => {
			const transport = createMockTransport();
			const client = createGitHubClient(transport);
			const snapshots = await collectAll(client.fetchOpenPRs("acme/widgets"));
			expect(snapshots).toEqual([]);
		});

		test("multiple PRs yield growing snapshots", async () => {
			const pr1: RawRestPR = { ...SAMPLE_REST_PR, number: 1, title: "PR 1" };
			const pr2: RawRestPR = { ...SAMPLE_REST_PR, number: 2, title: "PR 2" };
			const transport = createMockTransport({
				async *listOpenPRs() {
					yield pr1;
					yield pr2;
				},
				async *fetchReviewStatus() {},
			});
			const client = createGitHubClient(transport);
			const snapshots = await collectAll(client.fetchOpenPRs("acme/widgets"));

			expect(snapshots[0]).toHaveLength(1);
			expect(snapshots[1]).toHaveLength(2);
		});
	});

	describe("fetchPR", () => {
		test("merges REST + review status with body", async () => {
			const transport = createMockTransport({
				async getPR() {
					return { ...SAMPLE_REST_PR, body: "## Fix\n\nDoes the thing." };
				},
				async *fetchReviewStatus() {
					yield {
						prNumber: 42,
						additions: 50,
						deletions: 10,
						reviewDecision: "REVIEW_REQUIRED",
						mergeable: "MERGEABLE",
						commits: {
							nodes: [
								{
									commit: {
										oid: "abc123def456",
										committedDate: "2026-03-14T00:00:00Z",
									},
								},
							],
						},
					};
				},
			});
			const client = createGitHubClient(transport);
			const pr = await client.fetchPR("acme/widgets", 42);
			expect(pr.number).toBe(42);
			expect(pr.body).toBe("## Fix\n\nDoes the thing.");
			expect(pr.reviewDecision).toBe("REVIEW_REQUIRED");
		});
	});

	describe("fetchFiles", () => {
		test("yields cumulative file snapshots", async () => {
			const transport = createMockTransport({
				async *listPRFiles() {
					yield { filename: "src/a.ts", additions: 10, deletions: 5 };
					yield { filename: "src/b.ts", additions: 20, deletions: 3 };
				},
			});
			const client = createGitHubClient(transport);
			const snapshots = await collectAll(client.fetchFiles("acme/widgets", 42));

			expect(snapshots[0]).toHaveLength(1);
			expect(snapshots[0]![0]!.path).toBe("src/a.ts");
			expect(snapshots[1]).toHaveLength(2);
		});

		test("returns empty for PR with no files", async () => {
			const transport = createMockTransport();
			const client = createGitHubClient(transport);
			const snapshots = await collectAll(client.fetchFiles("acme/widgets", 42));
			expect(snapshots).toEqual([]);
		});
	});

	describe("fetchCheckRuns", () => {
		test("parses check runs from transport", async () => {
			const transport = createMockTransport({
				async *listCheckRuns() {
					yield { name: "build", status: "completed", conclusion: "success" };
					yield { name: "lint", status: "completed", conclusion: "failure" };
					yield { name: "deploy", status: "in_progress", conclusion: null };
				},
			});
			const client = createGitHubClient(transport);
			const checks = await client.fetchCheckRuns("acme/widgets", "abc123");
			expect(checks).toEqual([
				{ name: "build", status: "completed", conclusion: "success" },
				{ name: "lint", status: "completed", conclusion: "failure" },
				{ name: "deploy", status: "in_progress", conclusion: null },
			]);
		});

		test("returns empty array when no check runs", async () => {
			const transport = createMockTransport({
				async *listCheckRuns() {},
			});
			const client = createGitHubClient(transport);
			const checks = await client.fetchCheckRuns("acme/widgets", "abc123");
			expect(checks).toEqual([]);
		});
	});

	describe("fetchReviews", () => {
		test("deduplicates to latest review per user", async () => {
			const transport = createMockTransport({
				async *listReviews() {
					yield {
						user: { login: "alice" },
						state: "CHANGES_REQUESTED",
						submitted_at: "2026-03-01T00:00:00Z",
					};
					yield {
						user: { login: "alice" },
						state: "APPROVED",
						submitted_at: "2026-03-02T00:00:00Z",
					};
					yield {
						user: { login: "bob" },
						state: "COMMENTED",
						submitted_at: "2026-03-01T00:00:00Z",
					};
				},
			});
			const client = createGitHubClient(transport);
			const reviews = await client.fetchReviews("acme/widgets", 42);
			expect(reviews).toEqual([
				{ user: "alice", state: "APPROVED" },
				{ user: "bob", state: "COMMENTED" },
			]);
		});

		test("filters out PENDING reviews", async () => {
			const transport = createMockTransport({
				async *listReviews() {
					yield {
						user: { login: "alice" },
						state: "APPROVED",
						submitted_at: "2026-03-01T00:00:00Z",
					};
					yield {
						user: { login: "bob" },
						state: "PENDING",
						submitted_at: "2026-03-01T00:00:00Z",
					};
				},
			});
			const client = createGitHubClient(transport);
			const reviews = await client.fetchReviews("acme/widgets", 42);
			expect(reviews).toHaveLength(1);
			expect(reviews[0]!.user).toBe("alice");
		});

		test("returns empty array when no reviews", async () => {
			const transport = createMockTransport({
				async *listReviews() {},
			});
			const client = createGitHubClient(transport);
			const reviews = await client.fetchReviews("acme/widgets", 42);
			expect(reviews).toEqual([]);
		});
	});

	describe("fetchFullReviewThreads", () => {
		test("parses threads with comments and bot detection", async () => {
			const transport = createMockTransport({
				async *fetchFullReviewThreads() {
					yield {
						id: "RT_1",
						isResolved: false,
						path: "src/foo.ts",
						line: 42,
						comments: {
							nodes: [
								{
									id: "RC_1",
									author: { login: "bob", __typename: "User" },
									body: "Needs a null check",
									createdAt: "2026-03-10T00:00:00Z",
									url: "https://github.com/acme/widgets/pull/42#discussion_r1",
								},
								{
									id: "RC_2",
									author: { login: "alice", __typename: "User" },
									body: "Good catch, fixing",
									createdAt: "2026-03-11T00:00:00Z",
									url: "https://github.com/acme/widgets/pull/42#discussion_r2",
								},
							],
						},
					};
				},
			});
			const client = createGitHubClient(transport);
			const threads = await client.fetchFullReviewThreads("acme/widgets", 42, []);
			expect(threads).toHaveLength(1);
			expect(threads[0]!.id).toBe("RT_1");
			expect(threads[0]!.path).toBe("src/foo.ts");
			expect(threads[0]!.line).toBe(42);
			expect(threads[0]!.comments).toHaveLength(2);
			expect(threads[0]!.comments[0]!.author).toBe("bob");
			expect(threads[0]!.comments[0]!.isBot).toBe(false);
			expect(threads[0]!.comments[1]!.author).toBe("alice");
		});

		test("detects bots by __typename, [bot] suffix, and botLogins", async () => {
			const transport = createMockTransport({
				async *fetchFullReviewThreads() {
					yield {
						id: "RT_1",
						isResolved: false,
						path: "src/a.ts",
						line: 1,
						comments: {
							nodes: [
								{
									id: "RC_1",
									author: { login: "copilot[bot]", __typename: "Bot" },
									body: "Suggestion",
									createdAt: "2026-03-10T00:00:00Z",
									url: "https://example.com/r1",
								},
								{
									id: "RC_2",
									author: { login: "graphite-app[bot]" },
									body: "Auto-merge",
									createdAt: "2026-03-10T00:00:00Z",
									url: "https://example.com/r2",
								},
								{
									id: "RC_3",
									author: { login: "my-custom-bot" },
									body: "Custom",
									createdAt: "2026-03-10T00:00:00Z",
									url: "https://example.com/r3",
								},
							],
						},
					};
				},
			});
			const client = createGitHubClient(transport);
			const threads = await client.fetchFullReviewThreads("acme/widgets", 42, [
				"my-custom-bot",
			]);
			expect(threads[0]!.comments[0]!.isBot).toBe(true); // __typename Bot
			expect(threads[0]!.comments[1]!.isBot).toBe(true); // [bot] suffix
			expect(threads[0]!.comments[2]!.isBot).toBe(true); // botLogins config
		});

		test("null author maps to ghost and not bot", async () => {
			const transport = createMockTransport({
				async *fetchFullReviewThreads() {
					yield {
						id: "RT_1",
						isResolved: false,
						path: "src/a.ts",
						line: null,
						comments: {
							nodes: [
								{
									id: "RC_1",
									author: null,
									body: "Orphaned",
									createdAt: "2026-03-10T00:00:00Z",
									url: "https://example.com/r1",
								},
							],
						},
					};
				},
			});
			const client = createGitHubClient(transport);
			const threads = await client.fetchFullReviewThreads("acme/widgets", 42, []);
			expect(threads[0]!.comments[0]!.author).toBe("ghost");
			expect(threads[0]!.comments[0]!.isBot).toBe(false);
		});

		test("returns empty array when no threads", async () => {
			const transport = createMockTransport();
			const client = createGitHubClient(transport);
			const threads = await client.fetchFullReviewThreads("acme/widgets", 42, []);
			expect(threads).toEqual([]);
		});
	});

	describe("fetchIssueComments", () => {
		test("parses comments with bot detection", async () => {
			const transport = createMockTransport({
				async *listIssueComments() {
					yield {
						id: 100,
						user: { login: "bob", type: "User" },
						body: "Looks good",
						created_at: "2026-03-10T00:00:00Z",
						html_url: "https://github.com/acme/widgets/pull/42#issuecomment-100",
					};
					yield {
						id: 101,
						user: { login: "devin-ai[bot]", type: "Bot" },
						body: "Auto summary",
						created_at: "2026-03-11T00:00:00Z",
						html_url: "https://github.com/acme/widgets/pull/42#issuecomment-101",
					};
				},
			});
			const client = createGitHubClient(transport);
			const comments = await client.fetchIssueComments("acme/widgets", 42, []);
			expect(comments).toHaveLength(2);
			expect(comments[0]!.author).toBe("bob");
			expect(comments[0]!.isBot).toBe(false);
			expect(comments[0]!.url).toBe(
				"https://github.com/acme/widgets/pull/42#issuecomment-100",
			);
			expect(comments[1]!.author).toBe("devin-ai[bot]");
			expect(comments[1]!.isBot).toBe(true);
		});

		test("detects bots by explicit botLogins config", async () => {
			const transport = createMockTransport({
				async *listIssueComments() {
					yield {
						id: 100,
						user: { login: "internal-tool", type: "User" },
						body: "Automated check",
						created_at: "2026-03-10T00:00:00Z",
						html_url: "https://example.com/c100",
					};
				},
			});
			const client = createGitHubClient(transport);
			const comments = await client.fetchIssueComments("acme/widgets", 42, ["internal-tool"]);
			expect(comments[0]!.isBot).toBe(true);
		});

		test("null user maps to ghost and not bot", async () => {
			const transport = createMockTransport({
				async *listIssueComments() {
					yield {
						id: 100,
						user: null,
						body: "Orphaned comment",
						created_at: "2026-03-10T00:00:00Z",
						html_url: "https://example.com/c100",
					};
				},
			});
			const client = createGitHubClient(transport);
			const comments = await client.fetchIssueComments("acme/widgets", 42, []);
			expect(comments[0]!.author).toBe("ghost");
			expect(comments[0]!.isBot).toBe(false);
		});

		test("returns empty array when no comments", async () => {
			const transport = createMockTransport();
			const client = createGitHubClient(transport);
			const comments = await client.fetchIssueComments("acme/widgets", 42, []);
			expect(comments).toEqual([]);
		});
	});
});
