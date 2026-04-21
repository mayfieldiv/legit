import { execFileSync } from "child_process";
import { resolve } from "path";
import { loadConfig, saveConfig, addRepo, type LegitConfig, type RepoConfig } from "./config";
import { createGitHubTransport, type HttpFetch } from "./github-transport";
import { createGitHubClient, type GitHubClient } from "./github-client";
import {
  withConcurrencyLimit,
  type ConcurrencyLimitedFetch,
  type GitHubNetworkStats,
} from "./concurrency";

export type { GitHubNetworkStats };
import { categorizeFiles as _categorizeFiles } from "./file-categorizer";
import type {
  PR,
  PRDetail,
  CheckRun,
  Review,
  FileChange,
  FileCategorization,
  FullReviewThread,
  IssueComment,
} from "./types";

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
  currentUserOverride?: string;
}

// ── Internal: repo detection ────────────────────────────────────────────────

export function parseRemoteUrl(url: string): RepoInfo {
  // SSH: git@github.com:owner/repo.git  (repo may contain dots, e.g. angular.js)
  const sshMatch = url.match(/git@github\.com:(?<owner>[^/]+)\/(?<repo>.+?)(?:\.git)?$/);
  if (sshMatch?.groups?.owner && sshMatch.groups.repo) {
    return { owner: sshMatch.groups.owner, repo: sshMatch.groups.repo };
  }

  // HTTPS: https://github.com/owner/repo.git  (repo may contain dots)
  const httpsMatch = url.match(/https?:\/\/github\.com\/(?<owner>[^/]+)\/(?<repo>.+?)(?:\.git)?$/);
  if (httpsMatch?.groups?.owner && httpsMatch.groups.repo) {
    return { owner: httpsMatch.groups.owner, repo: httpsMatch.groups.repo };
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
export function cleanGhEnv(): Record<string, string | undefined> {
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
    throw new Error("Could not determine GitHub username. Ensure `gh` CLI is authenticated.");
  }

  return { user, token, tokenSource: "gh-cli" };
}

// ── Legit session ───────────────────────────────────────────────────────────

const DEFAULT_CONFIG_PATH = `${process.env.HOME}/.legit/config.json`;
const DEFAULT_WORKTREE_BASE = `${process.env.HOME}/.legit/worktrees`;

/**
 * Sanitize a git branch name into a filesystem-safe directory segment.
 * Replaces `/` with `-`, strips characters outside `[A-Za-z0-9._-]`, collapses
 * runs of `-`, and caps length at 80.
 */
export function sanitizeBranchForPath(branch: string): string {
  const replaced = branch.replace(/\//g, "-");
  const stripped = replaced.replace(/[^A-Za-z0-9._-]/g, "-").replace(/-+/g, "-");
  const trimmed = stripped.replace(/^-+|-+$/g, "");
  return trimmed.slice(0, 80);
}

/** Expand a leading `~` to `$HOME`. Returns absolute paths unchanged. */
function expandHome(p: string): string {
  if (p === "~") return process.env.HOME ?? "~";
  if (p.startsWith("~/")) return `${process.env.HOME ?? ""}/${p.slice(2)}`;
  return p;
}

export class Legit {
  private _options: LegitOptions;
  private _repo?: RepoInfo;
  private _auth?: AuthInfo;
  private _config?: LegitConfig;
  private _repoConfigIndex?: Map<string, RepoConfig>;
  private _client?: GitHubClient;
  private _concurrencyLimited?: ConcurrencyLimitedFetch;
  private _currentUserOverride?: string;

  constructor(options?: LegitOptions) {
    this._options = options ?? {};
    this._currentUserOverride = options?.currentUserOverride?.trim() || undefined;
  }

  get configPath(): string {
    return this._options.configPath ?? process.env.LEGIT_CONFIG_PATH ?? DEFAULT_CONFIG_PATH;
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
      this._repoConfigIndex = undefined;
    }
    return this._config;
  }

  /** Re-read config from disk, clearing the in-memory cache. */
  reloadConfig(): LegitConfig {
    this._config = undefined;
    this._repoConfigIndex = undefined;
    return this.config;
  }

  get client(): GitHubClient {
    if (!this._client) {
      this._concurrencyLimited = withConcurrencyLimit(
        10,
        this._options.httpFetch ?? globalThis.fetch,
      );
      const transport = createGitHubTransport(this.auth.token, this._concurrencyLimited.fetch);
      this._client = createGitHubClient(transport);
    }
    return this._client;
  }

  /** Snapshot of GitHub HTTP concurrency (in-flight vs waiting for a slot). */
  get githubNetworkStats(): GitHubNetworkStats {
    return this._concurrencyLimited?.getSnapshot() ?? { inFlight: 0, waiting: 0 };
  }

  /** Subscribe to changes in `githubNetworkStats` (after the GitHub client is first used). */
  subscribeGitHubNetworkStats(listener: () => void): () => void {
    void this.client;
    return this._concurrencyLimited?.subscribe(listener) ?? (() => {});
  }

  get repoSlug(): string {
    return `${this.repo.owner}/${this.repo.repo}`;
  }

  /** Current user login — prefers CLI override, then config, then gh auth. */
  get currentUser(): string {
    return this._currentUserOverride ?? this.config.user ?? this.auth.user;
  }

  /** Override the current user login for this process only. */
  setCurrentUserOverride(user?: string): void {
    this._currentUserOverride = user?.trim() || undefined;
  }

  /** All tracked repos (from config + current repo), deduplicated. */
  trackedRepos(): string[] {
    const repos = new Set<string>(this.config.repos.map((r) => r.slug));
    repos.add(this.repoSlug);
    return [...repos];
  }

  /** Find the config entry for a repo slug, or undefined if untracked. */
  repoConfig(slug: string): RepoConfig | undefined {
    if (!this._repoConfigIndex) {
      this._repoConfigIndex = new Map(this.config.repos.map((r) => [r.slug, r]));
    }
    return this._repoConfigIndex.get(slug);
  }

  /**
   * Absolute path to the source clone for a repo, or undefined if none is
   * configured. `~` is expanded. Worktree-creating operations should treat
   * `undefined` as an error condition ("no sourceClone configured").
   */
  resolveSourceClone(slug: string): string | undefined {
    const entry = this.repoConfig(slug);
    if (!entry?.sourceClone) return undefined;
    return resolve(expandHome(entry.sourceClone));
  }

  /**
   * Absolute directory under which this repo's worktrees live. Precedence:
   * per-repo `worktreeRoot` > global `worktreeRoot` > `~/.legit/worktrees/<owner>/<repo>`.
   */
  resolveWorktreeRoot(slug: string): string {
    const entry = this.repoConfig(slug);
    if (entry?.worktreeRoot) return resolve(expandHome(entry.worktreeRoot));
    if (this.config.worktreeRoot) {
      return resolve(expandHome(this.config.worktreeRoot), slug);
    }
    return resolve(DEFAULT_WORKTREE_BASE, slug);
  }

  /**
   * Absolute path where legit would create a worktree for this PR. This is the
   * deterministic target regardless of whether the worktree exists yet.
   */
  resolveWorktreePath(slug: string, prNumber: number, headRef: string): string {
    const root = this.resolveWorktreeRoot(slug);
    const segment = `${prNumber}-${sanitizeBranchForPath(headRef)}`;
    return resolve(root, segment);
  }

  /**
   * Fetch open PRs. Defaults to the detected repo.
   * Auto-adds repo to config if not already tracked.
   */
  fetchPRs(repo?: string, signal?: AbortSignal): AsyncIterable<PR[]> {
    const slug = repo ?? this.repoSlug;

    // Auto-add repo to config if not tracked (non-fatal if save fails)
    const updated = addRepo(this.config, slug);
    if (updated !== this.config) {
      this._config = updated;
      try {
        saveConfig(this.configPath, this._config);
      } catch {
        // Config persistence is non-essential — don't block PR fetching
      }
    }

    return this.client.fetchOpenPRs(slug, signal);
  }

  /**
   * Fetch a single PR detail.
   */
  async fetchPR(repo: string, number: number, signal?: AbortSignal): Promise<PRDetail> {
    return this.client.fetchPR(repo, number, signal);
  }

  fetchFiles(repo: string, number: number, signal?: AbortSignal): AsyncIterable<FileChange[]> {
    return this.client.fetchFiles(repo, number, signal);
  }

  categorizeFiles(files: FileChange[]): FileCategorization {
    return _categorizeFiles(files, this.config.fileRules);
  }

  /**
   * Fetch full review threads with comment bodies and bot flags.
   */
  async fetchFullReviewThreads(
    repo: string,
    prNumber: number,
    signal?: AbortSignal,
  ): Promise<FullReviewThread[]> {
    return this.client.fetchFullReviewThreads(repo, prNumber, this.config.botLogins, signal);
  }

  /**
   * Fetch issue (top-level) comments with bot flags.
   */
  async fetchIssueComments(
    repo: string,
    prNumber: number,
    signal?: AbortSignal,
  ): Promise<IssueComment[]> {
    return this.client.fetchIssueComments(repo, prNumber, this.config.botLogins, signal);
  }

  /**
   * Fetch check runs for a commit.
   */
  async fetchCheckRuns(repo: string, commitSha: string, signal?: AbortSignal): Promise<CheckRun[]> {
    return this.client.fetchCheckRuns(repo, commitSha, signal);
  }

  /**
   * Fetch reviews for a PR.
   */
  async fetchReviews(repo: string, prNumber: number, signal?: AbortSignal): Promise<Review[]> {
    return this.client.fetchReviews(repo, prNumber, signal);
  }

  /**
   * Fetch and categorize files for a PR.
   */
  async fetchCategorizedFiles(
    repo: string,
    prNumber: number,
    signal?: AbortSignal,
  ): Promise<FileCategorization> {
    const files = await collectFiles(this.client.fetchFiles(repo, prNumber, signal));
    return this.categorizeFiles(files);
  }
}

async function collectFiles(iter: AsyncIterable<FileChange[]>): Promise<FileChange[]> {
  let last: FileChange[] = [];
  for await (const snapshot of iter) last = snapshot;
  return last;
}
