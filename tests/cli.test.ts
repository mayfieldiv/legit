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

	test("pr <number> returns PR detail", async () => {
		const { fetch } = createMockFetch([
			{
				url: /\/pulls\/42$/,
				response: { status: 200, body: { ...makeSampleRestPR(42), body: "Detail body" } },
			},
			{
				url: /\/graphql/,
				method: "POST",
				response: {
					status: 200,
					body: makeGraphQLResponse([{ ...SAMPLE_GQL_META, number: 42 }]),
				},
			},
		]);
		const app = createTestLegit({ httpFetch: fetch });
		const result = await runCommand(["pr", "42"], app);
		const output = result.output as any;
		expect(output.number).toBe(42);
		expect(output.body).toBe("Detail body");
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
