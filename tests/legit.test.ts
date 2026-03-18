import { describe, test, expect, afterAll } from "bun:test";
import { Legit, type LegitOptions } from "../src/lib/legit";
import type { HttpFetch } from "../src/lib/github-client";
import type { AuthExecutor } from "../src/lib/auth";
import { mkdtempSync, rmSync } from "fs";
import { execFileSync } from "child_process";
import { join } from "path";
import { tmpdir } from "os";

// ── Helpers ─────────────────────────────────────────────────────────────────

const tmpDirs: string[] = [];

afterAll(() => {
	for (const dir of tmpDirs) {
		rmSync(dir, { recursive: true, force: true });
	}
});

function makeTmpGitRepo(remoteUrl: string): string {
	const dir = mkdtempSync(join(tmpdir(), "legit-session-test-"));
	tmpDirs.push(dir);
	execFileSync("git", ["init"], { cwd: dir, stdio: "pipe" });
	execFileSync("git", ["remote", "add", "origin", remoteUrl], {
		cwd: dir,
		stdio: "pipe",
	});
	return dir;
}

function tmpConfigPath(): string {
	const dir = mkdtempSync(join(tmpdir(), "legit-session-test-"));
	tmpDirs.push(dir);
	return join(dir, "config.json");
}

function mockAuth(): AuthExecutor {
	return (cmd, args) => {
		const key = [cmd, ...args].join(" ");
		if (key === "gh auth token") return "ghp_fake123\n";
		if (key === "gh api user --jq .login") return "testuser\n";
		throw new Error(`Unexpected command: ${key}`);
	};
}

const SAMPLE_GQL_PR = {
	number: 42,
	additions: 50,
	deletions: 10,
	reviewDecision: "APPROVED",
	mergeable: "MERGEABLE",
	commits: { nodes: [{ commit: { committedDate: "2026-03-14T00:00:00Z" } }] },
};

function mockFetch(restPRs: unknown[] = []): HttpFetch {
	return async (url, init) => {
		if (typeof url === "string" && url.includes("/pulls") && !init?.method) {
			return new Response(JSON.stringify(restPRs), {
				status: 200,
				headers: { "Content-Type": "application/json" },
			});
		}
		if (typeof url === "string" && url.includes("/graphql")) {
			// Build GraphQL response matching the number of PRs
			const gqlData: Record<string, unknown> = {};
			restPRs.forEach((pr: any, i: number) => {
				gqlData[`pr${i}`] = { ...SAMPLE_GQL_PR, number: pr.number };
			});
			return new Response(
				JSON.stringify({ data: { repository: gqlData } }),
				{ status: 200, headers: { "Content-Type": "application/json" } },
			);
		}
		return new Response(JSON.stringify({ message: "Not Found" }), {
			status: 404,
		});
	};
}

function makeSampleRestPR(n: number) {
	return {
		number: n,
		title: `PR #${n}`,
		user: { login: "alice" },
		created_at: "2026-03-01T00:00:00Z",
		updated_at: "2026-03-15T00:00:00Z",
		draft: false,
		labels: [],
		requested_reviewers: [],
		assignees: [],
	};
}

function createTestLegit(overrides?: Partial<LegitOptions>): Legit {
	return new Legit({
		cwd: makeTmpGitRepo("git@github.com:acme/widgets.git"),
		configPath: tmpConfigPath(),
		authExec: mockAuth(),
		httpFetch: mockFetch([makeSampleRestPR(42)]),
		...overrides,
	});
}

// ── Tests ───────────────────────────────────────────────────────────────────

describe("Legit", () => {
	test("fetchPRs returns PR data end-to-end", async () => {
		const app = createTestLegit();
		const prs = await app.fetchPRs();

		expect(prs).toHaveLength(1);
		expect(prs[0].number).toBe(42);
		expect(prs[0].title).toBe("PR #42");
		expect(prs[0].additions).toBe(50);
		expect(prs[0].reviewDecision).toBe("APPROVED");
	});

	test("repo detects owner/repo from git remote", () => {
		const app = createTestLegit();
		expect(app.repo).toEqual({ owner: "acme", repo: "widgets" });
	});

	test("auth resolves user and token", () => {
		const app = createTestLegit();
		expect(app.auth.user).toBe("testuser");
		expect(app.auth.token).toBe("ghp_fake123");
		expect(app.auth.tokenSource).toBe("gh-cli");
	});

	test("config loads and auto-saves user from auth", () => {
		const configPath = tmpConfigPath();
		const app = createTestLegit({ configPath });

		const config = app.config;
		expect(config.user).toBe("testuser");

		// Verify it was persisted
		const { readFileSync } = require("fs");
		const saved = JSON.parse(readFileSync(configPath, "utf-8"));
		expect(saved.user).toBe("testuser");
	});

	test("fetchPRs auto-adds detected repo to config", async () => {
		const configPath = tmpConfigPath();
		const app = createTestLegit({ configPath });

		await app.fetchPRs();

		const { readFileSync } = require("fs");
		const saved = JSON.parse(readFileSync(configPath, "utf-8"));
		expect(saved.repos).toContain("acme/widgets");
	});

	test("accessing repo does not trigger auth resolution", () => {
		let authCalled = false;
		const authExec: AuthExecutor = (cmd, args) => {
			authCalled = true;
			return "fake\n";
		};

		const app = createTestLegit({ authExec });
		// Access only repo
		const _repo = app.repo;

		expect(authCalled).toBe(false);
	});

	test("auth is cached — second access returns same value", () => {
		let callCount = 0;
		const authExec: AuthExecutor = (cmd, args) => {
			callCount++;
			const key = [cmd, ...args].join(" ");
			if (key === "gh auth token") return "ghp_fake\n";
			if (key === "gh api user --jq .login") return "testuser\n";
			throw new Error(`Unexpected: ${key}`);
		};

		const app = createTestLegit({ authExec });
		const a1 = app.auth;
		const a2 = app.auth;

		expect(a1).toBe(a2); // same reference
		expect(callCount).toBe(2); // gh auth token + gh api user, called once each
	});

	test("fetchPRs with explicit repo overrides detected repo", async () => {
		let fetchedRepo = "";
		const httpFetch: HttpFetch = async (url, init) => {
			if (typeof url === "string" && url.includes("/pulls")) {
				fetchedRepo = url;
				return new Response(JSON.stringify([]), {
					status: 200,
					headers: { "Content-Type": "application/json" },
				});
			}
			return new Response("{}", { status: 200 });
		};

		const app = createTestLegit({ httpFetch });
		await app.fetchPRs("other/repo");

		expect(fetchedRepo).toContain("other/repo");
	});

	test("fetchPR returns single PR detail", async () => {
		const detailPR = {
			...makeSampleRestPR(99),
			body: "## Fix\n\nDoes the thing.",
		};

		const httpFetch: HttpFetch = async (url, init) => {
			if (typeof url === "string" && url.includes("/pulls/99")) {
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
								pr0: { ...SAMPLE_GQL_PR, number: 99 },
							},
						},
					}),
					{ status: 200, headers: { "Content-Type": "application/json" } },
				);
			}
			return new Response("{}", { status: 404 });
		};

		const app = createTestLegit({ httpFetch });
		const pr = await app.fetchPR("acme/widgets", 99);

		expect(pr.number).toBe(99);
		expect(pr.body).toBe("## Fix\n\nDoes the thing.");
	});

	test("repoSlug returns owner/repo string", () => {
		const app = createTestLegit();
		expect(app.repoSlug).toBe("acme/widgets");
	});
});
