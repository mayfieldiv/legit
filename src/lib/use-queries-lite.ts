/**
 * Typed wrapper around @tanstack/solid-query's useQueries.
 *
 * The v6 beta's useQueries types infer `{}` instead of the result array
 * when no `combine` function is provided. This wrapper casts through the
 * correct return type while delegating entirely to the upstream
 * implementation (which uses createStore + reconcile for fine-grained
 * reactivity — each query result property is tracked independently).
 */

import type { QueryObserverOptions } from "@tanstack/query-core";
import { useQueries as upstreamUseQueries } from "@tanstack/solid-query";
import type { QueryClient, UseQueryResult } from "@tanstack/solid-query";
import type { Accessor } from "solid-js";

export function useQueriesLite<TData = unknown, TError = Error>(
  queriesOptions: Accessor<{
    queries: readonly QueryObserverOptions[];
  }>,
  queryClient?: Accessor<QueryClient>,
): UseQueryResult<TData, TError>[] {
  return upstreamUseQueries(queriesOptions, queryClient) as unknown as UseQueryResult<
    TData,
    TError
  >[];
}
