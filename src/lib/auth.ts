import { execFileSync } from "child_process";

export interface AuthInfo {
	user: string;
	token: string;
	tokenSource: string;
}

/**
 * Executor function — runs a command and returns stdout.
 * Injected so tests can provide a mock.
 */
export type AuthExecutor = (cmd: string, args: string[]) => string;

/**
 * Build a clean env for gh CLI calls.
 * Strips GITHUB_TOKEN and GH_TOKEN so gh uses its own keyring auth
 * rather than potentially polluted env vars (e.g. 1Password op:// refs).
 */
function cleanGhEnv(): Record<string, string | undefined> {
	const env = { ...process.env };
	delete env.GITHUB_TOKEN;
	delete env.GH_TOKEN;
	return env;
}

const defaultExecutor: AuthExecutor = (cmd, args) => {
	return execFileSync(cmd, args, {
		encoding: "utf-8",
		stdio: ["pipe", "pipe", "pipe"],
		env: cleanGhEnv(),
	});
};

/**
 * Resolve GitHub authentication from the gh CLI.
 */
export function resolveAuth(exec: AuthExecutor = defaultExecutor): AuthInfo {
	let token: string;
	try {
		token = exec("gh", ["auth", "token"]).trim();
	} catch {
		throw new Error(
			"Could not resolve GitHub token. Ensure `gh` CLI is installed and authenticated.",
		);
	}

	let user: string;
	try {
		user = exec("gh", ["api", "user", "--jq", ".login"]).trim();
	} catch {
		throw new Error(
			"Could not determine GitHub username. Ensure `gh` CLI is authenticated.",
		);
	}

	return { user, token, tokenSource: "gh-cli" };
}
