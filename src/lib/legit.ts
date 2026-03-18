import { execFileSync } from "child_process";
import {
	loadConfig,
	saveConfig,
	addRepo,
	type LegitConfig,
} from "./config";
import {
	createGitHubClient,
	type GitHubClient,
	type HttpFetch,
} from "./github-client";
import type { PR, PRDetail } from "./types";

// ── Types ───────────────────────────────────────────────────────────────────

export interface RepoInfo {
	owner: string;
	repo: string;
}

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

export interface LegitOptions {
	configPath?: string;
	cwd?: string;
	authExec?: AuthExecutor;
	httpFetch?: HttpFetch;
}

// ── Internal: repo detection ────────────────────────────────────────────────

export function parseRemoteUrl(url: string): RepoInfo {
	// SSH: git@github.com:owner/repo.git
	const sshMatch = url.match(/git@github\.com:([^/]+)\/([^/.]+)(?:\.git)?$/);
	if (sshMatch) {
		return { owner: sshMatch[1], repo: sshMatch[2] };
	}

	// HTTPS: https://github.com/owner/repo.git
	const httpsMatch = url.match(
		/https?:\/\/github\.com\/([^/]+)\/([^/.]+)(?:\.git)?$/,
	);
	if (httpsMatch) {
		return { owner: httpsMatch[1], repo: httpsMatch[2] };
	}

	throw new Error(`Cannot parse GitHub remote URL: ${url}`);
}

function detectRepo(cwd?: string): RepoInfo {
	const dir = cwd ?? process.cwd();

	let remoteUrl: string;
	try {
		remoteUrl = execFileSync("git", ["remote", "get-url", "origin"], {
			cwd: dir,
			encoding: "utf-8",
			stdio: ["pipe", "pipe", "pipe"],
		}).trim();
	} catch {
		throw new Error(`No git remote 'origin' found in ${dir}`);
	}

	return parseRemoteUrl(remoteUrl);
}

// ── Internal: auth resolution ───────────────────────────────────────────────

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

function resolveAuth(exec: AuthExecutor = defaultExecutor): AuthInfo {
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

// ── Legit session ───────────────────────────────────────────────────────────

const DEFAULT_CONFIG_PATH = `${process.env.HOME}/.config/legit/config.json`;

export class Legit {
	private _options: LegitOptions;
	private _repo?: RepoInfo;
	private _auth?: AuthInfo;
	private _config?: LegitConfig;
	private _client?: GitHubClient;

	constructor(options?: LegitOptions) {
		this._options = options ?? {};
	}

	get configPath(): string {
		return (
			this._options.configPath ??
			process.env.LEGIT_CONFIG_PATH ??
			DEFAULT_CONFIG_PATH
		);
	}

	get repo(): RepoInfo {
		if (!this._repo) {
			this._repo = detectRepo(this._options.cwd);
		}
		return this._repo;
	}

	get auth(): AuthInfo {
		if (!this._auth) {
			this._auth = resolveAuth(this._options.authExec);
		}
		return this._auth;
	}

	get config(): LegitConfig {
		if (!this._config) {
			let config = loadConfig(this.configPath);

			// Auto-update user from auth if not set
			if (!config.user) {
				try {
					config = { ...config, user: this.auth.user };
					saveConfig(this.configPath, config);
				} catch {
					// Auth may not be available — skip
				}
			}

			this._config = config;
		}
		return this._config;
	}

	get client(): GitHubClient {
		if (!this._client) {
			this._client = createGitHubClient(
				this.auth.token,
				this._options.httpFetch,
			);
		}
		return this._client;
	}

	get repoSlug(): string {
		return `${this.repo.owner}/${this.repo.repo}`;
	}

	/**
	 * Fetch open PRs. Defaults to the detected repo.
	 * Auto-adds repo to config if not already tracked.
	 */
	async fetchPRs(repo?: string): Promise<PR[]> {
		const slug = repo ?? this.repoSlug;

		// Auto-add repo to config if not tracked
		if (!this.config.repos.includes(slug)) {
			this._config = addRepo(this.config, slug);
			saveConfig(this.configPath, this._config);
		}

		return this.client.fetchOpenPRs(slug);
	}

	/**
	 * Fetch a single PR detail.
	 */
	async fetchPR(repo: string, number: number): Promise<PRDetail> {
		return this.client.fetchPR(repo, number);
	}
}
