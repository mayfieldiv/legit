/**
 * Detail-view state machine. Replaces the four independent signals
 * (`detailComments`, `detailLoading`, `detailError`, `detailRefreshKey`)
 * that previously coexisted in `AppInner` with a single discriminated
 * union — illegal combinations like `loading=true && error="..."` are
 * unrepresentable.
 *
 * Lifecycle:
 *   - `detailPr` undefined          → kind: "idle"
 *   - `detailPr` set, fetch pending → kind: "loading"
 *   - fetch resolves                 → kind: "ready" (carries comments)
 *   - fetch rejects                  → kind: "error" (also reports via setStatusMessage)
 *   - `detailPr` changes mid-flight  → in-flight fetch aborted, new fetch starts
 *   - `refresh()` while in any state  → re-runs fetch for the current pr
 */

import { createSignal, type Accessor } from "solid-js";
import { createAbortableAsyncEffect } from "./create-abortable-async-effect";
import type { PRIdentity } from "./pr-identity";
import type { PRDetail, FullReviewThread, IssueComment } from "./types";
import type { StatusMessage } from "./ui-state";

export interface DetailFetchResult {
  pr: PRDetail;
  threads: FullReviewThread[];
  comments: IssueComment[];
}

export type DetailViewState =
  | { kind: "idle" }
  | { kind: "loading"; pr: PRIdentity }
  | { kind: "ready"; pr: PRIdentity; comments: IssueComment[] }
  | { kind: "error"; pr: PRIdentity; error: Error };

export interface DetailActions {
  /** Re-run the fetch for the currently focused detail PR. No-op when idle. */
  refresh(): void;
}

export interface DetailStateDeps {
  /** Reactive source — emits the focused PR identity, or undefined for list view. */
  detailPr: Accessor<PRIdentity | undefined>;
  /** Async fetcher for the detail PR. Must honour the abort signal. */
  fetch: (pr: PRIdentity, signal: AbortSignal) => Promise<DetailFetchResult>;
  /** Hook called on successful fetch before the state transitions to "ready"
   *  — used to commit the freshly-fetched PR + threads to the query cache. */
  onFetched?: (pr: PRIdentity, result: DetailFetchResult) => void;
  /** Used to surface fetch errors as a transient status message. */
  setStatusMessage: (msg: StatusMessage | null) => void;
}

export function createDetailState(
  deps: DetailStateDeps,
): readonly [Accessor<DetailViewState>, DetailActions] {
  const { detailPr, fetch, onFetched, setStatusMessage } = deps;

  const [state, setState] = createSignal<DetailViewState>({ kind: "idle" });
  const [refreshKey, setRefreshKey] = createSignal(0);

  createAbortableAsyncEffect(
    () => ({ pr: detailPr(), refreshKey: refreshKey() }),
    async ({ pr }, signal, isCurrent) => {
      if (!pr) {
        setState({ kind: "idle" });
        return;
      }

      setState({ kind: "loading", pr });
      const result = await fetch(pr, signal);
      if (!isCurrent()) return;

      onFetched?.(pr, result);
      setState({ kind: "ready", pr, comments: result.comments });
    },
    (error, value) => {
      const pr = value.pr;
      const err = error instanceof Error ? error : new Error(String(error));
      if (pr) {
        setState({ kind: "error", pr, error: err });
      } else {
        setState({ kind: "idle" });
      }
      setStatusMessage({
        text: `detail fetch failed: ${err.message}`,
        kind: "error",
      });
    },
  );

  const actions: DetailActions = {
    refresh() {
      setRefreshKey((n) => n + 1);
    },
  };

  return [state, actions] as const;
}
