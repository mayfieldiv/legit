import { describe, test, expect, afterAll } from "bun:test";
import { mkdtempSync, rmSync } from "fs";
import { join } from "path";
import { tmpdir } from "os";
import { execFileSync } from "child_process";

const tmpDirs: string[] = [];

function makeTmpGitRepo(remoteUrl: string): string {
	const dir = mkdtempSync(join(tmpdir(), "legit-cli-test-"));
	tmpDirs.push(dir);
	execFileSync("git", ["init"], { cwd: dir, stdio: "pipe" });
	execFileSync("git", ["remote", "add", "origin", remoteUrl], {
		cwd: dir,
		stdio: "pipe",
	});
	return dir;
}

afterAll(() => {
	for (const dir of tmpDirs) {
		rmSync(dir, { recursive: true, force: true });
	}
});

function runCli(
	args: string[],
	options?: { cwd?: string; env?: Record<string, string> },
): { stdout: string; exitCode: number } {
	const cliPath = join(import.meta.dir, "..", "src", "cli.ts");
	try {
		const stdout = execFileSync("bun", ["run", cliPath, ...args], {
			cwd: options?.cwd ?? import.meta.dir + "/..",
			encoding: "utf-8",
			env: { ...process.env, ...options?.env },
			stdio: ["pipe", "pipe", "pipe"],
		});
		return { stdout: stdout.trim(), exitCode: 0 };
	} catch (err: any) {
		return {
			stdout: (err.stdout ?? "").trim(),
			exitCode: err.status ?? 1,
		};
	}
}

describe("CLI: legit detect", () => {
	test("outputs owner/repo JSON for a git repo with SSH remote", () => {
		const dir = makeTmpGitRepo("git@github.com:acme/widgets.git");
		const { stdout, exitCode } = runCli(["detect"], { cwd: dir });
		expect(exitCode).toBe(0);
		const result = JSON.parse(stdout);
		expect(result).toEqual({ owner: "acme", repo: "widgets" });
	});

	test("outputs owner/repo JSON for HTTPS remote", () => {
		const dir = makeTmpGitRepo("https://github.com/acme/gadgets.git");
		const { stdout, exitCode } = runCli(["detect"], { cwd: dir });
		expect(exitCode).toBe(0);
		const result = JSON.parse(stdout);
		expect(result).toEqual({ owner: "acme", repo: "gadgets" });
	});

	test("exits with error when not in a git repo", () => {
		const dir = mkdtempSync(join(tmpdir(), "legit-cli-test-"));
		tmpDirs.push(dir);
		const { exitCode } = runCli(["detect"], { cwd: dir });
		expect(exitCode).not.toBe(0);
	});
});

describe("CLI: legit auth", () => {
	test("outputs user and token source", () => {
		const { stdout, exitCode } = runCli(["auth"]);
		expect(exitCode).toBe(0);
		const result = JSON.parse(stdout);
		expect(result).toHaveProperty("user");
		expect(result).toHaveProperty("tokenSource");
		expect(result.user).toBe("mayfieldiv");
		expect(result.tokenSource).toBe("gh-cli");
		// Token itself should NOT be in the output
		expect(result).not.toHaveProperty("token");
	});
});

describe("CLI: legit config", () => {
	test("outputs config JSON", () => {
		const dir = mkdtempSync(join(tmpdir(), "legit-cli-test-"));
		tmpDirs.push(dir);
		const configPath = join(dir, "config.json");
		const { stdout, exitCode } = runCli(["config"], {
			env: { LEGIT_CONFIG_PATH: configPath },
		});
		expect(exitCode).toBe(0);
		const result = JSON.parse(stdout);
		expect(result).toHaveProperty("repos");
		expect(result).toHaveProperty("ui");
	});
});

describe("CLI: unknown subcommand", () => {
	test("exits with error and shows help", () => {
		const { exitCode, stdout } = runCli(["nonsense"]);
		expect(exitCode).not.toBe(0);
	});
});
