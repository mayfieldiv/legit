import { mkdtempSync, rmSync } from "fs";
import { execFileSync } from "child_process";
import { join } from "path";
import { tmpdir } from "os";
import type { PR } from "../src/lib/types";
import type { AuthExecutor } from "../src/lib/legit";
import type { HttpFetch } from "../src/lib/github-client";

// ── Temp directory management ───────────────────────────────────────────────

const tmpDirs: string[] = [];

export function cleanupTmpDirs(): void {
	for (const dir of tmpDirs) {
		rmSync(dir, { recursive: true, force: true });
	}
	tmpDirs.length = 0;
}

// ── Git repo helpers ────────────────────────────────────────────────────────

export function makeTmpGitRepo(remoteUrl?: string): string {
	const dir = mkdtempSync(join(tmpdir(), "legit-test-"));
	tmpDirs.push(dir);
	execFileSync("git", ["init"], { cwd: dir, stdio: "pipe" });
	if (remoteUrl) {
		execFileSync("git", ["remote", "add", "origin", remoteUrl], {
			cwd: dir,
			stdio: "pipe",
		});
	}
	return dir;
}

export function tmpConfigPath(): string {
	const dir = mkdtempSync(join(tmpdir(), "legit-test-"));
	tmpDirs.push(dir);
	return join(dir, "config.json");
}

// ── Mock factories ──────────────────────────────────────────────────────────

export function mockAuthExec(
	responses: Record<string, string> = {
		"gh auth token": "ghp_fake123\n",
		"gh api user --jq .login": "testuser\n",
	},
): AuthExecutor {
	return (cmd, args) => {
		const key = [cmd, ...args].join(" ");
		const result = responses[key];
		if (result === undefined) throw new Error(`Command failed: ${key}`);
		return result;
	};
}

export const SAMPLE_GQL_PR = {
	number: 42,
	additions: 50,
	deletions: 10,
	reviewDecision: "APPROVED",
	mergeable: "MERGEABLE",
	commits: { nodes: [{ commit: { committedDate: "2026-03-14T00:00:00Z" } }] },
};

export function makeSampleRestPR(n: number) {
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

export function mockHttpFetch(restPRs: unknown[] = []): HttpFetch {
	return async (url, init) => {
		if (typeof url === "string" && url.includes("/pulls") && !init?.method) {
			return new Response(JSON.stringify(restPRs), {
				status: 200,
				headers: { "Content-Type": "application/json" },
			});
		}
		if (typeof url === "string" && url.includes("/graphql")) {
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

// ── PR factory ──────────────────────────────────────────────────────────────

export function makePR(overrides: Partial<PR> = {}): PR {
	return {
		number: 42,
		title: "Fix the thing",
		author: "alice",
		createdAt: "2026-03-01T00:00:00Z",
		updatedAt: "2026-03-15T00:00:00Z",
		additions: 50,
		deletions: 10,
		isDraft: false,
		labels: [],
		requestedReviewers: [],
		assignees: [],
		reviewDecision: "",
		mergeable: "MERGEABLE",
		lastCommitDate: "2026-03-14T00:00:00Z",
		...overrides,
	};
}
