import { describe, test, expect, afterAll } from "bun:test";
import { runCommand, type CommandResult } from "../src/cli";
import { Legit, type LegitOptions } from "../src/lib/legit";
import {
	cleanupTmpDirs,
	makeTmpGitRepo,
	tmpConfigPath,
	mockAuthExec,
	mockHttpFetch,
	makeSampleRestPR,
} from "./helpers";
import { execFileSync } from "child_process";
import { join } from "path";

afterAll(cleanupTmpDirs);

function createTestLegit(overrides?: Partial<LegitOptions>): Legit {
	return new Legit({
		cwd: makeTmpGitRepo("git@github.com:acme/widgets.git"),
		configPath: tmpConfigPath(),
		authExec: mockAuthExec(),
		httpFetch: mockHttpFetch([makeSampleRestPR(42)]),
		...overrides,
	});
}

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
		const detailPR = {
			...makeSampleRestPR(42),
			body: "Detail body",
		};
		const app = createTestLegit({
			httpFetch: async (url: string, init?: RequestInit) => {
				if (typeof url === "string" && url.includes("/pulls/42") && !init?.method) {
					return new Response(JSON.stringify(detailPR), {
						status: 200,
						headers: { "Content-Type": "application/json" },
					});
				}
				if (typeof url === "string" && url.includes("/graphql")) {
					return new Response(
						JSON.stringify({
							data: {
								repository: {
									pr0: {
										number: 42,
										additions: 50,
										deletions: 10,
										reviewDecision: "APPROVED",
										mergeable: "MERGEABLE",
										commits: {
											nodes: [
												{
													commit: {
														committedDate:
															"2026-03-14T00:00:00Z",
													},
												},
											],
										},
									},
								},
							},
						}),
						{
							status: 200,
							headers: { "Content-Type": "application/json" },
						},
					);
				}
				return new Response("{}", { status: 404 });
			},
		});
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
