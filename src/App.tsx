import { createSignal, createMemo, onSettled } from "solid-js";
import type { JSX as OpenTuiJSX } from "@opentui/solid";
import { QueryClient, QueryClientProvider, useIsFetching } from "@tanstack/solid-query";
import { useQueriesLite as useQueries } from "./lib/use-queries-lite";
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
import { createBrowserActions } from "./lib/browser-actions";
import { createDetailState } from "./lib/detail-state";
import { createRefreshQueue, type RefreshPriority } from "./lib/refresh-queue";
import { createPRQueries } from "./lib/pr-queries";
import { createEnrichmentQueries } from "./lib/enrichment-queries";
import { runPrRefresh, fetchDetail, commitDetailFetch } from "./lib/pr-cache-mutators";

export { prUrl, devinUrl } from "./lib/browser-actions";

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

  // ── PR index + per-PR cache ───────────────────────────────────────────
  // The authoritative store for PR-shaped data is ["pr", repo, number].
  // createPRQueries owns the streamed pr-index queries that seed those
  // entries plus the derived state (visibleIndex, visiblePRs, prByKey,
  // loading, error, settledRepos, enrichmentReady, mergeable retry).
  const [prState, prActions] = createPRQueries({
    app: props.app,
    queryClient: props.queryClient,
    activeTab: () => uiState.activeTab,
  });

  const tabs = createMemo(() => ["All", ...prState.repoTabs]);

  const showRepo = createMemo(() => uiState.activeTab === 0 && prState.repoTabs.length > 1);

  // ── Per-PR enrichment queries (threads, checks, reviews) ──────────────
  const [enrichment] = createEnrichmentQueries({
    app: props.app,
    queryClient: props.queryClient,
    visiblePRs: () => prState.visiblePRs,
    enrichmentReady: () => prState.enrichmentReady,
  });
  const { threadsForPr, checksForPr, reviewsForPr } = enrichment;

  const worktreeController = createWorktreeController({
    app: props.app,
    queryClient: props.queryClient,
    repoTabs: () => prState.repoTabs,
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

  const cacheDeps = { app: props.app, queryClient: props.queryClient, prActions };
  const [detailViewState, detailActions] = createDetailState({
    detailPr,
    fetch: (pr, signal) => fetchDetail(props.app, pr, signal),
    onFetched: (pr, result) => commitDetailFetch(cacheDeps, pr, result),
    setStatusMessage: uiActions.setStatusMessage,
  });

  const detailComments = (): IssueComment[] => {
    const s = detailViewState();
    return s.kind === "ready" ? s.comments : [];
  };
  const detailLoading = (): boolean => detailViewState().kind === "loading";

  /** Read the freshest PRDetail for a PR identity from the per-PR cache.
   *  Reactive via prByKey so consumers re-render when any visible PR
   *  refetches. Returns undefined for PRs not in the current tab. */
  function cachedPr(pr: PRIdentity | undefined): PRDetail | undefined {
    if (!pr) return undefined;
    const repo = pr.repoSlug ?? props.app.repoSlug;
    return prState.prByKey.get(`${repo}#${pr.number}`);
  }

  const detailPrDetail = (): PRDetail | undefined => cachedPr(detailPr());

  /** Full data for the selected PR, derived from the cache. Memoized since
   *  several render consumers read it each tick. */
  const selectedPrDetail = createMemo<PRDetail | undefined>(() => cachedPr(selectedPr()));

  function selectedPrForRefresh(): PRIdentity | undefined {
    const identity = selectedPr();
    if (identity) {
      const live = cachedPr(identity) ?? prState.visiblePRs.find((pr) => samePr(pr, identity));
      return live ? prKey(live) : identity;
    }
    const firstVisible = prState.visiblePRs[0];
    return firstVisible ? prKey(firstVisible) : undefined;
  }

  /** Detail view threads read from the threads cache; reactive via threadsByKey. */
  const detailThreads = (): FullReviewThread[] | undefined => {
    const pr = detailPr();
    if (!pr) return undefined;
    return threadsForPr(pr);
  };

  // ── Refresh handlers ──────────────────────────────────────────────────
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

  const [refreshQueueState, refreshQueueActions] = createRefreshQueue({
    defaultRepoSlug: props.app.repoSlug,
    runRefresh: (item) => runPrRefresh(cacheDeps, item),
    setStatusMessage: uiActions.setStatusMessage,
  });

  const refreshStateForPr = refreshQueueState.refreshStateForPr;
  const queuePrRefresh = refreshQueueActions.queuePrRefresh;

  function refreshSelected(pr?: PR) {
    const target = pr ? prKey(pr) : selectedPrForRefresh();
    if (!target) return;
    queuePrRefresh(target, { priority: 0, includeFiles: true });
  }

  function refreshAll() {
    const currentRepos = prState.repoTabs;
    const activeTab = uiState.activeTab;

    props.app.reloadConfig();
    const nextRepos = props.app.trackedRepos();
    prActions.setRepoTabs(nextRepos);

    const currentTabRepo =
      activeTab === 0 ? undefined : (currentRepos[activeTab - 1] ?? nextRepos[activeTab - 1]);
    const targetRepos =
      activeTab === 0
        ? Array.from(new Set([...currentRepos, ...nextRepos]))
        : currentTabRepo
          ? [currentTabRepo]
          : [];
    const targetRepoSet = new Set(targetRepos);

    // Re-run the streamed pr-index query for each target repo. The streamed
    // reducer (defined alongside prIndexQueries above) seeds per-PR caches
    // and rebuilds the index — invalidation lets us skip a parallel
    // implementation that walks the same generator.
    for (const repo of targetRepos) {
      void props.queryClient.invalidateQueries({ queryKey: ["pr-index", repo] });
    }

    for (const pr of prState.visiblePRs) {
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
    detailActions.refresh();
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
      prs={prState.visiblePRs}
      loading={prState.loading}
      githubNetworkStats={githubNetworkStatsForBar()}
      repoSlug={displayRepoSlug()}
      showRepo={showRepo()}
      currentUser={props.app.currentUser}
      resetKey={uiState.activeTab}
      error={prState.error}
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
