import { createSignal, createMemo, createEffect, onSettled } from "solid-js";
import type { JSX as OpenTuiJSX } from "@opentui/solid";
import {
  QueryClient,
  QueryClientProvider,
  useIsFetching,
  experimental_streamedQuery as streamedQuery,
} from "@tanstack/solid-query";
import { useQueriesLite as useQueries } from "./lib/use-queries-lite";
import { createAbortableAsyncEffect } from "./lib/create-abortable-async-effect";
import { samePr, prKey, type PRIdentity } from "./lib/pr-identity";
import { AppShell } from "./components/AppShell";
import { createUIState } from "./lib/ui-state";
import type { Legit } from "./lib/legit";
import { GITHUB_HTTP_MAX_CONCURRENT_REQUESTS, type GitHubNetworkStats } from "./lib/concurrency";
import type {
  PR,
  PRDetail,
  CheckRun,
  Review,
  FullReviewThread,
  IssueComment,
  FileCategorization,
} from "./lib/types";
import { derivePRState, type PRDerivedState, type WorktreeInfo } from "./lib/pr-state";
import { createWorktreeController } from "./lib/worktree-controller";
import { createBrowserActions } from "./lib/browser-actions";

export { prUrl, devinUrl } from "./lib/browser-actions";

function sameStringSet(a: Set<string> | undefined, b: Set<string> | undefined): boolean {
  if (a === b) return true;
  if (!a || !b) return a === b;
  if (a.size !== b.size) return false;
  for (const value of a) {
    if (!b.has(value)) return false;
  }
  return true;
}

function checksLookupKey(repo: string, headCommitSha: string): string {
  return JSON.stringify([repo, headCommitSha]);
}

/** Lightweight index entry stored in ["pr-index", repo]. Per-repo data is
 *  seeded into ["pr", repo, number] caches by the streamed index query. */
interface PRIndexEntry {
  number: number;
  createdAt: string;
  repoSlug: string;
}

export interface AppProps {
  app: Legit;
}

function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        staleTime: Infinity,
        gcTime: Infinity,
        refetchOnWindowFocus: false,
        refetchOnReconnect: true,
        retry: 1,
      },
    },
  });
}

function OtuiQueryClientProvider(props: {
  client: QueryClient;
  children: OpenTuiJSX.Element;
}): OpenTuiJSX.Element {
  return QueryClientProvider(props as never) as OpenTuiJSX.Element;
}

export function App(props: AppProps) {
  const queryClient = createQueryClient();

  return (
    <OtuiQueryClientProvider client={queryClient}>
      <AppInner app={props.app} queryClient={queryClient} />
    </OtuiQueryClientProvider>
  );
}

interface AppInnerProps {
  app: Legit;
  queryClient: QueryClient;
}

function AppInner(props: AppInnerProps) {
  const [uiState, uiActions] = createUIState();

  /** HTTP concurrency (Legit fetch wrapper). */
  const [httpNetworkStats, setHttpNetworkStats] = createSignal<GitHubNetworkStats>({
    inFlight: 0,
    waiting: 0,
  });

  /**
   * Reactive count of queries with fetchStatus 'fetching' (TanStack's own hook wires
   * QueryCache → Solid signals; manual subscribe + setSignal did not update the TUI reliably).
   */
  const fetchingQueryCount = useIsFetching(undefined, () => props.queryClient);

  /** In-flight = HTTP layer; waiting = other fetching queries not yet in an HTTP slot (see concurrency.ts). */
  const githubNetworkStatsForBar = createMemo<GitHubNetworkStats>(() => {
    const h = httpNetworkStats();
    const f = fetchingQueryCount();
    return {
      inFlight: h.inFlight,
      waiting: Math.max(0, f - h.inFlight),
    };
  });

  onSettled(() => {
    setHttpNetworkStats(props.app.githubNetworkStats);
    const unsubHttp = props.app.subscribeGitHubNetworkStats(() => {
      setHttpNetworkStats(props.app.githubNetworkStats);
    });
    return unsubHttp;
  });

  // ── Repo tabs ─────────────────────────────────────────────────────────
  const [repoTabs, setRepoTabs] = createSignal<string[]>(props.app.trackedRepos());

  const tabs = createMemo(() => ["All", ...repoTabs()]);

  const showRepo = createMemo(() => uiState.activeTab === 0 && repoTabs().length > 1);

  // ── PR index + per-PR cache ───────────────────────────────────────────
  // The authoritative store for PR-shaped data is ["pr", repo, number].
  // Per-repo streamed index queries seed those entries; downstream code
  // reads PRs exclusively through the per-PR cache via `prQueries` below.
  const prIndexQueries = useQueries<PRIndexEntry[]>(() => ({
    queries: repoTabs().map((repo) => ({
      queryKey: ["pr-index", repo] as const,
      queryFn: streamedQuery({
        streamFn: ({ signal }: { signal: AbortSignal }) => props.app.fetchPRs(repo, signal),
        reducer: (_prev: PRIndexEntry[], snapshot: PR[]): PRIndexEntry[] => {
          for (const pr of snapshot) {
            const slug = pr.repoSlug ?? repo;
            props.queryClient.setQueryData<PRDetail>(["pr", slug, pr.number], (prev) => ({
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
      }),
    })),
  }));

  /** Index entries for the currently visible tab (merged & sorted on "All"). */
  const visibleIndex = createMemo<PRIndexEntry[]>(() => {
    const tab = uiState.activeTab;
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
    props.queryClient.setQueryData<PRIndexEntry[]>(["pr-index", repo], (prev) => {
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
        const next = await props.app.fetchPR(repoSlug, number, signal);
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
      const data = props.queryClient.getQueryData<PRDetail>(["pr", entry.repoSlug, entry.number]);
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

  // ── Track which repos have finished fetching (generator complete) ─────
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
    const tab = uiState.activeTab;
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
          const data = props.queryClient.getQueryData<PRDetail>([
            "pr",
            entry.repoSlug,
            entry.number,
          ]);
          if (data?.mergeable !== "UNKNOWN") continue;
          mergeableRetried.add(key);
          const timer = setTimeout(() => {
            void props.queryClient.invalidateQueries({
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

  // ── Per-PR enrichment queries (threads, checks, reviews) ──────────────
  const threadsQueries = useQueries<FullReviewThread[]>(() => ({
    queries: visiblePRs().map((pr) => {
      const repo = pr.repoSlug ?? props.app.repoSlug;
      return {
        queryKey: ["threads", repo, pr.number] as const,
        queryFn: async ({ signal }: { signal: AbortSignal }) =>
          props.app.fetchFullReviewThreads(repo, pr.number, signal),
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
      const repo = pr.repoSlug ?? props.app.repoSlug;
      const data = props.queryClient.getQueryData<FullReviewThread[]>(["threads", repo, pr.number]);
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
      const repo = pr.repoSlug ?? props.app.repoSlug;
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
        props.app.fetchCheckRuns(repo, headCommitSha, signal),
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

  const checksForPr = (pr: PR): CheckRun[] | undefined => {
    if (!pr.headCommitSha) return [];
    const repo = pr.repoSlug ?? props.app.repoSlug;
    return checksByKey().get(checksLookupKey(repo, pr.headCommitSha));
  };

  const reviewsQueries = useQueries<Review[]>(() => ({
    queries: visiblePRs().map((pr) => {
      const repo = pr.repoSlug ?? props.app.repoSlug;
      return {
        queryKey: ["reviews", repo, pr.number] as const,
        queryFn: async ({ signal }: { signal: AbortSignal }) =>
          props.app.fetchReviews(repo, pr.number, signal),
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
      const repo = pr.repoSlug ?? props.app.repoSlug;
      const data = props.queryClient.getQueryData<Review[]>(["reviews", repo, pr.number]);
      if (data) map.set(`${repo}#${pr.number}`, data);
    }
    return map;
  });

  const threadsForPr = (pr: PRIdentity): FullReviewThread[] | undefined => {
    const repo = pr.repoSlug ?? props.app.repoSlug;
    return threadsByKey().get(`${repo}#${pr.number}`);
  };

  const reviewsForPr = (pr: PRIdentity): Review[] | undefined => {
    const repo = pr.repoSlug ?? props.app.repoSlug;
    return reviewsByKey().get(`${repo}#${pr.number}`);
  };

  const worktreeController = createWorktreeController({
    app: props.app,
    queryClient: props.queryClient,
    repoTabs,
    setStatusMessage: uiActions.setStatusMessage,
  });
  const { worktreeForPr, createWorktree: handleCreateWorktree } = worktreeController;

  // ── Shared derived PR state lookup ────────────────────────────────────
  const getPRState = (pr: PR): PRDerivedState => {
    const threads = threadsForPr(pr);
    const reviews = reviewsForPr(pr);
    const checks = checksForPr(pr);

    return derivePRState(pr, {
      currentUser: props.app.currentUser,
      loading: threads === undefined || reviews === undefined,
      threads,
      checks: checks ?? [],
      reviews,
      worktree: worktreeForPr(pr),
    });
  };

  // ── Selection state ───────────────────────────────────────────────────
  // Holds only the PR identity. Full PR data (which changes on refresh)
  // is derived on demand from the per-PR cache via `selectedPrDetail` —
  // storing the PR object here would go stale behind samePr equality.
  const [selectedPr, setSelectedPr] = createSignal<PRIdentity | undefined>(undefined, {
    equals: samePr,
  });

  function selectPr(pr: PR) {
    setSelectedPr(prKey(pr));
  }

  function changeTab(index: number) {
    uiActions.changeTab(index);
    setSelectedPr(undefined);
  }

  // ── Summary panel: files query ────────────────────────────────────────
  // Files live in the query cache so a refresh invalidation preserves the
  // last-known data during refetch (no summary flicker), and so the cache
  // remains the single source of truth.
  const filesQueries = useQueries<FileCategorization>(() => {
    const pr = selectedPr();
    if (!pr) return { queries: [] };
    const repo = pr.repoSlug ?? props.app.repoSlug;
    return {
      queries: [
        {
          queryKey: ["files", repo, pr.number] as const,
          queryFn: async ({ signal }: { signal: AbortSignal }) =>
            props.app.fetchCategorizedFiles(repo, pr.number, signal),
          staleTime: Infinity,
        },
      ],
    };
  });

  const filesQueryVersion = createMemo(() =>
    filesQueries
      .map((query) => `${query.status}:${query.fetchStatus}:${query.dataUpdatedAt}`)
      .join("|"),
  );

  const selectedFiles = (): FileCategorization | undefined => {
    filesQueryVersion();
    const pr = selectedPr();
    if (!pr) return undefined;
    const repo = pr.repoSlug ?? props.app.repoSlug;
    return props.queryClient.getQueryData<FileCategorization>(["files", repo, pr.number]);
  };

  // ── Detail view queries ───────────────────────────────────────────────
  const detailPr = () => {
    const v = uiState.view;
    return v.view === "detail" ? v.pr : undefined;
  };

  const [detailComments, setDetailComments] = createSignal<IssueComment[]>([]);
  const [detailLoading, setDetailLoading] = createSignal(false);
  const [detailRefreshKey, setDetailRefreshKey] = createSignal(0);
  createAbortableAsyncEffect(
    () => ({ pr: detailPr(), refreshKey: detailRefreshKey() }),
    async ({ pr }, signal, isCurrent) => {
      setDetailComments([]);

      if (!pr) {
        setDetailLoading(false);
        return;
      }

      const repo = pr.repoSlug ?? props.app.repoSlug;
      setDetailLoading(true);

      const [nextPr, threads, comments] = await Promise.all([
        props.app.fetchPR(repo, pr.number, signal),
        props.app.fetchFullReviewThreads(repo, pr.number, signal),
        props.app.fetchIssueComments(repo, pr.number, signal),
      ]);
      if (!isCurrent()) return;

      props.queryClient.setQueryData<PRDetail>(["pr", repo, pr.number], (prev) => ({
        ...(prev ?? {}),
        ...nextPr,
        repoSlug: repo,
      }));
      props.queryClient.setQueryData(["threads", repo, pr.number], threads);
      prunePrIndexIfClosed(repo, nextPr);
      setDetailComments(comments);
      setDetailLoading(false);
    },
    (error) => {
      setDetailLoading(false);
      uiActions.setStatusMessage({
        text: `detail fetch failed: ${error instanceof Error ? error.message : String(error)}`,
        kind: "error",
      });
    },
  );

  /** Read the freshest PRDetail for a PR identity from the per-PR cache.
   *  Reactive via prByKey so consumers re-render when any visible PR
   *  refetches. Returns undefined for PRs not in the current tab. */
  function cachedPr(pr: PRIdentity | undefined): PRDetail | undefined {
    if (!pr) return undefined;
    const repo = pr.repoSlug ?? props.app.repoSlug;
    return prByKey().get(`${repo}#${pr.number}`);
  }

  const detailPrDetail = (): PRDetail | undefined => cachedPr(detailPr());

  /** Full data for the selected PR, derived from the cache. Memoized since
   *  several render consumers read it each tick. */
  const selectedPrDetail = createMemo<PRDetail | undefined>(() => cachedPr(selectedPr()));

  function selectedPrForRefresh(): PRIdentity | undefined {
    const identity = selectedPr();
    if (identity) {
      const live = cachedPr(identity) ?? visiblePRs().find((pr) => samePr(pr, identity));
      return live ? prKey(live) : identity;
    }
    const firstVisible = visiblePRs()[0];
    return firstVisible ? prKey(firstVisible) : undefined;
  }

  /** Detail view threads read from the threads cache; reactive via threadsByKey. */
  const detailThreads = (): FullReviewThread[] | undefined => {
    const pr = detailPr();
    if (!pr) return undefined;
    return threadsForPr(pr);
  };

  // ── Refresh handlers ──────────────────────────────────────────────────
  type RefreshPhase = "queued" | "refreshing";
  type RefreshPriority = 0 | 1 | 2 | 3 | 4;

  interface QueuedRefresh {
    repo: string;
    number: number;
    phase: RefreshPhase;
    priority: RefreshPriority;
    order: number;
    includeFiles: boolean;
  }

  const [queuedRefreshes, setQueuedRefreshes] = createSignal<Map<string, QueuedRefresh>>(new Map());
  let nextRefreshOrder = 0;
  let activeRefreshes = 0;
  const activeRepoRefreshes = new Set<string>();
  // Keep the app-level refresh queue aligned with the shared HTTP semaphore so
  // bulk refreshes can fully saturate available request capacity.
  const MAX_ACTIVE_REFRESHES = GITHUB_HTTP_MAX_CONCURRENT_REQUESTS;

  function refreshKey(pr: PRIdentity): string {
    return `${pr.repoSlug ?? props.app.repoSlug}#${pr.number}`;
  }

  function refreshPriorityForPr(pr: PR): RefreshPriority {
    const tier = getPRState(pr).smartStatus?.key;
    switch (tier) {
      case "me-blocking":
        return 1;
      case "needs-review":
        return 2;
      case "waiting-on-author":
        return 3;
      default:
        return 4;
    }
  }

  function refreshStateForPr(pr: PRIdentity): RefreshPhase | undefined {
    return queuedRefreshes().get(refreshKey(pr))?.phase;
  }

  function formatRefreshError(prefix: string, error: unknown): string {
    const message = error instanceof Error ? error.message : String(error);
    return `${prefix}: ${message.split("\n")[0]}`;
  }

  function nextQueuedRefresh(): QueuedRefresh | undefined {
    let next: QueuedRefresh | undefined;
    for (const refresh of queuedRefreshes().values()) {
      if (refresh.phase !== "queued") continue;
      if (
        !next ||
        refresh.priority < next.priority ||
        (refresh.priority === next.priority && refresh.order < next.order)
      ) {
        next = refresh;
      }
    }
    return next;
  }

  function setRefreshPhase(key: string, phase: RefreshPhase): void {
    setQueuedRefreshes((prev) => {
      const current = prev.get(key);
      if (!current || current.phase === phase) return prev;
      const next = new Map(prev);
      next.set(key, { ...current, phase });
      return next;
    });
  }

  function clearQueuedRefresh(key: string): void {
    setQueuedRefreshes((prev) => {
      if (!prev.has(key)) return prev;
      const next = new Map(prev);
      next.delete(key);
      return next;
    });
  }

  function queuePrRefresh(
    pr: PRIdentity,
    options: { priority: RefreshPriority; includeFiles: boolean },
  ): void {
    const repo = pr.repoSlug ?? props.app.repoSlug;
    const key = refreshKey({ ...pr, repoSlug: repo });
    let changed = false;

    setQueuedRefreshes((prev) => {
      const existing = prev.get(key);
      if (existing?.phase === "refreshing") {
        return prev;
      }

      const next = new Map(prev);
      const nextRefresh: QueuedRefresh = {
        repo,
        number: pr.number,
        phase: "queued",
        priority: existing
          ? (Math.min(existing.priority, options.priority) as RefreshPriority)
          : options.priority,
        order: existing?.order ?? nextRefreshOrder++,
        includeFiles: options.includeFiles || existing?.includeFiles === true,
      };

      if (
        existing &&
        existing.priority === nextRefresh.priority &&
        existing.includeFiles === nextRefresh.includeFiles
      ) {
        return prev;
      }

      next.set(key, nextRefresh);
      changed = true;
      return next;
    });

    if (changed) {
      queueMicrotask(() => pumpRefreshQueue());
    }
  }

  async function refreshRepoIndex(repo: string): Promise<void> {
    if (activeRepoRefreshes.has(repo)) return;
    activeRepoRefreshes.add(repo);

    try {
      let latestSnapshot: PR[] = [];
      for await (const snapshot of props.app.fetchPRs(repo)) {
        latestSnapshot = snapshot;
        for (const pr of snapshot) {
          const slug = pr.repoSlug ?? repo;
          props.queryClient.setQueryData<PRDetail>(["pr", slug, pr.number], (prev) => ({
            body: prev?.body ?? "",
            ...(prev ?? {}),
            ...pr,
            repoSlug: slug,
          }));
        }
      }

      props.queryClient.setQueryData<PRIndexEntry[]>(
        ["pr-index", repo],
        latestSnapshot.map((pr) => ({
          number: pr.number,
          createdAt: pr.createdAt,
          repoSlug: pr.repoSlug ?? repo,
        })),
      );
    } catch (error) {
      uiActions.setStatusMessage({
        text: formatRefreshError(`refresh failed for ${repo}`, error),
        kind: "error",
      });
    } finally {
      activeRepoRefreshes.delete(repo);
    }
  }

  async function runQueuedRefresh(refresh: QueuedRefresh): Promise<void> {
    const { repo, number, includeFiles } = refresh;
    mergeableRetried.delete(`${repo}:${number}`);

    const [nextPr, threads, reviews] = await Promise.all([
      props.app.fetchPR(repo, number),
      props.app.fetchFullReviewThreads(repo, number),
      props.app.fetchReviews(repo, number),
    ]);

    props.queryClient.setQueryData<PRDetail>(["pr", repo, number], (prev) => ({
      ...(prev ?? {}),
      ...nextPr,
      repoSlug: repo,
    }));
    props.queryClient.setQueryData(["threads", repo, number], threads);
    props.queryClient.setQueryData(["reviews", repo, number], reviews);
    prunePrIndexIfClosed(repo, nextPr);

    if (nextPr.headCommitSha) {
      const checks = await props.app.fetchCheckRuns(repo, nextPr.headCommitSha);
      props.queryClient.setQueryData(["checks", repo, nextPr.headCommitSha], checks);
    }

    if (includeFiles) {
      const files = await props.app.fetchCategorizedFiles(repo, number);
      props.queryClient.setQueryData(["files", repo, number], files);
    }

    const sourceClone = props.app.resolveSourceClone(repo);
    if (sourceClone) {
      void props.queryClient.invalidateQueries({ queryKey: ["worktrees", sourceClone] });
    }
  }

  function pumpRefreshQueue(): void {
    while (activeRefreshes < MAX_ACTIVE_REFRESHES) {
      const next = nextQueuedRefresh();
      if (!next) return;

      const key = `${next.repo}#${next.number}`;
      next.phase = "refreshing";
      activeRefreshes++;
      setRefreshPhase(key, "refreshing");

      void runQueuedRefresh(next)
        .catch((error) => {
          uiActions.setStatusMessage({
            text: formatRefreshError(`refresh failed for #${next.number}`, error),
            kind: "error",
          });
        })
        .finally(() => {
          activeRefreshes--;
          clearQueuedRefresh(key);
          pumpRefreshQueue();
        });
    }
  }

  function refreshSelected(pr?: PR) {
    const target = pr ? prKey(pr) : selectedPrForRefresh();
    if (!target) return;
    queuePrRefresh(target, { priority: 0, includeFiles: true });
  }

  function refreshAll() {
    const currentRepos = repoTabs();
    const activeTab = uiState.activeTab;

    props.app.reloadConfig();
    const nextRepos = props.app.trackedRepos();
    setRepoTabs(nextRepos);

    const currentTabRepo =
      activeTab === 0 ? undefined : (currentRepos[activeTab - 1] ?? nextRepos[activeTab - 1]);
    const targetRepos =
      activeTab === 0
        ? Array.from(new Set([...currentRepos, ...nextRepos]))
        : currentTabRepo
          ? [currentTabRepo]
          : [];
    const targetRepoSet = new Set(targetRepos);

    for (const repo of targetRepos) {
      void refreshRepoIndex(repo);
    }

    for (const pr of visiblePRs()) {
      const repo = pr.repoSlug ?? props.app.repoSlug;
      if (!targetRepoSet.has(repo)) continue;
      queuePrRefresh(prKey(pr), {
        priority: refreshPriorityForPr(pr),
        includeFiles: false,
      });
    }
  }

  function refreshDetail() {
    const pr = detailPr() ?? detailPrDetail();
    if (!pr) return;
    queuePrRefresh(prKey(pr), { priority: 0, includeFiles: true });
    setDetailRefreshKey((n) => n + 1);
  }

  // ── Browser actions ───────────────────────────────────────────────────
  const [browserActions] = createBrowserActions({
    defaultRepoSlug: props.app.repoSlug,
    setStatusMessage: uiActions.setStatusMessage,
  });

  const displayRepoSlug = () => {
    const tab = uiState.activeTab;
    return tab === 0 ? "All repos" : (tabs()[tab] ?? "All repos");
  };

  // ── Build summary data for SummaryPanel ───────────────────────────────
  const summaryThreads = (): FullReviewThread[] | undefined => {
    const pr = selectedPr();
    if (!pr) return undefined;
    return threadsForPr(pr);
  };

  const summaryChecks = (): CheckRun[] | undefined => {
    const pr = selectedPrDetail();
    if (!pr) return undefined;
    // Checks query can be permanently disabled (null headCommitSha) — treat as empty.
    return checksForPr(pr) ?? [];
  };

  const summaryReviews = (): Review[] | undefined => {
    const pr = selectedPr();
    if (!pr) return undefined;
    return reviewsForPr(pr);
  };

  const summaryLoading = (): boolean => {
    const pr = selectedPr();
    if (!pr) return false;
    return (
      threadsForPr(pr) === undefined ||
      reviewsForPr(pr) === undefined ||
      selectedFiles() === undefined
    );
  };

  const summaryState = (): PRDerivedState | undefined => {
    const pr = selectedPrDetail();
    return pr ? getPRState(pr) : undefined;
  };

  const detailWorktree = (): WorktreeInfo | undefined => {
    const pr = detailPrDetail();
    return pr ? worktreeForPr(pr) : undefined;
  };

  return (
    <AppShell
      view={uiState.view}
      prs={visiblePRs()}
      loading={loading()}
      githubNetworkStats={githubNetworkStatsForBar()}
      repoSlug={displayRepoSlug()}
      showRepo={showRepo()}
      currentUser={props.app.currentUser}
      resetKey={uiState.activeTab}
      error={prError()}
      tabs={tabs()}
      activeTab={uiState.activeTab}
      selectedPr={selectedPrDetail()}
      summaryThreads={summaryThreads()}
      summaryChecks={summaryChecks()}
      summaryReviews={summaryReviews()}
      summaryFiles={selectedFiles()}
      summaryLoading={summaryLoading()}
      getPRState={getPRState}
      getRefreshState={refreshStateForPr}
      summaryState={summaryState()}
      onSelectionChange={selectPr}
      onTabChange={changeTab}
      onRefreshAll={refreshAll}
      onRefreshSelected={refreshSelected}
      onEnterDetail={(pr: PR) => uiActions.enterDetail(pr)}
      detailPr={detailPrDetail()}
      detailChecks={summaryChecks()}
      detailThreads={detailThreads()}
      detailComments={detailComments()}
      detailLoading={detailLoading()}
      showResolved={uiState.showResolved}
      showBotComments={uiState.showBotComments}
      onExitDetail={uiActions.exitDetail}
      onToggleResolved={uiActions.toggleResolved}
      onToggleBotComments={uiActions.toggleBotComments}
      onRefreshDetail={refreshDetail}
      onOpenInBrowser={browserActions.openInBrowser}
      onOpenInDevin={browserActions.openInDevin}
      onOpenUrl={browserActions.openUrl}
      onCreateWorktree={handleCreateWorktree}
      statusMessage={uiState.statusMessage}
      detailWorktree={detailWorktree()}
    />
  );
}

export function createApp(app: Legit) {
  return () => <App app={app} />;
}
