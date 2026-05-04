/**
 * Per-PR refresh queue: priority ordering with a concurrency cap.
 *
 * Owns the queued-refresh map, active count, and pump loop. The queue is
 * deliberately I/O-agnostic — callers inject `runRefresh` to do the
 * fetch + cache-write work. Errors from `runRefresh` are surfaced through
 * the shared status-message channel and the failed item is dropped from
 * the queue so a follow-up press can re-queue it.
 *
 * Re-queuing a PR that is already "queued" upgrades its priority to the
 * stronger of the two and unions `includeFiles`. Re-queuing a PR that is
 * currently "refreshing" is a no-op — wait for the in-flight run to
 * settle, then re-queue.
 */

import { createSignal } from "solid-js";
import { GITHUB_HTTP_MAX_CONCURRENT_REQUESTS } from "./concurrency";
import type { PRIdentity } from "./pr-identity";
import type { StatusMessage } from "./ui-state";

export type RefreshPriority = 0 | 1 | 2 | 3 | 4;
export type RefreshPhase = "queued" | "refreshing";

/** A snapshot of one queued/refreshing PR. Passed to `runRefresh`. */
export interface QueueItem {
  /** Stable map key — `${repoSlug}#${number}` with default repo slug filled in. */
  key: string;
  pr: PRIdentity;
  priority: RefreshPriority;
  includeFiles: boolean;
}

interface Entry extends QueueItem {
  phase: RefreshPhase;
  /** Insertion order — used as the FIFO tiebreaker within a priority tier. */
  order: number;
}

export interface RefreshQueueState {
  /** Returns the current refresh phase for a PR, or `undefined` when idle. */
  refreshStateForPr(pr: PRIdentity): RefreshPhase | undefined;
}

export interface RefreshQueueActions {
  queuePrRefresh(
    pr: PRIdentity,
    options: { priority: RefreshPriority; includeFiles: boolean },
  ): void;
}

export interface RefreshQueueDeps {
  /** Default repo slug used when a PR's `repoSlug` is undefined. */
  defaultRepoSlug: string;
  /** Run the I/O for a single queued refresh. The pump enforces concurrency. */
  runRefresh: (item: QueueItem) => Promise<void>;
  /** Used to surface fetch errors as a transient status message. */
  setStatusMessage: (msg: StatusMessage | null) => void;
  /** Concurrent active refresh cap. Defaults to the HTTP semaphore size. */
  maxActive?: number;
}

function entryKey(pr: PRIdentity, defaultRepoSlug: string): string {
  return `${pr.repoSlug ?? defaultRepoSlug}#${pr.number}`;
}

function formatRefreshError(prefix: string, error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  return `${prefix}: ${message.split("\n")[0]}`;
}

export function createRefreshQueue(
  deps: RefreshQueueDeps,
): readonly [RefreshQueueState, RefreshQueueActions] {
  const { defaultRepoSlug, runRefresh, setStatusMessage } = deps;
  const maxActive = deps.maxActive ?? GITHUB_HTTP_MAX_CONCURRENT_REQUESTS;

  // Authoritative queue lives in this imperative Map so the synchronous pump
  // loop sees its own writes immediately. The version signal is bumped after
  // every mutation so reactive consumers (refreshStateForPr inside a memo or
  // effect) re-read.
  const entries = new Map<string, Entry>();
  const [version, setVersion] = createSignal(0);
  const bumpVersion = () => setVersion((n) => n + 1);
  let nextOrder = 0;
  let activeCount = 0;

  function nextQueued(): Entry | undefined {
    let best: Entry | undefined;
    for (const entry of entries.values()) {
      if (entry.phase !== "queued") continue;
      if (
        !best ||
        entry.priority < best.priority ||
        (entry.priority === best.priority && entry.order < best.order)
      ) {
        best = entry;
      }
    }
    return best;
  }

  function pump(): void {
    while (activeCount < maxActive) {
      const next = nextQueued();
      if (!next) return;

      activeCount++;
      next.phase = "refreshing";
      bumpVersion();

      const item: QueueItem = {
        key: next.key,
        pr: next.pr,
        priority: next.priority,
        includeFiles: next.includeFiles,
      };

      void runRefresh(item)
        .catch((error: unknown) => {
          setStatusMessage({
            text: formatRefreshError(`refresh failed for #${next.pr.number}`, error),
            kind: "error",
          });
        })
        .finally(() => {
          activeCount--;
          entries.delete(next.key);
          bumpVersion();
          pump();
        });
    }
  }

  const actions: RefreshQueueActions = {
    queuePrRefresh(pr, options) {
      const repoSlug = pr.repoSlug ?? defaultRepoSlug;
      const key = entryKey({ ...pr, repoSlug }, defaultRepoSlug);

      const existing = entries.get(key);
      if (existing?.phase === "refreshing") return;

      const nextPriority: RefreshPriority = existing
        ? (Math.min(existing.priority, options.priority) as RefreshPriority)
        : options.priority;
      const nextIncludeFiles = options.includeFiles || existing?.includeFiles === true;

      if (
        existing &&
        existing.priority === nextPriority &&
        existing.includeFiles === nextIncludeFiles
      ) {
        return;
      }

      entries.set(key, {
        key,
        pr: { number: pr.number, repoSlug },
        phase: "queued",
        priority: nextPriority,
        order: existing?.order ?? nextOrder++,
        includeFiles: nextIncludeFiles,
      });
      bumpVersion();
      queueMicrotask(() => pump());
    },
  };

  const state: RefreshQueueState = {
    refreshStateForPr(pr) {
      version();
      return entries.get(entryKey(pr, defaultRepoSlug))?.phase;
    },
  };

  return [state, actions] as const;
}
