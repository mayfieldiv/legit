/**
 * Git worktree helpers. Parse `git worktree list --porcelain`, create new
 * worktrees for a PR via `git worktree add -d` + `gh pr checkout`, and compute
 * the local branch name legit expects a PR's worktree to be on.
 */

import { mkdir } from "fs/promises";
import { dirname } from "path";
import { cleanGhEnv } from "./legit";
import type { PR } from "./types";

// ── Parsing ────────────────────────────────────────────────────────────────

/**
 * One entry from `git worktree list --porcelain`. `branchRef` is the full ref
 * (e.g. `refs/heads/main`); `branchName` is the short name. Detached worktrees
 * have both fields undefined. `bare` is true only for the bare main worktree of
 * a bare clone.
 */
export interface WorktreeEntry {
  path: string;
  head: string;
  branchRef?: string;
  branchName?: string;
  detached: boolean;
  bare: boolean;
  locked?: string;
  prunable?: string;
}

/**
 * Parse the output of `git worktree list --porcelain`. The porcelain format
 * emits one record per worktree, separated by blank lines. Each record starts
 * with a `worktree <path>` line followed by zero or more attribute lines. We
 * ignore attributes we don't use.
 */
export function parseWorktreeList(stdout: string): WorktreeEntry[] {
  const entries: WorktreeEntry[] = [];
  const records = stdout.split(/\n\n+/);

  for (const record of records) {
    const lines = record.split("\n").filter((l) => l.length > 0);
    if (lines.length === 0) continue;

    let path: string | undefined;
    let head = "";
    let branchRef: string | undefined;
    let detached = false;
    let bare = false;
    let locked: string | undefined;
    let prunable: string | undefined;

    for (const line of lines) {
      if (line.startsWith("worktree ")) {
        path = line.slice("worktree ".length);
      } else if (line.startsWith("HEAD ")) {
        head = line.slice("HEAD ".length);
      } else if (line.startsWith("branch ")) {
        branchRef = line.slice("branch ".length);
      } else if (line === "detached") {
        detached = true;
      } else if (line === "bare") {
        bare = true;
      } else if (line === "locked" || line.startsWith("locked ")) {
        locked = line === "locked" ? "" : line.slice("locked ".length);
      } else if (line === "prunable" || line.startsWith("prunable ")) {
        prunable = line === "prunable" ? "" : line.slice("prunable ".length);
      }
    }

    if (path === undefined) continue;

    const entry: WorktreeEntry = {
      path,
      head,
      detached,
      bare,
    };
    if (branchRef !== undefined) {
      entry.branchRef = branchRef;
      entry.branchName = branchRef.startsWith("refs/heads/")
        ? branchRef.slice("refs/heads/".length)
        : branchRef;
    }
    if (locked !== undefined) entry.locked = locked;
    if (prunable !== undefined) entry.prunable = prunable;
    entries.push(entry);
  }

  return entries;
}

// ── Runtime ────────────────────────────────────────────────────────────────

/**
 * Run a command and return stdout. Throws if the process exits non-zero,
 * wrapping the error with the stderr tail for display and the full stderr on
 * a `.stderr` property for logging.
 */
async function run(
  label: string,
  cmd: string[],
  options: { cwd?: string; env?: Record<string, string | undefined> } = {},
): Promise<string> {
  const proc = Bun.spawn(cmd, {
    cwd: options.cwd,
    env: options.env,
    stdout: "pipe",
    stderr: "pipe",
  });

  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ]);

  if (exitCode !== 0) {
    const err = new Error(`${label} failed: ${stderr.trim() || `exit ${exitCode}`}`) as Error & {
      stderr?: string;
    };
    err.stderr = stderr;
    throw err;
  }
  return stdout;
}

/** Shell out and parse. Throws if `sourceClone` is not a git repo. */
export async function listWorktrees(sourceClone: string): Promise<WorktreeEntry[]> {
  const stdout = await run("git worktree list", [
    "git",
    "-C",
    sourceClone,
    "worktree",
    "list",
    "--porcelain",
  ]);
  return parseWorktreeList(stdout);
}

/**
 * Create a worktree for a PR. Strategy:
 *   1. `git -C <sourceClone> worktree add -d <targetPath>` — attach a detached
 *      worktree at HEAD of the source clone. Needs the parent dir to exist.
 *   2. `gh pr checkout <prNumber>` (cwd=targetPath) — resolve the PR's branch
 *      (including fork-owner prefixing for cross-repo PRs) and check it out.
 *
 * Rejects with an Error whose `.stderr` carries the underlying tool output.
 */
export async function createWorktreeForPR(params: {
  sourceClone: string;
  targetPath: string;
  prNumber: number;
}): Promise<void> {
  const { sourceClone, targetPath, prNumber } = params;

  // git worktree add fails if the parent dir doesn't exist.
  await mkdir(dirname(targetPath), { recursive: true });

  await run("git worktree add", ["git", "-C", sourceClone, "worktree", "add", "-d", targetPath]);

  // gh doesn't accept -C; use cwd instead.
  await run("gh pr checkout", ["gh", "pr", "checkout", String(prNumber)], {
    cwd: targetPath,
    env: cleanGhEnv(),
  });
}

// ── PR matching ────────────────────────────────────────────────────────────

/**
 * Compute the local branch name `gh pr checkout` would use for a given PR.
 * Same-repo PRs keep `pr.headRef`; fork PRs are prefixed with `<forkOwner>-`
 * to avoid collisions across multiple forks of the same branch name.
 *
 * If `headRepositoryOwner` is missing (fork deleted), falls back to `pr.headRef`
 * — the worktree indicator won't match, but we avoid a spurious mismatch.
 */
export function expectedBranchForPR(pr: PR, repoOwner: string): string {
  if (!pr.headRepositoryOwner || pr.headRepositoryOwner === repoOwner) return pr.headRef;
  return `${pr.headRepositoryOwner}-${pr.headRef}`;
}

/**
 * Find a worktree that hosts the PR. Tries branch match first (matches manual
 * worktrees at any path). Falls back to the deterministic path we would have
 * created to catch detached worktrees and still-populating gh pr checkouts.
 */
export function matchWorktree(
  entries: WorktreeEntry[],
  expectedBranch: string,
  expectedPath: string,
): WorktreeEntry | undefined {
  const byBranch = entries.find((e) => e.branchName === expectedBranch);
  if (byBranch) return byBranch;
  return entries.find((e) => e.path === expectedPath);
}
