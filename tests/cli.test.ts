import { describe, test, expect, afterAll } from "bun:test";
import { runCommand } from "../src/cli";
import {
	cleanupTmpDirs,
	makeTmpGitRepo,
	makeSampleRestPR,
	createTestLegit,
	createMockFetch,
	makeGraphQLResponse,
	SAMPLE_GQL_META,
} from "./helpers";
import { execFileSync } from "child_process";
import { join } from "path";

afterAll(cleanupTmpDirs);

// ── In-process command tests (fast) ─────────────────────────────────────────

describe("runCommand", () => {
	test("detect returns owner/repo", async () => {
		const app = createTestLegit();
		const result = await runCommand(["detect"], app);
		expect(result.output).toEqual({ owner: "acme", repo: "widgets" });
		expect(result.error).toBeUndefined();
	});

	test("auth returns user and tokenSource without token", async () => {
		const app = createTestLegit();
		const result = await runCommand(["auth"], app);
		const output = result.output as any;
		expect(output.user).toBe("testuser");
		expect(output.tokenSource).toBe("gh-cli");
		expect(output.token).toBeUndefined();
	});

	test("config returns config object", async () => {
		const app = createTestLegit();
		const result = await runCommand(["config"], app);
		const output = result.output as any;
		expect(output).toHaveProperty("repos");
		expect(output).toHaveProperty("ui");
	});

	test("prs returns PR list", async () => {
		const app = createTestLegit();
		const result = await runCommand(["prs"], app);
		const output = result.output as any[];
		expect(output).toHaveLength(1);
		expect(output[0].number).toBe(42);
	});

	test("pr <number> returns PR summary with checks, reviews, comments, and files", async () => {
		const { fetch } = createMockFetch([
			// fetchPR: REST detail
			{
				url: /\/pulls\/42$/,
				response: {
					status: 200,
					body: { ...makeSampleRestPR(42), body: "Detail body" },
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
										nodes: [],
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
				response: { status: 200, body: [] },
			},
		]);
		const app = createTestLegit({ httpFetch: fetch });
		const result = await runCommand(["pr", "42"], app);
		const output = result.output as any;
		expect(output.number).toBe(42);
		expect(output.body).toBe("Detail body");
		expect(output.checks).toEqual([
			{ name: "build", status: "completed", conclusion: "success" },
		]);
		expect(output.reviews).toEqual([{ user: "bob", state: "APPROVED" }]);
		expect(output.comments).toEqual({
			total: 0,
			unresolved: 0,
			unresolvedHuman: 0,
			unresolvedBot: 0,
		});
		expect(output.files).toBeDefined();
	});

	test("pr without number returns error", async () => {
		const app = createTestLegit();
		const result = await runCommand(["pr"], app);
		expect(result.error).toContain("Usage");
	});

	test("pr rejects malformed numeric input like '12abc'", async () => {
		const app = createTestLegit();
		const result = await runCommand(["pr", "12abc"], app);
		expect(result.error).toContain("Usage");
	});

	test("pr rejects zero", async () => {
		const app = createTestLegit();
		const result = await runCommand(["pr", "0"], app);
		expect(result.error).toContain("Usage");
	});

	test("unknown command returns error", async () => {
		const app = createTestLegit();
		const result = await runCommand(["nonsense"], app);
		expect(result.error).toContain("Unknown command");
	});

	test("no command signals launchTui", async () => {
		const app = createTestLegit();
		const result = await runCommand([], app);
		expect(result.launchTui).toBe(true);
	});

	test("files <number> returns categorized file list with breakdown", async () => {
		const { fetch } = createMockFetch([
			{
				url: /\/files/,
				response: {
					status: 200,
					body: [
						{
							filename: "src/app.ts",
							additions: 30,
							deletions: 10,
							changes: 40,
							status: "modified",
						},
						{
							filename: "bun.lock",
							additions: 500,
							deletions: 200,
							changes: 700,
							status: "modified",
						},
					],
				},
			},
		]);
		const app = createTestLegit({ httpFetch: fetch });
		const result = await runCommand(["files", "42"], app);
		const output = result.output as any;

		expect(output.files).toHaveLength(2);
		expect(output.files[0].category).toBe("code");
		expect(output.files[1].category).toBe("generated");
		expect(output.breakdown.code).toEqual({ additions: 30, deletions: 10, files: 1 });
		expect(output.breakdown.generated).toEqual({ additions: 500, deletions: 200, files: 1 });
		expect(output.breakdown.total).toEqual({ additions: 530, deletions: 210, files: 2 });
	});

	test("files without number returns error", async () => {
		const app = createTestLegit();
		const result = await runCommand(["files"], app);
		expect(result.error).toContain("Usage");
	});

	test("files rejects zero", async () => {
		const app = createTestLegit();
		const result = await runCommand(["files", "0"], app);
		expect(result.error).toContain("Usage");
	});

	test("repos returns tracked repos from config", async () => {
		const app = createTestLegit();
		app.config.repos = ["acme/widgets"];
		const result = await runCommand(["repos"], app);
		expect(result.output).toEqual(["acme/widgets"]);
	});

	test("prs --repo=<slug> fetches PRs for explicit repo", async () => {
		const app = createTestLegit();
		const result = await runCommand(["prs", "--repo=acme/gadgets"], app);
		const output = result.output as any[];
		expect(output).toHaveLength(1);
		expect(output[0].number).toBe(42);
	});

	test("prs --all fetches PRs for all tracked repos", async () => {
		const app = createTestLegit();
		app.config.repos = ["acme/widgets"];
		const result = await runCommand(["prs", "--all"], app);
		const output = result.output as any;
		expect(output).toEqual({
			"acme/widgets": [expect.objectContaining({ number: 42 })],
		});
	});

	test("blocker <number> returns blocker result as JSON", async () => {
		const { fetch } = createMockFetch([
			// fetchPR: REST detail
			{
				url: /\/pulls\/42$/,
				response: {
					status: 200,
					body: {
						...makeSampleRestPR(42),
						body: "Detail body",
						requested_reviewers: [{ login: "testuser" }],
					},
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
					body: { total_count: 0, check_runs: [] },
				},
			},
			// fetchReviews
			{
				url: /\/reviews/,
				response: { status: 200, body: [] },
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
										nodes: [],
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
				response: { status: 200, body: [] },
			},
		]);
		const app = createTestLegit({ httpFetch: fetch });
		const result = await runCommand(["blocker", "42"], app);
		const output = result.output as any;
		// APPROVED overrides me-blocking: PR is approved, author should merge.
		expect(output.tier).toBe("waiting-on-author");
		expect(output.blocker).toBe("alice"); // PR author from SAMPLE_REST_PR
		expect(typeof output.reason).toBe("string");
	});

	test("blocker without number returns error", async () => {
		const app = createTestLegit();
		const result = await runCommand(["blocker"], app);
		expect(result.error).toContain("Usage");
	});

	test("blocker rejects zero", async () => {
		const app = createTestLegit();
		const result = await runCommand(["blocker", "0"], app);
		expect(result.error).toContain("Usage");
	});

	test("prs --with-blockers includes tier and blocker for each PR", async () => {
		const app = createTestLegit();
		const result = await runCommand(["prs", "--with-blockers"], app);
		const output = result.output as any[];
		expect(output).toHaveLength(1);
		const pr = output[0];
		expect(pr.number).toBe(42);
		expect(pr).toHaveProperty("tier");
		expect(pr).toHaveProperty("blocker");
		expect(pr).toHaveProperty("reason");
	});

	test("prs --group-by=status returns grouped result", async () => {
		const app = createTestLegit();
		const result = await runCommand(["prs", "--group-by=status"], app);
		const output = result.output as any;
		expect(output).toHaveProperty("groups");
		expect(output).toHaveProperty("totalMatched");
		expect(Array.isArray(output.groups)).toBe(true);
	});

	test("prs --group-by=author returns groups with label and prs array", async () => {
		const app = createTestLegit();
		const result = await runCommand(["prs", "--group-by=author"], app);
		const output = result.output as any;
		expect(output.groups).toHaveLength(1); // one author from the mock PR
		expect(output.groups[0]).toHaveProperty("label");
		expect(output.groups[0]).toHaveProperty("prs");
		expect(Array.isArray(output.groups[0].prs)).toBe(true);
	});

	test("prs --sort-by=size returns sorted flat list", async () => {
		const app = createTestLegit();
		const result = await runCommand(["prs", "--sort-by=size"], app);
		const output = result.output as any;
		// With sort but no group, returns grouped result with one group
		expect(output).toHaveProperty("groups");
		expect(output.groups[0]?.prs).toHaveLength(1);
	});

	test("prs --filter=<text> returns filtered result", async () => {
		const app = createTestLegit();
		// The mock PR has number=42 title="PR #42"
		const result = await runCommand(["prs", "--filter=PR"], app);
		const output = result.output as any;
		expect(output).toHaveProperty("groups");
		expect(output.totalMatched).toBe(1);
	});

	test("prs --filter=<no-match> returns empty groups", async () => {
		const app = createTestLegit();
		const result = await runCommand(["prs", "--filter=zzznomatch"], app);
		const output = result.output as any;
		expect(output.groups).toHaveLength(0);
		expect(output.totalMatched).toBe(0);
	});

	test("prs --group-by and --filter can be combined", async () => {
		const app = createTestLegit();
		const result = await runCommand(["prs", "--group-by=author", "--filter=PR"], app);
		const output = result.output as any;
		expect(output).toHaveProperty("groups");
		expect(output).toHaveProperty("totalMatched");
	});

	test("prs --sort-by=size --sort-dir=asc is valid", async () => {
		const app = createTestLegit();
		const result = await runCommand(["prs", "--sort-by=size", "--sort-dir=asc"], app);
		expect(result.error).toBeUndefined();
		const output = result.output as any;
		expect(output).toHaveProperty("groups");
	});

	test("prs --group-by and --with-blockers cannot be combined", async () => {
		const app = createTestLegit();
		const result = await runCommand(["prs", "--group-by=author", "--with-blockers"], app);
		expect(result.error).toContain("--with-blockers");
	});

	test("prs --all and --group-by cannot be combined", async () => {
		const app = createTestLegit();
		const result = await runCommand(["prs", "--all", "--group-by=author"], app);
		expect(result.error).toContain("--all");
	});

	test("prs --all and --sort-by cannot be combined", async () => {
		const app = createTestLegit();
		const result = await runCommand(["prs", "--all", "--sort-by=size"], app);
		expect(result.error).toContain("--all");
	});

	test("prs --all and --filter cannot be combined", async () => {
		const app = createTestLegit();
		const result = await runCommand(["prs", "--all", "--filter=fix"], app);
		expect(result.error).toContain("--all");
	});

	test("prs --sort-by with invalid value returns error", async () => {
		const app = createTestLegit();
		const result = await runCommand(["prs", "--sort-by=invalid"], app);
		expect(result.error).toBeDefined();
	});

	test("prs --group-by with invalid value returns error", async () => {
		const app = createTestLegit();
		const result = await runCommand(["prs", "--group-by=invalid"], app);
		expect(result.error).toBeDefined();
	});

	test("prs --sort-dir without --sort-by returns error", async () => {
		const app = createTestLegit();
		const result = await runCommand(["prs", "--sort-dir=asc"], app);
		expect(result.error).toContain("--sort-dir requires --sort-by");
	});
});

// ── Subprocess smoke test (one test to verify the entry point works) ────────

describe("CLI subprocess", () => {
	test("legit detect runs end-to-end as subprocess", () => {
		const dir = makeTmpGitRepo("git@github.com:acme/widgets.git");
		const cliPath = join(import.meta.dir, "..", "src", "cli.ts");
		const stdout = execFileSync("bun", ["run", cliPath, "detect"], {
			cwd: dir,
			encoding: "utf-8",
			stdio: ["pipe", "pipe", "pipe"],
		}).trim();
		const result = JSON.parse(stdout);
		expect(result).toEqual({ owner: "acme", repo: "widgets" });
	});
});
