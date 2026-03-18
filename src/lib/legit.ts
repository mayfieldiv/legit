import { detectRepo, type RepoInfo } from "./detect-repo";
import { resolveAuth, type AuthExecutor, type AuthInfo } from "./auth";
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
	type PR,
	type PRDetail,
} from "./github-client";

export interface LegitOptions {
	configPath?: string;
	cwd?: string;
	authExec?: AuthExecutor;
	httpFetch?: HttpFetch;
}

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
