/**
 * Drop-in replacement for @tanstack/solid-query's useQueries that avoids
 * the O(N) unwrap-per-query-resolution performance issue.
 *
 * The upstream useQueries (v5) calls `unwrap()` on every query result in
 * the array whenever *any* single query resolves, causing O(N²) deep proxy
 * traversals during the enrichment burst. This wrapper replaces that with
 * `reconcile()`, which structurally diffs and only touches store nodes
 * that actually changed — matching the approach used in the v6 rewrite.
 */

import { QueriesObserver, noop } from "@tanstack/query-core";
import type { QueryObserverOptions, QueryObserverResult } from "@tanstack/query-core";
import { useQueryClient } from "@tanstack/solid-query";
import type { QueryClient, UseQueryResult } from "@tanstack/solid-query";
import type { Accessor } from "solid-js";
import {
  createMemo,
  createComputed,
  onCleanup,
  onMount,
  createStore,
  reconcile,
} from "./solid-compat";

/**
 * Lightweight useQueries using reconcile() instead of per-element unwrap().
 *
 * Type signature mirrors @tanstack/solid-query useQueries — accepts the same
 * options accessor and returns a store array of query results.
 */
type IndexedQueryObserverResult = QueryObserverResult & {
  __queryIndex: number;
};

function withQueryIndex(results: readonly QueryObserverResult[]): IndexedQueryObserverResult[] {
  return results.map((result, index) => ({
    ...result,
    __queryIndex: index,
  }));
}

export function useQueriesLite<TData = unknown, TError = Error>(
  queriesOptions: Accessor<{
    queries: readonly QueryObserverOptions[];
  }>,
  queryClient?: Accessor<QueryClient>,
): UseQueryResult<TData, TError>[] {
  const client = createMemo(() => {
    if (queryClient) return queryClient();
    return useQueryClient();
  });

  const defaultedQueries = createMemo(() =>
    queriesOptions().queries.map((options) => ({
      ...(client().defaultQueryOptions(options) as QueryObserverOptions<
        unknown,
        Error,
        unknown,
        unknown,
        readonly unknown[],
        never
      >),
      _optimisticResults: "optimistic" as const,
    })),
  );

  const observer = new QueriesObserver(client(), defaultedQueries());

  // Get initial optimistic result
  const [, getCombinedResult] = observer.getOptimisticResult(defaultedQueries(), undefined);

  const [state, setState] = createStore<IndexedQueryObserverResult[]>(
    withQueryIndex(getCombinedResult()),
  );

  // Subscribe to observer updates. Use reconcile() for O(changed) diffing
  // instead of the upstream's O(all) unwrap loop.
  let unsubscribe: () => void = noop;
  let taskQueue: Array<() => void> = [];

  const subscribeToObserver = () =>
    observer.subscribe((result) => {
      taskQueue.push(() => {
        setState(reconcile(withQueryIndex(result), "__queryIndex"));
      });
      queueMicrotask(() => {
        const taskToRun = taskQueue.pop();
        if (taskToRun) taskToRun();
        taskQueue = [];
      });
    });

  createComputed<() => void>((cleanup) => {
    cleanup?.();
    unsubscribe = subscribeToObserver();
    return () => queueMicrotask(unsubscribe);
  });
  onCleanup(unsubscribe);

  // Sync observer when query options change
  onMount(() => {
    observer.setQueries(defaultedQueries());
  });

  createComputed(() => {
    observer.setQueries(defaultedQueries());
  });

  return state as unknown as UseQueryResult<TData, TError>[];
}
