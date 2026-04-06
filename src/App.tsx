import { createSignal, createMemo, createEffect, on } from "solid-js";
import { execFile } from "child_process";
import {
	QueryClient,
	QueryClientProvider,
	useQuery,
	useQueries,
	experimental_streamedQuery as streamedQuery,
} from "@tanstack/solid-query";
import { AppShell } from "./components/AppShell";
import { createUIState } from "./lib/ui-state";
import type { Legit } from "./lib/legit";
import type { PR, CheckRun, Review, FullReviewThread, FileCategorization } from "./lib/types";
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

export function App(props: AppProps) {
	const queryClient = createQueryClient();

	return (
		<QueryClientProvider client={queryClient}>
			<AppInner app={props.app} queryClient={queryClient} />
		</QueryClientProvider>
	);
}

interface AppInnerProps {
	app: Legit;
	queryClient: QueryClient;
}

function AppInner(props: AppInnerProps) {
	const ui = createUIState();

	// ── Repo tabs ─────────────────────────────────────────────────────────
	const [repoTabs, setRepoTabs] = createSignal<string[]>(props.app.trackedRepos());

	const tabs = createMemo(() => ["All", ...repoTabs()]);

	const showRepo = createMemo(() => ui.activeTab() === 0 && repoTabs().length > 1);

	// ── PR queries (one per repo) ─────────────────────────────────────────
	const prQueries = useQueries(() => ({
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

	// ── Per-PR enrichment queries (threads, checks, reviews) ──────────────
	const threadsQueries = useQueries(() => ({
		queries: visiblePRs().map((pr) => ({
			queryKey: ["threads", pr.repoSlug ?? props.app.repoSlug, pr.number] as const,
			queryFn: async ({ signal }: { signal: AbortSignal }) =>
				props.app.fetchFullReviewThreads(
					pr.repoSlug ?? props.app.repoSlug,
					pr.number,
					signal,
				),
			enabled: true,
		})),
	}));

	const checksQueries = useQueries(() => ({
		queries: visiblePRs().map((pr) => ({
			queryKey: [
				"checks",
				pr.repoSlug ?? props.app.repoSlug,
				pr.headCommitSha ?? "",
			] as const,
			queryFn: async ({ signal }: { signal: AbortSignal }) =>
				pr.headCommitSha
					? props.app.fetchCheckRuns(
							pr.repoSlug ?? props.app.repoSlug,
							pr.headCommitSha,
							signal,
						)
					: ([] as CheckRun[]),
			enabled: !!pr.headCommitSha,
		})),
	}));

	const reviewsQueries = useQueries(() => ({
		queries: visiblePRs().map((pr) => ({
			queryKey: ["reviews", pr.repoSlug ?? props.app.repoSlug, pr.number] as const,
			queryFn: async ({ signal }: { signal: AbortSignal }) =>
				props.app.fetchReviews(pr.repoSlug ?? props.app.repoSlug, pr.number, signal),
			enabled: true,
		})),
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
		if (tq.isPending || cq.isPending || rq.isPending) return undefined;

		return {
			threads: tq.data ?? [],
			checks: cq.data ?? [],
			reviews: rq.data ?? [],
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
	const filesQuery = useQuery(() => ({
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
	}));

	// ── Detail view queries ───────────────────────────────────────────────
	const detailPr = () => {
		const v = ui.view();
		return v.view === "detail" ? v.pr : undefined;
	};

	const detailPrQuery = useQuery(() => ({
		queryKey: [
			"pr-detail",
			detailPr()?.repoSlug ?? props.app.repoSlug,
			detailPr()?.number ?? 0,
		] as const,
		queryFn: async ({ signal }: { signal: AbortSignal }) => {
			const pr = detailPr();
			if (!pr) return undefined;
			return props.app.fetchPR(pr.repoSlug ?? props.app.repoSlug, pr.number, signal);
		},
		enabled: !!detailPr(),
	}));

	const detailThreadsQuery = useQuery(() => ({
		queryKey: [
			"threads",
			detailPr()?.repoSlug ?? props.app.repoSlug,
			detailPr()?.number ?? 0,
		] as const,
		queryFn: async ({ signal }: { signal: AbortSignal }) => {
			const pr = detailPr();
			if (!pr) return undefined;
			return props.app.fetchFullReviewThreads(
				pr.repoSlug ?? props.app.repoSlug,
				pr.number,
				signal,
			);
		},
		enabled: !!detailPr(),
	}));

	const detailCommentsQuery = useQuery(() => ({
		queryKey: [
			"issue-comments",
			detailPr()?.repoSlug ?? props.app.repoSlug,
			detailPr()?.number ?? 0,
		] as const,
		queryFn: async ({ signal }: { signal: AbortSignal }) => {
			const pr = detailPr();
			if (!pr) return undefined;
			return props.app.fetchIssueComments(
				pr.repoSlug ?? props.app.repoSlug,
				pr.number,
				signal,
			);
		},
		enabled: !!detailPr(),
	}));

	// ── Refresh handlers ──────────────────────────────────────────────────
	function refreshSelected() {
		const pr = selectedPr();
		if (!pr) return;
		const repo = pr.repoSlug ?? props.app.repoSlug;
		void props.queryClient.invalidateQueries({ queryKey: ["threads", repo, pr.number] });
		void props.queryClient.invalidateQueries({
			queryKey: ["checks", repo, pr.headCommitSha ?? ""],
		});
		void props.queryClient.invalidateQueries({ queryKey: ["reviews", repo, pr.number] });
		void props.queryClient.invalidateQueries({ queryKey: ["files", repo, pr.number] });
	}

	function refreshAll() {
		props.app.reloadConfig();
		setRepoTabs(props.app.trackedRepos());
		void props.queryClient.invalidateQueries();
	}

	function refreshDetail() {
		const pr = detailPr();
		if (!pr) return;
		const repo = pr.repoSlug ?? props.app.repoSlug;
		void props.queryClient.invalidateQueries({ queryKey: ["pr-detail", repo, pr.number] });
		void props.queryClient.invalidateQueries({ queryKey: ["threads", repo, pr.number] });
		void props.queryClient.invalidateQueries({
			queryKey: ["issue-comments", repo, pr.number],
		});
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
		return checksQueries[idx]?.data;
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
		return filesQuery.data ?? undefined;
	};

	const summaryLoading = (): boolean => {
		const pr = selectedPr();
		if (!pr) return false;
		const prs = visiblePRs();
		const idx = prs.findIndex((p) => p.number === pr.number && p.repoSlug === pr.repoSlug);
		if (idx < 0) return false;
		return (
			(threadsQueries[idx]?.isPending ?? true) ||
			(checksQueries[idx]?.isPending ?? true) ||
			(reviewsQueries[idx]?.isPending ?? true) ||
			filesQuery.isPending
		);
	};

	return (
		<AppShell
			view={ui.view()}
			prs={visiblePRs()}
			loading={loading()}
			repoSlug={displayRepoSlug()}
			showRepo={showRepo()}
			currentUser={props.app.currentUser}
			resetKey={ui.activeTab()}
			error={prError() || browserError()}
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
			detailPr={detailPrQuery.data ?? undefined}
			detailThreads={detailThreadsQuery.data ?? []}
			detailComments={detailCommentsQuery.data ?? []}
			detailLoading={detailPrQuery.isPending && !!detailPr()}
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
