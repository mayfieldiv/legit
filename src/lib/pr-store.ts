import { batch, createMemo, createSignal, onCleanup, onMount, type Accessor } from "solid-js";
import { makeCoalescer } from "./coalescer";
import type { Legit } from "./legit";
import type { PR, PRSummary, CommentCounts } from "./types";

export interface PRStoreOptions {
	/**
	 * Debounce delay for summary fetches on selection change.
	 * Default: 300. Tests should pass 0 to make selection-driven
	 * summary fetches immediate and deterministic.
	 */
	summaryDebounceMs?: number;
}

export interface PRStore {
	readonly prs: Accessor<PR[]>;
	readonly tabs: Accessor<string[]>;
	readonly activeTab: Accessor<number>;
	readonly selectedPr: Accessor<PR | undefined>;
	readonly summary: Accessor<PRSummary | undefined>;
	readonly loading: Accessor<boolean>;
	readonly error: Accessor<string>;
	readonly showRepo: Accessor<boolean>;
	selectPr(pr: PR): void;
	changeTab(index: number): void;
	refreshAll(): void;
	refreshSelected(): void;
}

const THREAD_CONCURRENCY = 5;

export function createPRStore(app: Legit, options?: PRStoreOptions): PRStore {
	const summaryDebounceMs = options?.summaryDebounceMs ?? 300;

	const [error, setError] = createSignal("");
	const [repoTabs, setRepoTabs] = createSignal<string[]>([]);
	const [activeTab, setActiveTab] = createSignal(0);
	const [prsByRepo, setPrsByRepo] = createSignal<Record<string, PR[]>>({});
	const [loading, setLoading] = createSignal(true);
	const [selectedPr, setSelectedPr] = createSignal<PR | undefined>();
	const [summary, setSummary] = createSignal<PRSummary | undefined>();

	const repoControllers = new Map<string, AbortController>();
	const [_loadingRepos, setLoadingRepos] = createSignal(new Set<string>());
	let summaryController: AbortController | undefined;
	let bgThreadsController: AbortController | undefined;
	let debounceTimer: ReturnType<typeof setTimeout> | undefined;
	const summaryCache = new Map<string, PRSummary>();

	function cacheKey(pr: PR): string {
		return `${pr.repoSlug ?? app.repoSlug}#${pr.number}`;
	}

	const tabs = createMemo(() => ["All", ...repoTabs()]);

	function visiblePRsForTab(tabIndex: number): PR[] {
		const byRepo = prsByRepo();
		if (tabIndex === 0) {
			const merged: PR[] = [];
			for (const repo of repoTabs()) {
				const repoPrs = byRepo[repo] ?? [];
				for (const pr of repoPrs) {
					merged.push({ ...pr, repoSlug: repo });
				}
			}
			merged.sort(
				(a, b) => new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime(),
			);
			return merged;
		}
		const repo = repoTabs()[tabIndex - 1];
		if (!repo) return [];
		return (byRepo[repo] ?? []).map((pr) => (pr.repoSlug ? pr : { ...pr, repoSlug: repo }));
	}

	const prs = createMemo(() => visiblePRsForTab(activeTab()));

	const showRepo = createMemo(() => activeTab() === 0 && repoTabs().length > 1);

	function setRepoLoading(repo: string, value: boolean) {
		setLoadingRepos((prev) => {
			const next = new Set(prev);
			if (value) next.add(repo);
			else next.delete(repo);
			setLoading(next.size > 0);
			return next;
		});
	}

	async function loadRepo(repo: string) {
		repoControllers.get(repo)?.abort();
		const ac = new AbortController();
		repoControllers.set(repo, ac);
		setRepoLoading(repo, true);
		setError("");
		setPrsByRepo((prev) => ({ ...prev, [repo]: [] }));

		const { schedule, flush } = makeCoalescer<PR[]>((snapshot) => {
			setPrsByRepo((prev) => ({ ...prev, [repo]: snapshot }));
		}, ac.signal);

		try {
			for await (const snapshot of app.fetchPRs(repo, ac.signal)) {
				schedule(snapshot);
				if (!selectedPr() && snapshot.length > 0 && activeTab() === 0) {
					selectPr({ ...snapshot[0]!, repoSlug: repo });
				}
			}
		} catch (err: any) {
			if (!ac.signal.aborted) {
				setError(err.message ?? String(err));
			}
		} finally {
			flush();
			if (!ac.signal.aborted) {
				setRepoLoading(repo, false);
			}
		}
	}

	function discoverRepos(): string[] {
		return app.trackedRepos();
	}

	async function loadPRs(opts?: { resetActiveTab?: boolean }) {
		bgThreadsController?.abort();
		for (const c of repoControllers.values()) c.abort();
		repoControllers.clear();

		const repos = discoverRepos();

		batch(() => {
			setLoadingRepos(new Set<string>());
			setPrsByRepo({});
			setLoading(true);
			setError("");
			summaryCache.clear();
			setSummary(undefined);
			setSelectedPr(undefined);
			setRepoTabs(repos);
			if (opts?.resetActiveTab) {
				setActiveTab(0);
			}
		});

		const pending = repos.map((repo) => loadRepo(repo));
		const controllers = repos.map((repo) => repoControllers.get(repo)!);
		await Promise.all(pending);
		const stale = controllers.some((c) => c.signal.aborted);
		if (!stale) {
			setLoading(false);
			startBackgroundThreadLoad(repos).catch(() => {});
		}
	}

	async function fetchSummary(pr: PR) {
		summaryController?.abort();
		const ac = new AbortController();
		summaryController = ac;

		const key = cacheKey(pr);
		try {
			const repo = pr.repoSlug ?? app.repoSlug;
			const result = await app.fetchPRSummary(repo, pr.number, ac.signal);
			if (ac.signal.aborted) return;
			summaryCache.set(key, result);

			setPrsByRepo((prev) => {
				const repoPrs = prev[repo] ?? [];
				const updated = repoPrs.map((p) =>
					p.number === pr.number
						? {
								...p,
								mergeable: result.mergeable,
								reviewDecision: result.reviewDecision,
								requestedReviewers: result.requestedReviewers,
								isDraft: result.isDraft,
								headCommitSha: result.headCommitSha,
								lastCommitDate: result.lastCommitDate,
								additions: result.additions,
								deletions: result.deletions,
								labels: result.labels,
								assignees: result.assignees,
								updatedAt: result.updatedAt,
								comments: result.comments,
								threadsLoading: false,
							}
						: p,
				);
				return { ...prev, [repo]: updated };
			});

			const selected = selectedPr();
			if (selected && cacheKey(selected) === key) {
				setSummary(result);
			}
		} catch {
			// Non-fatal — summary just won't load (includes abort)
		}
	}

	async function startBackgroundThreadLoad(repos: string[]) {
		bgThreadsController?.abort();
		const ac = new AbortController();
		bgThreadsController = ac;

		const snapshot = prsByRepo();
		const queue: Array<{ repo: string; prNumber: number }> = [];
		for (const repo of repos) {
			for (const pr of snapshot[repo] ?? []) {
				if (pr.comments === undefined) {
					queue.push({ repo, prNumber: pr.number });
				}
			}
		}
		if (queue.length === 0) return;

		setPrsByRepo((prev) => {
			const next = { ...prev };
			for (const repo of repos) {
				next[repo] = (prev[repo] ?? []).map((pr) =>
					pr.comments !== undefined ? pr : { ...pr, threadsLoading: true },
				);
			}
			return next;
		});

		const results = new Map<string, CommentCounts>();

		const worker = async () => {
			while (queue.length > 0 && !ac.signal.aborted) {
				const item = queue.shift()!;

				const current = (prsByRepo()[item.repo] ?? []).find(
					(p) => p.number === item.prNumber,
				);
				if (current?.comments !== undefined) continue;

				try {
					const counts = await app.fetchThreadCounts(item.repo, item.prNumber, ac.signal);
					if (ac.signal.aborted) return;
					results.set(`${item.repo}#${item.prNumber}`, counts);
				} catch {
					if (ac.signal.aborted) return;
				}
			}
		};

		await Promise.all(
			Array.from({ length: Math.min(THREAD_CONCURRENCY, queue.length) }, () => worker()),
		);

		if (ac.signal.aborted) return;

		setPrsByRepo((prev) => {
			const next = { ...prev };
			for (const repo of repos) {
				next[repo] = (prev[repo] ?? []).map((pr) => {
					const counts = results.get(`${repo}#${pr.number}`);
					if (counts) {
						return { ...pr, comments: counts, threadsLoading: false };
					}
					return pr.threadsLoading ? { ...pr, threadsLoading: false } : pr;
				});
			}
			return next;
		});
	}

	function selectPr(pr: PR) {
		setSelectedPr(pr);
		clearTimeout(debounceTimer);
		summaryController?.abort();

		const key = cacheKey(pr);
		const cached = summaryCache.get(key);
		if (cached) {
			setSummary(cached);
			return;
		}

		setSummary(undefined);
		if (summaryDebounceMs <= 0) {
			void fetchSummary(pr);
		} else {
			debounceTimer = setTimeout(() => fetchSummary(pr), summaryDebounceMs);
		}
	}

	function changeTab(index: number) {
		setActiveTab(index);
		const visible = prs();
		const first = visible[0];
		if (first) {
			selectPr(first);
		} else {
			clearTimeout(debounceTimer);
			summaryController?.abort();
			setSelectedPr(undefined);
			setSummary(undefined);
		}
	}

	function refreshSelected() {
		const pr = selectedPr();
		if (!pr) return;
		clearTimeout(debounceTimer);
		summaryController?.abort();
		summaryCache.delete(cacheKey(pr));
		setSummary(undefined);
		void fetchSummary(pr);
	}

	function refreshAll() {
		clearTimeout(debounceTimer);
		summaryController?.abort();
		app.reloadConfig();
		void loadPRs({ resetActiveTab: true });
	}

	onMount(() => {
		void loadPRs();
	});

	onCleanup(() => {
		for (const c of repoControllers.values()) c.abort();
		summaryController?.abort();
		bgThreadsController?.abort();
		clearTimeout(debounceTimer);
	});

	return {
		prs,
		tabs,
		activeTab,
		selectedPr,
		summary,
		loading,
		error,
		showRepo,
		selectPr,
		changeTab,
		refreshAll,
		refreshSelected,
	};
}
