/**
 * Drop-in replacement for @tanstack/solid-query's useQueries that avoids
 * the O(N) unwrap-per-query-resolution performance issue.
 *
 * The upstream useQueries (v5) calls `unwrap()` on every query result in
 * the array whenever *any* single query resolves, causing O(N²) deep proxy
 * traversals during the enrichment burst. This wrapper uses a signal-backed
 * array proxy with microtask-coalesced updates instead, avoiding per-element
 * unwrap overhead entirely.
 */

import { QueriesObserver, noop } from "@tanstack/query-core";
import type { QueryObserverOptions, QueryObserverResult } from "@tanstack/query-core";
import { useQueryClient } from "@tanstack/solid-query";
import type { QueryClient, UseQueryResult } from "@tanstack/solid-query";
import { createMemo, createEffect, onCleanup, createSignal, type Accessor } from "solid-js";

/**
 * Lightweight useQueries backed by a signal + microtask coalescing.
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

  // QueriesObserver fires on every internal state transition (not just data
  // changes). The signal uses structural equality on the query-observable
  // surface (data, status, fetchStatus) so that no-op observer notifications
  // don't cascade through the reactive graph — without this, downstream
  // memos that produce new array references (e.g. allPRs sorting) create
  // an infinite microtask loop.
  const queryResultsEqual = (
    prev: IndexedQueryObserverResult[],
    next: IndexedQueryObserverResult[],
  ): boolean =>
    prev.length === next.length &&
    prev.every((p, i) => {
      const n = next[i]!;
      return p.data === n.data && p.status === n.status && p.fetchStatus === n.fetchStatus;
    });

  const [state, setState] = createSignal<IndexedQueryObserverResult[]>(
    withQueryIndex(getCombinedResult()),
    { equals: queryResultsEqual },
  );

  // Subscribe to observer updates. Use a signal-backed array proxy instead of
  // Solid 2 store reconciliation, which is still unstable under the current
  // terminal renderer/query workload.
  let unsubscribe: () => void = noop;
  let queued = false;
  let latestResult = state();

  const subscribeToObserver = () =>
    observer.subscribe((result) => {
      latestResult = withQueryIndex(result);
      if (queued) return;
      queued = true;
      queueMicrotask(() => {
        queued = false;
        setState(latestResult);
      });
    });

  unsubscribe = subscribeToObserver();
  onCleanup(() => unsubscribe());

  // Sync observer when query options change.
  createEffect(
    () => defaultedQueries(),
    (queries) => observer.setQueries(queries),
  );

  const proxy = new Proxy([] as IndexedQueryObserverResult[], {
    get(_target, prop) {
      const current = state();
      const value = Reflect.get(current, prop, current);
      return typeof value === "function" ? value.bind(current) : value;
    },
    has(_target, prop) {
      return prop in state();
    },
    ownKeys() {
      return Reflect.ownKeys(state());
    },
    getOwnPropertyDescriptor(_target, prop) {
      const descriptor = Reflect.getOwnPropertyDescriptor(state(), prop);
      if (!descriptor) return undefined;
      return { ...descriptor, configurable: true };
    },
  });

  return proxy as unknown as UseQueryResult<TData, TError>[];
}
