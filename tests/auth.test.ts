import { describe, test, expect } from "bun:test";
import { resolveAuth, type AuthInfo, type AuthExecutor } from "../src/lib/auth";

function mockExecutor(responses: Record<string, string>): AuthExecutor {
	return (cmd: string, args: string[]) => {
		const key = [cmd, ...args].join(" ");
		const result = responses[key];
		if (result === undefined) {
			throw new Error(`Command failed: ${key}`);
		}
		return result;
	};
}

describe("resolveAuth", () => {
	test("resolves token and user from gh CLI", () => {
		const exec = mockExecutor({
			"gh auth token": "ghp_abc123",
			"gh api user --jq .login": "mayfieldiv",
		});
		const result = resolveAuth(exec);
		expect(result).toEqual({
			user: "mayfieldiv",
			token: "ghp_abc123",
			tokenSource: "gh-cli",
		});
	});

	test("throws when gh auth token fails", () => {
		const exec = mockExecutor({});
		expect(() => resolveAuth(exec)).toThrow(/Could not resolve GitHub token/);
	});

	test("throws when gh api user fails", () => {
		const exec = mockExecutor({
			"gh auth token": "ghp_abc123",
		});
		expect(() => resolveAuth(exec)).toThrow(
			/Could not determine GitHub username/,
		);
	});

	test("trims whitespace from token and user", () => {
		const exec = mockExecutor({
			"gh auth token": "  ghp_abc123\n",
			"gh api user --jq .login": "  mayfieldiv\n",
		});
		const result = resolveAuth(exec);
		expect(result.token).toBe("ghp_abc123");
		expect(result.user).toBe("mayfieldiv");
	});
});
