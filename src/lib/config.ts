import { mkdirSync, readFileSync, writeFileSync } from "fs";
import { dirname } from "path";
import type { FileCategory } from "./types";

export interface FileRule {
  pattern: string;
  category: FileCategory;
}

/**
 * Per-repo configuration. `slug` is the GitHub `owner/repo`. When
 * `sourceClone` is set, legit can create git worktrees for that repo's PRs;
 * when absent, worktree-related features error out for that repo.
 * `worktreeRoot` overrides the global default worktree root for this repo.
 */
export interface RepoConfig {
  slug: string;
  sourceClone?: string;
  worktreeRoot?: string;
}

export interface LegitConfig {
  user: string;
  repos: RepoConfig[];
  botLogins: string[];
  fileRules: FileRule[];
  /** Global default worktree root. Per-repo `worktreeRoot` takes precedence. */
  worktreeRoot?: string;
  ui: {
    defaultGroupBy: string;
    defaultSortBy: string;
  };
}

export const DEFAULT_CONFIG: LegitConfig = {
  user: "",
  repos: [],
  botLogins: ["app/devin-ai-integration", "app/copilot-swe-agent"],
  fileRules: [],
  ui: {
    defaultGroupBy: "smart-status",
    defaultSortBy: "updated",
  },
};

function parseRepoEntry(entry: unknown): RepoConfig | null {
  if (typeof entry === "string") {
    // tolerate legacy string form while reading; caller will rewrite on save
    return entry.includes("/") ? { slug: entry } : null;
  }
  if (entry && typeof entry === "object" && "slug" in entry) {
    const obj = entry as Record<string, unknown>;
    const slug = obj.slug;
    if (typeof slug !== "string" || !slug.includes("/")) return null;
    const result: RepoConfig = { slug };
    if (typeof obj.sourceClone === "string") result.sourceClone = obj.sourceClone;
    if (typeof obj.worktreeRoot === "string") result.worktreeRoot = obj.worktreeRoot;
    return result;
  }
  return null;
}

/**
 * Load config from disk. Returns defaults if file doesn't exist.
 * Merges partial configs with defaults so missing fields get filled in.
 */
export function loadConfig(configPath: string): LegitConfig {
  let raw: string;
  try {
    raw = readFileSync(configPath, "utf-8");
  } catch (e: unknown) {
    if (e instanceof Error && "code" in e && e.code === "ENOENT")
      return structuredClone(DEFAULT_CONFIG);
    throw e;
  }
  const partial = JSON.parse(raw);

  const repos: RepoConfig[] = Array.isArray(partial.repos)
    ? partial.repos.map(parseRepoEntry).filter((r: RepoConfig | null): r is RepoConfig => r != null)
    : [...DEFAULT_CONFIG.repos];

  const config: LegitConfig = {
    user: partial.user ?? DEFAULT_CONFIG.user,
    repos,
    botLogins: partial.botLogins ?? [...DEFAULT_CONFIG.botLogins],
    fileRules: partial.fileRules ?? [...DEFAULT_CONFIG.fileRules],
    ui: {
      defaultGroupBy: partial.ui?.defaultGroupBy ?? DEFAULT_CONFIG.ui.defaultGroupBy,
      defaultSortBy: partial.ui?.defaultSortBy ?? DEFAULT_CONFIG.ui.defaultSortBy,
    },
  };

  if (typeof partial.worktreeRoot === "string") config.worktreeRoot = partial.worktreeRoot;
  return config;
}

/**
 * Save config to disk. Creates parent directories if needed.
 */
export function saveConfig(configPath: string, config: LegitConfig): void {
  mkdirSync(dirname(configPath), { recursive: true });
  writeFileSync(configPath, JSON.stringify(config, null, "\t") + "\n");
}

/**
 * Add a repo to the config. Returns a new config (immutable).
 * Existing entries are preserved untouched so user-edited fields like
 * `sourceClone` are not clobbered by auto-add-on-query.
 */
export function addRepo(config: LegitConfig, slug: string): LegitConfig {
  if (config.repos.some((r) => r.slug === slug)) {
    return config;
  }
  return { ...config, repos: [...config.repos, { slug }] };
}

/**
 * Remove a repo from the config. Returns a new config (immutable).
 */
export function removeRepo(config: LegitConfig, slug: string): LegitConfig {
  const filtered = config.repos.filter((r) => r.slug !== slug);
  if (filtered.length === config.repos.length) {
    return config;
  }
  return { ...config, repos: filtered };
}
