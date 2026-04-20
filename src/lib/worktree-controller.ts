/**
 * Per-session worktree controller. Owns the `["worktrees", sourceClone]`
 * queries (one per unique source clone across tracked repos), resolves
 * worktree info for a PR by matching against the cached porcelain output,
 * and drives the `w` keystroke flow: create the worktree if missing, then
 * copy the path to the clipboard via OSC 52.
 *
 * Instantiate from within a Solid component scope — uses `useQueries` and
 * `useRenderer` which depend on their respective contexts.
 */

import { createMemo, type Accessor } from "solid-js";
import type { QueryClient } from "@tanstack/solid-query";
import { useRenderer } from "@opentui/solid";
import { useQueriesLite as useQueries } from "./use-queries-lite";
import {
  createWorktreeForPR,
  expectedBranchForPR,
  listWorktrees,
  matchWorktree,
  type WorktreeEntry,
} from "./worktree";
import type { Legit } from "./legit";
import type { PR } from "./types";
import type { WorktreeInfo } from "./pr-state";
import type { StatusMessage } from "./ui-state";
import { abbreviateHome } from "./format";

export interface WorktreeController {
  /**
   * Resolve the worktree attached to this PR (if any), via the cached
   * `git worktree list` output. Matches by branch name first, falling back
   * to the deterministic legit path. Reactive via the worktree query state.
   */
  worktreeForPr(pr: PR): WorktreeInfo | undefined;

  /**
   * The `w` keystroke handler. If a worktree already exists for this PR,
   * copy its path. Otherwise run `git worktree add -d` + `gh pr checkout`,
   * then copy the new path and invalidate the worktree query. All feedback
   * goes through the status-message channel.
   */
  createWorktree(pr: PR): Promise<void>;
}

export interface WorktreeControllerDeps {
  app: Legit;
  queryClient: QueryClient;
  /** Repos visible in the current session; drives which source clones to list. */
  repoTabs: Accessor<string[]>;
  setStatusMessage: (msg: StatusMessage | null) => void;
}

export function createWorktreeController(deps: WorktreeControllerDeps): WorktreeController {
  const { app, queryClient, repoTabs, setStatusMessage } = deps;
  const renderer = useRenderer();
  const inFlight = new Set<string>();

  const uniqueSourceClones = createMemo<string[]>(() => {
    const set = new Set<string>();
    for (const slug of repoTabs()) {
      const clone = app.resolveSourceClone(slug);
      if (clone) set.add(clone);
    }
    return [...set];
  });

  const worktreeQueries = useQueries<WorktreeEntry[]>(() => ({
    queries: uniqueSourceClones().map((sourceClone) => ({
      queryKey: ["worktrees", sourceClone] as const,
      queryFn: () => listWorktrees(sourceClone),
      staleTime: Infinity,
    })),
  }));

  const queryVersion = createMemo(() =>
    worktreeQueries
      .map((query) => `${query.status}:${query.fetchStatus}:${query.dataUpdatedAt}`)
      .join("|"),
  );

  function worktreeForPr(pr: PR): WorktreeInfo | undefined {
    queryVersion();
    const slug = pr.repoSlug ?? app.repoSlug;
    const sourceClone = app.resolveSourceClone(slug);
    if (!sourceClone) return undefined;
    const entries = queryClient.getQueryData<WorktreeEntry[]>(["worktrees", sourceClone]);
    if (!entries) return undefined;
    const [owner] = slug.split("/");
    const expectedBranch = expectedBranchForPR(pr, owner ?? "");
    const expectedPath = app.resolveWorktreePath(slug, pr.number, pr.headRef);
    const match = matchWorktree(entries, expectedBranch, expectedPath);
    if (!match) return undefined;
    return { path: match.path, branch: match.branchName };
  }

  function copyToClipboard(text: string): boolean {
    if (!renderer.isOsc52Supported()) {
      setStatusMessage({ text: "clipboard unavailable: OSC 52 not supported", kind: "error" });
      return false;
    }
    try {
      renderer.copyToClipboardOSC52(text);
      return true;
    } catch (err: unknown) {
      setStatusMessage({
        text: `clipboard failed: ${err instanceof Error ? err.message : String(err)}`,
        kind: "error",
      });
      return false;
    }
  }

  async function createWorktree(pr: PR): Promise<void> {
    const slug = pr.repoSlug ?? app.repoSlug;
    const sourceClone = app.resolveSourceClone(slug);
    if (!sourceClone) {
      setStatusMessage({
        text: `no sourceClone configured for ${slug} — edit ~/.legit/config.json`,
        kind: "error",
      });
      return;
    }

    const key = `${slug}#${pr.number}`;
    if (inFlight.has(key)) return;

    const existing = worktreeForPr(pr);
    if (existing) {
      if (copyToClipboard(existing.path)) {
        setStatusMessage({ text: `copied ${abbreviateHome(existing.path)}`, kind: "success" });
      }
      return;
    }

    const targetPath = app.resolveWorktreePath(slug, pr.number, pr.headRef);
    inFlight.add(key);
    setStatusMessage({ text: `creating worktree for #${pr.number}…`, kind: "info" });

    try {
      await createWorktreeForPR({ sourceClone, targetPath, prNumber: pr.number });
      copyToClipboard(targetPath);
      setStatusMessage({ text: `copied ${abbreviateHome(targetPath)}`, kind: "success" });
      void queryClient.invalidateQueries({ queryKey: ["worktrees", sourceClone] });
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      setStatusMessage({ text: `worktree failed: ${msg.split("\n")[0]}`, kind: "error" });
    } finally {
      inFlight.delete(key);
    }
  }

  return { worktreeForPr, createWorktree };
}
