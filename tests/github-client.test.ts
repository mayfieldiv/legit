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
						commits: { nodes: [{ commit: { committedDate: "2026-03-14T00:00:00Z" } }] },
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
						commits: { nodes: [{ commit: { committedDate: "2026-03-14T00:00:00Z" } }] },
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
});
