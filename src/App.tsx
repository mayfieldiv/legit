import { createSignal, createMemo, createEffect, on, onMount, onCleanup } from "./lib/solid-compat";
import type { JSX as OpenTuiJSX } from "@opentui/solid";
import { createEffect as solidCreateEffect } from "solid-js";
import { execFile } from "child_process";
import {
  QueryClient,
  QueryClientProvider,
  useIsFetching,
  experimental_streamedQuery as streamedQuery,
} from "@tanstack/solid-query";
import { useQueriesLite as useQueries } from "./lib/use-queries-lite";
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
import type { BlockerOptions } from "./lib/blocker-engine";
/** Build a GitHub PR URL from a repo slug and PR number. */
export function prUrl(repoSlug: string, number: number): string {
  return `https://github.com/${repoSlug}/pull/${number}`;
}

/** Build a Devin review URL from a repo slug and PR number. */
export function devinUrl(repoSlug: string, number: number): string {
  const [owner, repo] = repoSlug.split("/");
  return `https://app.devin.ai/review/${owner}/${repo}/pull/${number}`;
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

  onMount(() => {
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
  const settledRepos = createMemo(() => {
    const settled = new Set<string>();
    const repos = repoTabs();
    for (let i = 0; i < repos.length; i++) {
      const q = prQueries[i];
      if (q && !q.isFetching) settled.add(repos[i]!);
    }
    return settled;
  });

  // ── Retry UNKNOWN mergeable status after settlement ─────────────────
  // GitHub computes mergeability lazily — the initial fetch triggers
  // background computation but returns UNKNOWN.  Once the PR list
  // generator settles, we schedule a single delayed re-fetch so the
  // retry runs in parallel with enrichment queries instead of blocking them.
  const mergeableRetried = new Set<string>();
  createEffect(
    on(settledRepos, (settled) => {
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
    }),
  );

  // ── Per-PR enrichment queries (threads, checks, reviews) ──────────────
  const threadsQueries = useQueries<FullReviewThread[]>(() => ({
    queries: visiblePRs().map((pr) => {
      const repo = pr.repoSlug ?? props.app.repoSlug;
      return {
        queryKey: ["threads", repo, pr.number] as const,
        queryFn: async ({ signal }: { signal: AbortSignal }) =>
          props.app.fetchFullReviewThreads(repo, pr.number, signal),
        enabled: settledRepos().has(repo),
      };
    }),
  }));

  const checksQueries = useQueries<CheckRun[]>(() => ({
    queries: visiblePRs().map((pr) => {
      const repo = pr.repoSlug ?? props.app.repoSlug;
      return {
        queryKey: ["checks", repo, pr.headCommitSha ?? `missing-${pr.number}`] as const,
        queryFn: async ({ signal }: { signal: AbortSignal }) =>
          pr.headCommitSha
            ? props.app.fetchCheckRuns(repo, pr.headCommitSha, signal)
            : ([] as CheckRun[]),
        enabled: !!pr.headCommitSha && settledRepos().has(repo),
      };
    }),
  }));

  const reviewsQueries = useQueries<Review[]>(() => ({
    queries: visiblePRs().map((pr) => {
      const repo = pr.repoSlug ?? props.app.repoSlug;
      return {
        queryKey: ["reviews", repo, pr.number] as const,
        queryFn: async ({ signal }: { signal: AbortSignal }) =>
          props.app.fetchReviews(repo, pr.number, signal),
        enabled: settledRepos().has(repo),
      };
    }),
  }));

  // ── Blocker data lookup for grouping engine ───────────────────────────
  const getBlockerData = (pr: PR): BlockerOptions | undefined => {
    const prs = visiblePRs();
    const idx = prs.findIndex((p) => p.number === pr.number && p.repoSlug === pr.repoSlug);
    if (idx < 0) return undefined;

    const tq = threadsQueries[idx];
    const cq = checksQueries[idx];
    const rq = reviewsQueries[idx];

    if (!tq || !cq || !rq) return undefined;
    // Threads and reviews always become enabled once the repo settles, so
    // undefined data means "not yet loaded." Checks can be permanently
    // disabled (null headCommitSha) — treat missing checks data as empty.
    if (tq.data === undefined || rq.data === undefined) return undefined;

    return {
      threads: tq.data,
      checks: cq.data ?? [],
      reviews: rq.data,
    };
  };

  // ── Selection state ───────────────────────────────────────────────────
  const [selectedPr, setSelectedPr] = createSignal<PR | undefined>();

  // Auto-select first PR when list loads
  createEffect(
    on(visiblePRs, (prs) => {
      if (!selectedPr() && prs.length > 0) {
        setSelectedPr(prs[0]);
      }
    }),
  );

  function selectPr(pr: PR) {
    setSelectedPr(pr);
  }

  function changeTab(index: number) {
    ui.changeTab(index);
    setSelectedPr(undefined);
  }

  // ── Summary panel queries ─────────────────────────────────────────────
  const filesQueries = useQueries<FileCategorization | undefined>(() => ({
    queries: [
      {
        queryKey: [
          "files",
          selectedPr()?.repoSlug ?? props.app.repoSlug,
          selectedPr()?.number ?? 0,
        ] as const,
        queryFn: async ({ signal }: { signal: AbortSignal }) => {
          const pr = selectedPr();
          if (!pr) return undefined;
          return props.app.fetchCategorizedFiles(
            pr.repoSlug ?? props.app.repoSlug,
            pr.number,
            signal,
          );
        },
        enabled: !!selectedPr(),
      },
    ],
  }));
  const filesQuery = () => filesQueries[0];
  const [filesData, setFilesData] = createSignal<FileCategorization | undefined>();

  createEffect(
    on(
      () => selectedPr(),
      () => {
        setFilesData(undefined);
      },
    ),
  );

  createEffect(
    on(
      () => filesQuery()?.data,
      (data) => {
        setFilesData(data ?? undefined);
      },
    ),
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
  let detailController: AbortController | undefined;

  onCleanup(() => {
    detailController?.abort();
  });

  solidCreateEffect(
    () => {
      const pr = detailPr();
      const refreshKey = detailRefreshKey();
      void refreshKey;

      detailController?.abort();
      detailController = undefined;

      setDetailPrData(undefined);
      setDetailThreadsData([]);
      setDetailCommentsData([]);
      setDetailError("");

      if (!pr) {
        setDetailLoading(false);
        return;
      }

      const repo = pr.repoSlug ?? props.app.repoSlug;
      const controller = new AbortController();
      detailController = controller;
      setDetailLoading(true);

      void Promise.all([
        props.app.fetchPR(repo, pr.number, controller.signal),
        props.app.fetchFullReviewThreads(repo, pr.number, controller.signal),
        props.app.fetchIssueComments(repo, pr.number, controller.signal),
      ])
        .then(([nextPr, threads, comments]) => {
          if (controller.signal.aborted || detailController !== controller) return;
          props.queryClient.setQueryData(["pr-detail", repo, pr.number], nextPr);
          props.queryClient.setQueryData(["threads", repo, pr.number], threads);
          props.queryClient.setQueryData(["issue-comments", repo, pr.number], comments);
          setDetailPrData(nextPr);
          setDetailThreadsData(threads);
          setDetailCommentsData(comments);
          setDetailLoading(false);
        })
        .catch((error: unknown) => {
          if (controller.signal.aborted || detailController !== controller) return;
          setDetailLoading(false);
          setDetailError(error instanceof Error ? error.message : String(error));
        });
    },
    () => undefined,
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
    void props.queryClient.invalidateQueries({
      queryKey: ["files", repo, pr.number],
    });
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
    const prs = visiblePRs();
    const idx = prs.findIndex((p) => p.number === pr.number && p.repoSlug === pr.repoSlug);
    if (idx < 0) return undefined;
    return threadsQueries[idx]?.data;
  };

  const summaryChecks = (): CheckRun[] | undefined => {
    const pr = selectedPr();
    if (!pr) return undefined;
    const prs = visiblePRs();
    const idx = prs.findIndex((p) => p.number === pr.number && p.repoSlug === pr.repoSlug);
    if (idx < 0) return undefined;
    // Checks query can be permanently disabled (null headCommitSha) — treat as empty.
    return checksQueries[idx]?.data ?? [];
  };

  const summaryReviews = (): Review[] | undefined => {
    const pr = selectedPr();
    if (!pr) return undefined;
    const prs = visiblePRs();
    const idx = prs.findIndex((p) => p.number === pr.number && p.repoSlug === pr.repoSlug);
    if (idx < 0) return undefined;
    return reviewsQueries[idx]?.data;
  };

  const summaryFiles = (): FileCategorization | undefined => {
    return filesData();
  };

  const summaryLoading = (): boolean => {
    const pr = selectedPr();
    if (!pr) return false;
    const prs = visiblePRs();
    const idx = prs.findIndex((p) => p.number === pr.number && p.repoSlug === pr.repoSlug);
    if (idx < 0) return false;
    return (
      threadsQueries[idx]?.data === undefined ||
      reviewsQueries[idx]?.data === undefined ||
      filesData() === undefined
    );
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
      getBlockerData={getBlockerData}
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
