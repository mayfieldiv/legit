/**
 * Owns the streamed PR index, per-PR detail cache fan-out, and the
 * derived state that downstream consumers (selection, summary panel,
 * detail view, refresh queue) read from. Repo-tab membership lives
 * here too so refresh-all can reset the tab list atomically with the
 * underlying index queries.
 *
 * Must be instantiated inside a Solid component scope under a
 * QueryClientProvider — uses `useQueries`, `createMemo`, and
 * `createEffect`.
 */

import { createSignal, createMemo, createEffect, type Accessor } from "solid-js";
import {
  experimental_streamedQuery as streamedQuery,
  type QueryClient,
} from "@tanstack/solid-query";
import { useQueriesLite as useQueries } from "./use-queries-lite";
import type { Legit } from "./legit";
import type { PR, PRDetail } from "./types";

/** Lightweight index entry stored in `["pr-index", repo]`. Per-repo data is
 *  seeded into `["pr", repo, number]` caches by the streamed index query. */
export interface PRIndexEntry {
  number: number;
  createdAt: string;
  repoSlug: string;
}

export interface PRQueriesState {
  readonly repoTabs: string[];
  readonly visibleIndex: PRIndexEntry[];
  readonly visiblePRs: PR[];
  readonly prByKey: Map<string, PRDetail>;
  readonly loading: boolean;
  readonly error: string;
  readonly enrichmentReady: boolean;
  readonly settledRepos: Set<string>;
}

export interface PRQueriesActions {
  setRepoTabs(repos: string[]): void;
  prunePrIndexIfClosed(repo: string, pr: Pick<PR, "number" | "state">): void;
  /** Called after a per-PR refresh executes. Clears the mergeable-retry
   *  cooldown so a future settled pass can re-schedule the UNKNOWN retry. */
  notePrRefreshed(repo: string, number: number): void;
}

export interface PRQueriesDeps {
  app: Legit;
  queryClient: QueryClient;
  /** The currently focused tab (0 = "All", 1+ = repo tabs). Reactive. */
  activeTab: Accessor<number>;
}

function sameStringSet(a: Set<string> | undefined, b: Set<string> | undefined): boolean {
  if (a === b) return true;
  if (!a || !b) return a === b;
  if (a.size !== b.size) return false;
  for (const value of a) {
    if (!b.has(value)) return false;
  }
  return true;
}

export function createPRQueries(deps: PRQueriesDeps): readonly [PRQueriesState, PRQueriesActions] {
  const { app, queryClient, activeTab } = deps;

  const [repoTabs, setRepoTabsSignal] = createSignal<string[]>(app.trackedRepos());

  const prIndexQueries = useQueries<PRIndexEntry[]>(() => ({
    queries: repoTabs().map((repo) => ({
      queryKey: ["pr-index", repo] as const,
      queryFn: streamedQuery({
        streamFn: ({ signal }: { signal: AbortSignal }) => app.fetchPRs(repo, signal),
        reducer: (_prev: PRIndexEntry[], snapshot: PR[]): PRIndexEntry[] => {
          for (const pr of snapshot) {
            const slug = pr.repoSlug ?? repo;
            queryClient.setQueryData<PRDetail>(["pr", slug, pr.number], (prev) => ({
              body: "",
              ...(prev ?? {}),
              ...pr,
              repoSlug: slug,
            }));
          }
          return snapshot.map((pr) => ({
            number: pr.number,
            createdAt: pr.createdAt,
            repoSlug: pr.repoSlug ?? repo,
          }));
        },
        initialValue: [] as PRIndexEntry[],
        // Refresh-all routes through queryClient.invalidateQueries, which
        // re-runs this streamFn. `replace` keeps the previously-cached
        // index visible until the new stream finishes — matches the
        // pre-refactor behaviour where a separate refreshRepoIndex helper
        // accumulated the snapshot before writing.
        refetchMode: "replace",
      }),
    })),
  }));

  /** Index entries for the currently visible tab (merged & sorted on "All"). */
  const visibleIndex = createMemo<PRIndexEntry[]>(() => {
    const tab = activeTab();
    const repos = repoTabs();
    if (tab === 0) {
      const merged: PRIndexEntry[] = [];
      for (let i = 0; i < repos.length; i++) {
        const data = prIndexQueries[i]?.data ?? [];
        merged.push(...data);
      }
      merged.sort((a, b) => new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime());
      return merged;
    }
    const repo = repos[tab - 1];
    if (!repo) return [];
    const idx = repos.indexOf(repo);
    return prIndexQueries[idx]?.data ?? [];
  });

  /** Drop a merged/closed PR from the repo's pr-index so the list re-renders
   *  without it. No-op when the PR is open or already absent — returning the
   *  same reference keeps setQueryData from notifying observers. */
  function prunePrIndexIfClosed(repo: string, pr: Pick<PR, "number" | "state">): void {
    if (pr.state === "OPEN") return;
    queryClient.setQueryData<PRIndexEntry[]>(["pr-index", repo], (prev) => {
      if (!prev || !prev.some((e) => e.number === pr.number)) return prev;
      return prev.filter((e) => e.number !== pr.number);
    });
  }

  // Fan-out per-PR queries over the visible index. Initial hydration comes
  // from the streamed list seeding the cache via setQueryData, so these
  // queryFns only fire when an entry is explicitly invalidated (e.g. `r`).
  // The queryFn stamps `repoSlug` onto the fetched PRDetail so cache reads
  // by repoSlug-keyed lookups (threads, reviews, checks) stay correct after
  // a refetch — `legit.fetchPR` returns a PR without repoSlug.
  const prQueries = useQueries<PRDetail>(() => ({
    queries: visibleIndex().map(({ repoSlug, number }) => ({
      queryKey: ["pr", repoSlug, number] as const,
      queryFn: async ({ signal }: { signal: AbortSignal }) => {
        const next = await app.fetchPR(repoSlug, number, signal);
        const pr = { ...next, repoSlug };
        prunePrIndexIfClosed(repoSlug, pr);
        return pr;
      },
      staleTime: Infinity,
    })),
  }));

  /** Map of "repoSlug#number" → PRDetail for the current tab's visible PRs.
   *  Built by iterating the source signal (visibleIndex) positionally and
   *  reading the cache; tracking `prQueries[i].dataUpdatedAt` along the way
   *  is the only reliable way to propagate Solid Store updates (see commit
   *  048eb22). All consumers — visiblePRs, cachedPr — derive from this. */
  const prByKey = createMemo(() => {
    const index = visibleIndex();
    const map = new Map<string, PRDetail>();
    for (let i = 0; i < index.length; i++) {
      void prQueries[i]?.dataUpdatedAt;
      const entry = index[i]!;
      const data = queryClient.getQueryData<PRDetail>(["pr", entry.repoSlug, entry.number]);
      if (data) map.set(`${entry.repoSlug}#${entry.number}`, data);
    }
    return map;
  });

  /** PRs for the current tab, ordered by visibleIndex. */
  const visiblePRs = createMemo<PR[]>(() => {
    const map = prByKey();
    const prs: PR[] = [];
    for (const entry of visibleIndex()) {
      const pr = map.get(`${entry.repoSlug}#${entry.number}`);
      if (pr) prs.push(pr);
    }
    return prs;
  });

  /** True while any repo's index is still pending (no data yet). */
  const loading = createMemo(() => prIndexQueries.some((q) => q.isPending));

  /** First error message, if any. */
  const prError = createMemo(() => {
    for (const q of prIndexQueries) {
      if (q.error) return (q.error as Error).message ?? String(q.error);
    }
    return "";
  });

  /**
   * Repos whose PR streamedQuery generator has finished. Until a repo
   * settles, its PRs must NOT trigger enrichment — otherwise hundreds of
   * per-PR queries flood the concurrency semaphore and starve the
   * still-running PR-list pagination & reviewStatus batches.
   */
  const settledRepos = createMemo(
    () => {
      const settled = new Set<string>();
      const repos = repoTabs();
      for (let i = 0; i < repos.length; i++) {
        const q = prIndexQueries[i];
        if (q && !q.isFetching) settled.add(repos[i]!);
      }
      return settled;
    },
    undefined,
    {
      equals: sameStringSet,
    },
  );

  /**
   * Delay list-view enrichment until the current tab's base PR set is stable.
   * On single-repo tabs this matches that repo's settled state. On the "All"
   * tab we wait for every tracked repo to finish streaming before threads,
   * reviews, and checks start reshaping smart-status groups.
   */
  const enrichmentReady = createMemo(() => {
    const tab = activeTab();
    if (tab === 0) {
      const repos = repoTabs();
      return repos.length > 0 && repos.every((repo) => settledRepos().has(repo));
    }

    const repo = repoTabs()[tab - 1];
    return !!repo && settledRepos().has(repo);
  });

  // ── Retry UNKNOWN mergeable status after settlement ─────────────────
  // GitHub computes mergeability lazily — the initial fetch triggers
  // background computation but returns UNKNOWN. Once the index settles
  // for a repo, schedule a delayed per-PR re-fetch for any UNKNOWN entries.
  // Keyed (repo, number) so each PR retries independently and at most once.
  const mergeableRetried = new Set<string>();
  createEffect(
    () => settledRepos(),
    (settled) => {
      const timers: ReturnType<typeof setTimeout>[] = [];
      const repos = repoTabs();
      for (let i = 0; i < repos.length; i++) {
        const repo = repos[i]!;
        if (!settled.has(repo)) continue;
        const entries = prIndexQueries[i]?.data ?? [];
        for (const entry of entries) {
          const key = `${entry.repoSlug}:${entry.number}`;
          if (mergeableRetried.has(key)) continue;
          const data = queryClient.getQueryData<PRDetail>(["pr", entry.repoSlug, entry.number]);
          if (data?.mergeable !== "UNKNOWN") continue;
          mergeableRetried.add(key);
          const timer = setTimeout(() => {
            void queryClient.invalidateQueries({
              queryKey: ["pr", entry.repoSlug, entry.number],
            });
          }, 3_000);
          timers.push(timer);
        }
      }
      return () => {
        for (const timer of timers) clearTimeout(timer);
      };
    },
  );

  const state: PRQueriesState = {
    get repoTabs() {
      return repoTabs();
    },
    get visibleIndex() {
      return visibleIndex();
    },
    get visiblePRs() {
      return visiblePRs();
    },
    get prByKey() {
      return prByKey();
    },
    get loading() {
      return loading();
    },
    get error() {
      return prError();
    },
    get enrichmentReady() {
      return enrichmentReady();
    },
    get settledRepos() {
      return settledRepos();
    },
  };

  const actions: PRQueriesActions = {
    setRepoTabs(repos: string[]): void {
      setRepoTabsSignal(repos);
    },
    prunePrIndexIfClosed,
    notePrRefreshed(repo: string, number: number): void {
      mergeableRetried.delete(`${repo}:${number}`);
    },
  };

  return [state, actions] as const;
}
