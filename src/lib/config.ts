import { mkdirSync, readFileSync, writeFileSync } from "fs";
import { dirname } from "path";
import type { FileCategory } from "./types";

export interface FileRule {
  pattern: string;
  category: FileCategory;
}

export interface LegitConfig {
  user: string;
  repos: string[];
  botLogins: string[];
  fileRules: FileRule[];
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

/**
 * Load config from disk. Returns defaults if file doesn't exist.
 * Merges partial configs with defaults so missing fields get filled in.
 */
export function loadConfig(configPath: string): LegitConfig {
  let raw: string;
  try {
    raw = readFileSync(configPath, "utf-8");
  } catch (e: any) {
    if (e.code === "ENOENT") return structuredClone(DEFAULT_CONFIG);
    throw e;
  }
  const partial = JSON.parse(raw);

  return {
    user: partial.user ?? DEFAULT_CONFIG.user,
    repos: partial.repos ?? [...DEFAULT_CONFIG.repos],
    botLogins: partial.botLogins ?? [...DEFAULT_CONFIG.botLogins],
    fileRules: partial.fileRules ?? [...DEFAULT_CONFIG.fileRules],
    ui: {
      defaultGroupBy: partial.ui?.defaultGroupBy ?? DEFAULT_CONFIG.ui.defaultGroupBy,
      defaultSortBy: partial.ui?.defaultSortBy ?? DEFAULT_CONFIG.ui.defaultSortBy,
    },
  };
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
 */
export function addRepo(config: LegitConfig, repo: string): LegitConfig {
  if (config.repos.includes(repo)) {
    return config;
  }
  return { ...config, repos: [...config.repos, repo] };
}

/**
 * Remove a repo from the config. Returns a new config (immutable).
 */
export function removeRepo(config: LegitConfig, repo: string): LegitConfig {
  const filtered = config.repos.filter((r) => r !== repo);
  if (filtered.length === config.repos.length) {
    return config;
  }
  return { ...config, repos: filtered };
}
