/**
 * Per-PR enrichment queries (review threads, check runs, reviews).
 *
 * All three queries are gated on the parent `enrichmentReady` flag from
 * `createPRQueries` so a not-yet-settled streaming index doesn't flood
 * the HTTP concurrency semaphore with hundreds of enrichment requests.
 *
 * Must be instantiated inside a Solid component scope under a
 * QueryClientProvider — uses `useQueries` and `createMemo`.
 */

import { createMemo, type Accessor } from "solid-js";
import type { QueryClient } from "@tanstack/solid-query";
import { useQueriesLite as useQueries } from "./use-queries-lite";
import type { Legit } from "./legit";
import type { PRIdentity } from "./pr-identity";
import type { PR, CheckRun, Review, FullReviewThread } from "./types";

export interface EnrichmentQueriesState {
  threadsForPr(pr: PRIdentity): FullReviewThread[] | undefined;
  checksForPr(pr: PR): CheckRun[] | undefined;
  reviewsForPr(pr: PRIdentity): Review[] | undefined;
}

export interface EnrichmentQueriesDeps {
  app: Legit;
  queryClient: QueryClient;
  visiblePRs: Accessor<PR[]>;
  enrichmentReady: Accessor<boolean>;
}

function checksLookupKey(repo: string, headCommitSha: string): string {
  return JSON.stringify([repo, headCommitSha]);
}

export function createEnrichmentQueries(
  deps: EnrichmentQueriesDeps,
): readonly [EnrichmentQueriesState] {
  const { app, queryClient, visiblePRs, enrichmentReady } = deps;

  const threadsQueries = useQueries<FullReviewThread[]>(() => ({
    queries: visiblePRs().map((pr) => {
      const repo = pr.repoSlug ?? app.repoSlug;
      return {
        queryKey: ["threads", repo, pr.number] as const,
        queryFn: async ({ signal }: { signal: AbortSignal }) =>
          app.fetchFullReviewThreads(repo, pr.number, signal),
        enabled: enrichmentReady(),
      };
    }),
  }));

  const threadsByKey = createMemo(() => {
    const prs = visiblePRs();
    const map = new Map<string, FullReviewThread[]>();
    for (let i = 0; i < prs.length; i++) {
      void threadsQueries[i]?.dataUpdatedAt;
      const pr = prs[i]!;
      const repo = pr.repoSlug ?? app.repoSlug;
      const data = queryClient.getQueryData<FullReviewThread[]>(["threads", repo, pr.number]);
      if (data) map.set(`${repo}#${pr.number}`, data);
    }
    return map;
  });

  const uniqueChecks = createMemo(() => {
    const checks = new Map<
      string,
      { key: string; repo: string; headCommitSha: string; enabled: boolean }
    >();

    for (const pr of visiblePRs()) {
      const repo = pr.repoSlug ?? app.repoSlug;
      const headCommitSha = pr.headCommitSha;
      if (!headCommitSha) continue;

      const key = checksLookupKey(repo, headCommitSha);
      if (checks.has(key)) continue;

      checks.set(key, {
        key,
        repo,
        headCommitSha,
        enabled: enrichmentReady(),
      });
    }

    return Array.from(checks.values());
  });

  const checksQueries = useQueries<CheckRun[]>(() => ({
    queries: uniqueChecks().map(({ repo, headCommitSha, enabled }) => ({
      queryKey: ["checks", repo, headCommitSha] as const,
      queryFn: async ({ signal }: { signal: AbortSignal }) =>
        app.fetchCheckRuns(repo, headCommitSha, signal),
      enabled,
    })),
  }));

  const checksByKey = createMemo(() => {
    const map = new Map<string, CheckRun[] | undefined>();
    const checks = uniqueChecks();
    for (let i = 0; i < checks.length; i++) {
      map.set(checks[i]!.key, checksQueries[i]?.data);
    }
    return map;
  });

  const reviewsQueries = useQueries<Review[]>(() => ({
    queries: visiblePRs().map((pr) => {
      const repo = pr.repoSlug ?? app.repoSlug;
      return {
        queryKey: ["reviews", repo, pr.number] as const,
        queryFn: async ({ signal }: { signal: AbortSignal }) =>
          app.fetchReviews(repo, pr.number, signal),
        enabled: enrichmentReady(),
      };
    }),
  }));

  const reviewsByKey = createMemo(() => {
    const prs = visiblePRs();
    const map = new Map<string, Review[]>();
    for (let i = 0; i < prs.length; i++) {
      void reviewsQueries[i]?.dataUpdatedAt;
      const pr = prs[i]!;
      const repo = pr.repoSlug ?? app.repoSlug;
      const data = queryClient.getQueryData<Review[]>(["reviews", repo, pr.number]);
      if (data) map.set(`${repo}#${pr.number}`, data);
    }
    return map;
  });

  const state: EnrichmentQueriesState = {
    threadsForPr(pr: PRIdentity): FullReviewThread[] | undefined {
      const repo = pr.repoSlug ?? app.repoSlug;
      return threadsByKey().get(`${repo}#${pr.number}`);
    },
    checksForPr(pr: PR): CheckRun[] | undefined {
      if (!pr.headCommitSha) return [];
      const repo = pr.repoSlug ?? app.repoSlug;
      return checksByKey().get(checksLookupKey(repo, pr.headCommitSha));
    },
    reviewsForPr(pr: PRIdentity): Review[] | undefined {
      const repo = pr.repoSlug ?? app.repoSlug;
      return reviewsByKey().get(`${repo}#${pr.number}`);
    },
  };

  return [state] as const;
}
