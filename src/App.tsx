import { createSignal, createMemo, createEffect, onSettled, onCleanup } from "solid-js";
import type { JSX as OpenTuiJSX } from "@opentui/solid";
import { execFile } from "child_process";
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
import type { GitHubNetworkStats } from "./lib/concurrency";
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
/** Build a GitHub PR URL from a repo slug and PR number. */
export function prUrl(repoSlug: string, number: number): string {
  return `https://github.com/${repoSlug}/pull/${number}`;
}

/** Build a Devin review URL from a repo slug and PR number. */
export function devinUrl(repoSlug: string, number: number): string {
  const [owner, repo] = repoSlug.split("/");
  return `https://app.devin.ai/review/${owner}/${repo}/pull/${number}`;
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
  const ui = createUIState();

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
    onCleanup(unsubHttp);
  });

  // ── Repo tabs ─────────────────────────────────────────────────────────
  const [repoTabs, setRepoTabs] = createSignal<string[]>(props.app.trackedRepos());

  const tabs = createMemo(() => ["All", ...repoTabs()]);

  const showRepo = createMemo(() => ui.activeTab() === 0 && repoTabs().length > 1);

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
    const tab = ui.activeTab();
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

  const prQueryVersion = createMemo(() =>
    prQueries
      .map((query) => `${query.status}:${query.fetchStatus}:${query.dataUpdatedAt}`)
      .join("|"),
  );

  /** PRs for the current tab, read from the per-PR cache by key. Reading
   *  by (repo, number) avoids positional mismatches when visibleIndex and
   *  prQueries.state are transiently out of sync during reconfiguration. */
  const visiblePRs = createMemo<PR[]>(() => {
    prQueryVersion();
    const prs: PR[] = [];
    for (const entry of visibleIndex()) {
      const data = props.queryClient.getQueryData<PRDetail>(["pr", entry.repoSlug, entry.number]);
      if (data) prs.push(data);
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
    const tab = ui.activeTab();
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
          onCleanup(() => clearTimeout(timer));
        }
      }
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

  const threadsQueryVersion = createMemo(() =>
    threadsQueries
      .map((query) => `${query.status}:${query.fetchStatus}:${query.dataUpdatedAt}`)
      .join("|"),
  );

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

  const reviewsQueryVersion = createMemo(() =>
    reviewsQueries
      .map((query) => `${query.status}:${query.fetchStatus}:${query.dataUpdatedAt}`)
      .join("|"),
  );

  const threadsForPr = (pr: PRIdentity): FullReviewThread[] | undefined => {
    threadsQueryVersion();
    const repo = pr.repoSlug ?? props.app.repoSlug;
    return props.queryClient.getQueryData<FullReviewThread[]>(["threads", repo, pr.number]);
  };

  const reviewsForPr = (pr: PRIdentity): Review[] | undefined => {
    reviewsQueryVersion();
    const repo = pr.repoSlug ?? props.app.repoSlug;
    return props.queryClient.getQueryData<Review[]>(["reviews", repo, pr.number]);
  };

  const threadStateForPr = (pr: PRIdentity) => {
    threadsQueryVersion();
    const repo = pr.repoSlug ?? props.app.repoSlug;
    return props.queryClient.getQueryState<FullReviewThread[]>(["threads", repo, pr.number]);
  };

  const reviewStateForPr = (pr: PRIdentity) => {
    reviewsQueryVersion();
    const repo = pr.repoSlug ?? props.app.repoSlug;
    return props.queryClient.getQueryState<Review[]>(["reviews", repo, pr.number]);
  };

  const worktreeController = createWorktreeController({
    app: props.app,
    queryClient: props.queryClient,
    repoTabs,
    setStatusMessage: ui.setStatusMessage,
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
    ui.changeTab(index);
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
    const v = ui.view();
    return v.view === "detail" ? v.pr : undefined;
  };

  const [detailComments, setDetailComments] = createSignal<IssueComment[]>([]);
  const [detailLoading, setDetailLoading] = createSignal(false);
  const [detailError, setDetailError] = createSignal("");
  const [detailRefreshKey, setDetailRefreshKey] = createSignal(0);
  createAbortableAsyncEffect(
    () => ({ pr: detailPr(), refreshKey: detailRefreshKey() }),
    async ({ pr }, signal, isCurrent) => {
      setDetailComments([]);
      setDetailError("");

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
      setDetailError(error instanceof Error ? error.message : String(error));
    },
  );

  /** Read the freshest PRDetail for a PR identity from the per-PR cache.
   *  Reactive via prQueryVersion so consumers re-render when any PR
   *  refetches. */
  function cachedPr(pr: PRIdentity | undefined): PRDetail | undefined {
    prQueryVersion();
    if (!pr) return undefined;
    const repo = pr.repoSlug ?? props.app.repoSlug;
    return props.queryClient.getQueryData<PRDetail>(["pr", repo, pr.number]);
  }

  const detailPrDetail = (): PRDetail | undefined => cachedPr(detailPr());

  /** Full data for the selected PR, derived from the cache. Memoized since
   *  several render consumers read it each tick. */
  const selectedPrDetail = createMemo<PRDetail | undefined>(() => cachedPr(selectedPr()));

  /** Detail view threads read from the threads cache; reactive via threadsQueryVersion. */
  const detailThreads = (): FullReviewThread[] | undefined => {
    threadsQueryVersion();
    const pr = detailPr();
    if (!pr) return undefined;
    const repo = pr.repoSlug ?? props.app.repoSlug;
    return props.queryClient.getQueryData<FullReviewThread[]>(["threads", repo, pr.number]);
  };

  // ── Refresh handlers ──────────────────────────────────────────────────
  function invalidatePr(repo: string, pr: PR) {
    mergeableRetried.delete(`${repo}:${pr.number}`);
    void props.queryClient.invalidateQueries({ queryKey: ["pr", repo, pr.number] });
    void props.queryClient.invalidateQueries({ queryKey: ["threads", repo, pr.number] });
    void props.queryClient.invalidateQueries({
      queryKey: ["checks", repo, pr.headCommitSha ?? ""],
    });
    void props.queryClient.invalidateQueries({ queryKey: ["reviews", repo, pr.number] });
    void props.queryClient.invalidateQueries({ queryKey: ["files", repo, pr.number] });
    const sourceClone = props.app.resolveSourceClone(repo);
    if (sourceClone) {
      void props.queryClient.invalidateQueries({ queryKey: ["worktrees", sourceClone] });
    }
  }

  function refreshSelected() {
    const pr = selectedPrDetail();
    if (!pr) return;
    const repo = pr.repoSlug ?? props.app.repoSlug;
    invalidatePr(repo, pr);
  }

  function refreshAll() {
    props.app.reloadConfig();
    setRepoTabs(props.app.trackedRepos());
    mergeableRetried.clear();
    // Re-stream indexes; the reducer re-seeds per-PR caches, so we do not
    // separately invalidate ["pr", ...] (that would produce N extra fetches).
    void props.queryClient.invalidateQueries({ queryKey: ["pr-index"] });
    void props.queryClient.invalidateQueries({ queryKey: ["threads"] });
    void props.queryClient.invalidateQueries({ queryKey: ["reviews"] });
    void props.queryClient.invalidateQueries({ queryKey: ["checks"] });
    void props.queryClient.invalidateQueries({ queryKey: ["files"] });
    void props.queryClient.invalidateQueries({ queryKey: ["worktrees"] });
  }

  function refreshDetail() {
    const pr = detailPrDetail();
    if (!pr) return;
    const repo = pr.repoSlug ?? props.app.repoSlug;
    invalidatePr(repo, pr);
    setDetailRefreshKey((n) => n + 1);
  }

  // ── Browser actions ───────────────────────────────────────────────────
  const [browserError, setBrowserError] = createSignal("");

  function handleOpenInBrowser(pr: PR) {
    setBrowserError("");
    execFile("open", [prUrl(pr.repoSlug ?? props.app.repoSlug, pr.number)], (err) => {
      if (err) setBrowserError(`Failed to open browser: ${err.message}`);
    });
  }

  function handleOpenUrl(url: string) {
    setBrowserError("");
    execFile("open", [url], (err) => {
      if (err) setBrowserError(`Failed to open browser: ${err.message}`);
    });
  }

  function handleOpenInDevin(pr: PR) {
    setBrowserError("");
    execFile("open", [devinUrl(pr.repoSlug ?? props.app.repoSlug, pr.number)], (err) => {
      if (err) setBrowserError(`Failed to open Devin: ${err.message}`);
    });
  }

  const displayRepoSlug = () => {
    const tab = ui.activeTab();
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
    const threadState = threadStateForPr(pr);
    const reviewState = reviewStateForPr(pr);
    return (
      threadState?.data === undefined ||
      reviewState?.data === undefined ||
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
      view={ui.view()}
      prs={visiblePRs()}
      loading={loading()}
      githubNetworkStats={githubNetworkStatsForBar()}
      repoSlug={displayRepoSlug()}
      showRepo={showRepo()}
      currentUser={props.app.currentUser}
      resetKey={ui.activeTab()}
      error={prError() || detailError() || browserError()}
      tabs={tabs()}
      activeTab={ui.activeTab()}
      selectedPr={selectedPrDetail()}
      summaryThreads={summaryThreads()}
      summaryChecks={summaryChecks()}
      summaryReviews={summaryReviews()}
      summaryFiles={selectedFiles()}
      summaryLoading={summaryLoading()}
      getPRState={getPRState}
      summaryState={summaryState()}
      onSelectionChange={selectPr}
      onTabChange={changeTab}
      onRefreshAll={refreshAll}
      onRefreshSelected={refreshSelected}
      onEnterDetail={(pr: PR) => ui.enterDetail(pr)}
      detailPr={detailPrDetail()}
      detailChecks={summaryChecks()}
      detailThreads={detailThreads()}
      detailComments={detailComments()}
      detailLoading={detailLoading()}
      showResolved={ui.showResolved()}
      showBotComments={ui.showBotComments()}
      onExitDetail={ui.exitDetail}
      onToggleResolved={ui.toggleResolved}
      onToggleBotComments={ui.toggleBotComments}
      onRefreshDetail={refreshDetail}
      onOpenInBrowser={handleOpenInBrowser}
      onOpenInDevin={handleOpenInDevin}
      onOpenUrl={handleOpenUrl}
      onCreateWorktree={handleCreateWorktree}
      statusMessage={ui.statusMessage()}
      detailWorktree={detailWorktree()}
    />
  );
}

export function createApp(app: Legit) {
  return () => <App app={app} />;
}
