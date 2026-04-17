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
import { samePr } from "./lib/pr-identity";
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
import { derivePRState, type PRDerivedState } from "./lib/pr-state";
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

  // ── PR queries (one per repo) ─────────────────────────────────────────
  const prQueries = useQueries<PR[]>(() => ({
    queries: repoTabs().map((repo) => ({
      queryKey: ["prs", repo] as const,
      queryFn: streamedQuery({
        streamFn: ({ signal }: { signal: AbortSignal }) => props.app.fetchPRs(repo, signal),
        reducer: (_prev: PR[], snapshot: PR[]) => snapshot,
        initialValue: [] as PR[],
      }),
    })),
  }));

  /** All PRs across repos, with repoSlug stamped. */
  const allPRs = createMemo<PR[]>(() => {
    const repos = repoTabs();
    const merged: PR[] = [];
    for (let i = 0; i < repos.length; i++) {
      const q = prQueries[i];
      const data = q?.data ?? [];
      const repo = repos[i]!;
      for (const pr of data) {
        merged.push(pr.repoSlug ? pr : { ...pr, repoSlug: repo });
      }
    }
    merged.sort((a, b) => new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime());
    return merged;
  });

  /** PRs visible for current tab. */
  const visiblePRs = createMemo<PR[]>(() => {
    const tab = ui.activeTab();
    if (tab === 0) return allPRs();
    const repo = repoTabs()[tab - 1];
    if (!repo) return [];
    const idx = repoTabs().indexOf(repo);
    const q = prQueries[idx];
    const data = q?.data ?? [];
    return data.map((pr) => (pr.repoSlug ? pr : { ...pr, repoSlug: repo }));
  });

  /** True while any PR query is still pending (no data yet). */
  const loading = createMemo(() => prQueries.some((q) => q.isPending));

  /** First error message, if any. */
  const prError = createMemo(() => {
    for (const q of prQueries) {
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
        const q = prQueries[i];
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
  // background computation but returns UNKNOWN.  Once the PR list
  // generator settles, we schedule a single delayed re-fetch so the
  // retry runs in parallel with enrichment queries instead of blocking them.
  const mergeableRetried = new Set<string>();
  createEffect(
    () => settledRepos(),
    (settled) => {
      const repos = repoTabs();
      for (let i = 0; i < repos.length; i++) {
        const repo = repos[i]!;
        if (!settled.has(repo)) continue;
        if (mergeableRetried.has(repo)) continue;
        const prs = prQueries[i]?.data ?? [];
        if (prs.some((pr) => pr.mergeable === "UNKNOWN")) {
          mergeableRetried.add(repo);
          const timer = setTimeout(() => {
            void props.queryClient.invalidateQueries({ queryKey: ["prs", repo] });
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

  const threadsForPr = (pr: PR): FullReviewThread[] | undefined => {
    threadsQueryVersion();
    const repo = pr.repoSlug ?? props.app.repoSlug;
    return props.queryClient.getQueryData<FullReviewThread[]>(["threads", repo, pr.number]);
  };

  const reviewsForPr = (pr: PR): Review[] | undefined => {
    reviewsQueryVersion();
    const repo = pr.repoSlug ?? props.app.repoSlug;
    return props.queryClient.getQueryData<Review[]>(["reviews", repo, pr.number]);
  };

  const threadStateForPr = (pr: PR) => {
    threadsQueryVersion();
    const repo = pr.repoSlug ?? props.app.repoSlug;
    return props.queryClient.getQueryState<FullReviewThread[]>(["threads", repo, pr.number]);
  };

  const reviewStateForPr = (pr: PR) => {
    reviewsQueryVersion();
    const repo = pr.repoSlug ?? props.app.repoSlug;
    return props.queryClient.getQueryState<Review[]>(["reviews", repo, pr.number]);
  };

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
    });
  };

  // ── Selection state ───────────────────────────────────────────────────
  const [selectedPr, setSelectedPr] = createSignal<PR | undefined>(undefined, {
    equals: samePr,
  });

  function selectPr(pr: PR) {
    setSelectedPr(pr);
  }

  function changeTab(index: number) {
    ui.changeTab(index);
    setSelectedPr(undefined);
  }

  // ── Summary panel queries ─────────────────────────────────────────────
  const [filesData, setFilesData] = createSignal<FileCategorization | undefined>();
  const [filesRefreshKey, setFilesRefreshKey] = createSignal(0);
  createAbortableAsyncEffect(
    () => ({ pr: selectedPr(), refreshKey: filesRefreshKey() }),
    async ({ pr }, signal, isCurrent) => {
      setFilesData(undefined);

      if (!pr) return;

      const repo = pr.repoSlug ?? props.app.repoSlug;
      const data = await props.app.fetchCategorizedFiles(repo, pr.number, signal);
      if (!isCurrent()) return;
      setFilesData(data);
    },
    () => {},
  );

  // ── Detail view queries ───────────────────────────────────────────────
  const detailPr = () => {
    const v = ui.view();
    return v.view === "detail" ? v.pr : undefined;
  };

  const [detailPrData, setDetailPrData] = createSignal<PRDetail | undefined>();
  const [detailThreadsData, setDetailThreadsData] = createSignal<FullReviewThread[]>([]);
  const [detailCommentsData, setDetailCommentsData] = createSignal<IssueComment[]>([]);
  const [detailLoading, setDetailLoading] = createSignal(false);
  const [detailError, setDetailError] = createSignal("");
  const [detailRefreshKey, setDetailRefreshKey] = createSignal(0);
  createAbortableAsyncEffect(
    () => ({ pr: detailPr(), refreshKey: detailRefreshKey() }),
    async ({ pr }, signal, isCurrent) => {
      setDetailPrData(undefined);
      setDetailThreadsData([]);
      setDetailCommentsData([]);
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

      props.queryClient.setQueryData(["pr-detail", repo, pr.number], nextPr);
      props.queryClient.setQueryData(["threads", repo, pr.number], threads);
      props.queryClient.setQueryData(["issue-comments", repo, pr.number], comments);
      setDetailPrData(nextPr);
      setDetailThreadsData(threads);
      setDetailCommentsData(comments);
      setDetailLoading(false);
    },
    (error) => {
      setDetailLoading(false);
      setDetailError(error instanceof Error ? error.message : String(error));
    },
  );

  // ── Refresh handlers ──────────────────────────────────────────────────
  function refreshSelected() {
    const pr = selectedPr();
    if (!pr) return;
    const repo = pr.repoSlug ?? props.app.repoSlug;
    mergeableRetried.delete(repo);
    void props.queryClient.invalidateQueries({ queryKey: ["prs", repo] });
    void props.queryClient.invalidateQueries({
      queryKey: ["threads", repo, pr.number],
    });
    void props.queryClient.invalidateQueries({
      queryKey: ["checks", repo, pr.headCommitSha ?? ""],
    });
    void props.queryClient.invalidateQueries({
      queryKey: ["reviews", repo, pr.number],
    });
    setFilesRefreshKey((n) => n + 1);
  }

  function refreshAll() {
    props.app.reloadConfig();
    setRepoTabs(props.app.trackedRepos());
    mergeableRetried.clear();
    void props.queryClient.invalidateQueries();
  }

  function refreshDetail() {
    if (!detailPr()) return;
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
    const pr = selectedPr();
    if (!pr) return undefined;
    // Checks query can be permanently disabled (null headCommitSha) — treat as empty.
    return checksForPr(pr) ?? [];
  };

  const summaryReviews = (): Review[] | undefined => {
    const pr = selectedPr();
    if (!pr) return undefined;
    return reviewsForPr(pr);
  };

  const summaryFiles = (): FileCategorization | undefined => {
    return filesData();
  };

  const summaryLoading = (): boolean => {
    const pr = selectedPr();
    if (!pr) return false;
    const threadState = threadStateForPr(pr);
    const reviewState = reviewStateForPr(pr);
    return (
      threadState?.data === undefined ||
      reviewState?.data === undefined ||
      filesData() === undefined
    );
  };

  const summaryState = (): PRDerivedState | undefined => {
    const pr = selectedPr();
    return pr ? getPRState(pr) : undefined;
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
      selectedPr={selectedPr()}
      summaryThreads={summaryThreads()}
      summaryChecks={summaryChecks()}
      summaryReviews={summaryReviews()}
      summaryFiles={summaryFiles()}
      summaryLoading={summaryLoading()}
      getPRState={getPRState}
      summaryState={summaryState()}
      onSelectionChange={selectPr}
      onTabChange={changeTab}
      onRefreshAllActive={refreshAll}
      onRefreshSelected={refreshSelected}
      onEnterDetail={(pr: PR) => ui.enterDetail(pr)}
      detailPr={(() => {
        const data = detailPrData();
        if (!data) return undefined;
        const pr = detailPr();
        return { ...data, repoSlug: pr?.repoSlug ?? props.app.repoSlug };
      })()}
      detailChecks={summaryChecks()}
      detailThreads={detailThreadsData()}
      detailComments={detailCommentsData()}
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
    />
  );
}

export function createApp(app: Legit) {
  return () => <App app={app} />;
}
