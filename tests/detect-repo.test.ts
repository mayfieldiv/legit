import { describe, test, expect, afterAll } from "bun:test";
import { detectRepo, parseRemoteUrl } from "../src/lib/detect-repo";
import { execFileSync } from "child_process";
import { mkdtempSync, rmSync } from "fs";
import { join } from "path";
import { tmpdir } from "os";

const tmpDirs: string[] = [];

function makeTmpGitRepo(remoteUrl?: string): string {
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

afterAll(() => {
	for (const dir of tmpDirs) {
		rmSync(dir, { recursive: true, force: true });
	}
});

describe("detectRepo", () => {
	test("detects owner/repo from SSH remote", () => {
		const dir = makeTmpGitRepo("git@github.com:acme/widgets.git");
		const result = detectRepo(dir);
		expect(result).toEqual({ owner: "acme", repo: "widgets" });
	});

	test("detects owner/repo from HTTPS remote", () => {
		const dir = makeTmpGitRepo("https://github.com/acme/widgets.git");
		const result = detectRepo(dir);
		expect(result).toEqual({ owner: "acme", repo: "widgets" });
	});

	test("detects owner/repo from HTTPS remote without .git suffix", () => {
		const dir = makeTmpGitRepo("https://github.com/acme/widgets");
		const result = detectRepo(dir);
		expect(result).toEqual({ owner: "acme", repo: "widgets" });
	});

	test("throws when git repo has no remote", () => {
		const dir = makeTmpGitRepo();
		expect(() => detectRepo(dir)).toThrow(/No git remote/);
	});

	test("throws when directory is not a git repo", () => {
		const dir = mkdtempSync(join(tmpdir(), "legit-test-"));
		tmpDirs.push(dir);
		expect(() => detectRepo(dir)).toThrow();
	});

	test("throws when directory does not exist", () => {
		expect(() => detectRepo("/nonexistent/path")).toThrow();
	});

	test("defaults to process.cwd() when no cwd provided", () => {
		// The legit repo itself should be detectable
		const result = detectRepo();
		expect(result).toEqual({ owner: "mayfieldiv", repo: "legit" });
	});
});

describe("parseRemoteUrl", () => {
	test("parses SSH URL with .git suffix", () => {
		expect(parseRemoteUrl("git@github.com:owner/repo.git")).toEqual({
			owner: "owner",
			repo: "repo",
		});
	});

	test("parses SSH URL without .git suffix", () => {
		expect(parseRemoteUrl("git@github.com:owner/repo")).toEqual({
			owner: "owner",
			repo: "repo",
		});
	});

	test("parses HTTPS URL with .git suffix", () => {
		expect(parseRemoteUrl("https://github.com/owner/repo.git")).toEqual({
			owner: "owner",
			repo: "repo",
		});
	});

	test("parses HTTPS URL without .git suffix", () => {
		expect(parseRemoteUrl("https://github.com/owner/repo")).toEqual({
			owner: "owner",
			repo: "repo",
		});
	});

	test("throws on non-GitHub URL", () => {
		expect(() => parseRemoteUrl("git@gitlab.com:owner/repo.git")).toThrow(
			/Cannot parse/,
		);
	});

	test("throws on malformed URL", () => {
		expect(() => parseRemoteUrl("not-a-url")).toThrow(/Cannot parse/);
	});
});
