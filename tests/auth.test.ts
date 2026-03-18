import { describe, test, expect } from "bun:test";
import { Legit, type AuthExecutor } from "../src/lib/legit";
import { mkdtempSync } from "fs";
import { execFileSync } from "child_process";
import { join } from "path";
import { tmpdir } from "os";

function makeTmpGitRepo(): string {
	const dir = mkdtempSync(join(tmpdir(), "legit-auth-test-"));
	execFileSync("git", ["init"], { cwd: dir, stdio: "pipe" });
	execFileSync(
		"git",
		["remote", "add", "origin", "git@github.com:acme/widgets.git"],
		{ cwd: dir, stdio: "pipe" },
	);
	return dir;
}

function mockAuth(responses: Record<string, string>): AuthExecutor {
	return (cmd, args) => {
		const key = [cmd, ...args].join(" ");
		const result = responses[key];
		if (result === undefined) throw new Error(`Command failed: ${key}`);
		return result;
	};
}

describe("Legit.auth", () => {
	test("resolves token and user from gh CLI", () => {
		const app = new Legit({
			cwd: makeTmpGitRepo(),
			authExec: mockAuth({
				"gh auth token": "ghp_abc123",
				"gh api user --jq .login": "mayfieldiv",
			}),
		});
		expect(app.auth).toEqual({
			user: "mayfieldiv",
			token: "ghp_abc123",
			tokenSource: "gh-cli",
		});
	});

	test("throws when gh auth token fails", () => {
		const app = new Legit({
			cwd: makeTmpGitRepo(),
			authExec: mockAuth({}),
		});
		expect(() => app.auth).toThrow(/Could not resolve GitHub token/);
	});

	test("throws when gh api user fails", () => {
		const app = new Legit({
			cwd: makeTmpGitRepo(),
			authExec: mockAuth({
				"gh auth token": "ghp_abc123",
			}),
		});
		expect(() => app.auth).toThrow(/Could not determine GitHub username/);
	});

	test("trims whitespace from token and user", () => {
		const app = new Legit({
			cwd: makeTmpGitRepo(),
			authExec: mockAuth({
				"gh auth token": "  ghp_abc123\n",
				"gh api user --jq .login": "  mayfieldiv\n",
			}),
		});
		expect(app.auth.token).toBe("ghp_abc123");
		expect(app.auth.user).toBe("mayfieldiv");
	});
});
