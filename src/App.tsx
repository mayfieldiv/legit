import { createSignal, onMount, onCleanup } from "solid-js";
import { AppShell } from "./components/AppShell";
import type { Legit } from "./lib/legit";
import type { PR, PRSummary } from "./lib/types";

export interface AppProps {
	app: Legit;
}

export function App(props: AppProps) {
	const [error, setError] = createSignal("");
	const [repoTabs, setRepoTabs] = createSignal<string[]>([]);
	const [activeTab, setActiveTab] = createSignal(0);
	const [prsByRepo, setPrsByRepo] = createSignal<Record<string, PR[]>>({});
	const [prs, setPrs] = createSignal<PR[]>([]);
	const [loading, setLoading] = createSignal(true);
	const [selectedPr, setSelectedPr] = createSignal<PR | undefined>();
	const [summary, setSummary] = createSignal<PRSummary | undefined>();

	const repoControllers = new Map<string, AbortController>();
	let summaryController: AbortController | undefined;
	let debounceTimer: ReturnType<typeof setTimeout> | undefined;
	/** Session cache (not keyed by commit); cleared on refresh. */
	const summaryCache = new Map<string, PRSummary>();

	function currentRepoSlug(): string | undefined {
		if (activeTab() === 0) return undefined;
		return repoTabs()[activeTab() - 1];
	}

	function cacheKey(pr: PR): string {
		return `${pr.repoSlug ?? currentRepoSlug() ?? props.app.repoSlug}#${pr.number}`;
	}

	function tabs(): string[] {
		return ["All", ...repoTabs()];
	}

	function visiblePRsForTab(tabIndex = activeTab()): PR[] {
		const byRepo = prsByRepo();
		if (tabIndex === 0) {
			const merged: PR[] = [];
			for (const repo of repoTabs()) {
				const repoPrs = byRepo[repo] ?? [];
				for (const pr of repoPrs) {
					merged.push({ ...pr, repoSlug: repo });
				}
			}
			return merged;
		}
		const repo = repoTabs()[tabIndex - 1];
		return (repo ? byRepo[repo] : []) ?? [];
	}

	function updateDisplayedPRs() {
		setPrs(visiblePRsForTab());
	}

	function setRepoLoading(repo: string, value: boolean) {
		setLoading((prev) => {
			if (activeTab() === 0) {
				return value || prev;
			}
			if (currentRepoSlug() === repo) return value;
			return prev;
		});
	}

	async function loadRepo(repo: string) {
		repoControllers.get(repo)?.abort();
		const ac = new AbortController();
		repoControllers.set(repo, ac);
		setRepoLoading(repo, true);
		setError("");
		setPrsByRepo((prev) => ({ ...prev, [repo]: [] }));
		updateDisplayedPRs();
		try {
			for await (const snapshot of props.app.fetchPRs(repo, ac.signal)) {
				setPrsByRepo((prev) => ({ ...prev, [repo]: snapshot }));
				updateDisplayedPRs();
				if (!selectedPr() && snapshot.length > 0 && activeTab() === 0) {
					handleSelectionChange({ ...snapshot[0]!, repoSlug: repo });
				}
			}
		} catch (err: any) {
			if (!ac.signal.aborted) {
				setError(err.message ?? String(err));
			}
		} finally {
			if (!ac.signal.aborted) {
				setRepoLoading(repo, false);
			}
		}
	}

	function discoverRepos(): string[] {
		const repos = new Set<string>(props.app.config.repos);
		repos.add(props.app.repoSlug);
		return [...repos];
	}

	async function loadPRs() {
		for (const c of repoControllers.values()) c.abort();
		repoControllers.clear();
		setPrsByRepo({});
		setPrs([]);
		setLoading(true);
		setError("");
		summaryCache.clear();
		setSummary(undefined);
		setSelectedPr(undefined);
		const repos = discoverRepos();
		setRepoTabs(repos);
		await Promise.all(repos.map((repo) => loadRepo(repo)));
		setLoading(false);
	}

	async function fetchSummary(pr: PR) {
		summaryController?.abort();
		const ac = new AbortController();
		summaryController = ac;

		const key = cacheKey(pr);
		try {
			const repo = pr.repoSlug ?? currentRepoSlug() ?? props.app.repoSlug;
			const result = await props.app.fetchPRSummary(repo, pr.number, ac.signal);
			if (ac.signal.aborted) return;
			summaryCache.set(key, result);
			if (selectedPr()?.number === pr.number) {
				setSummary(result);
			}
		} catch {
			// Non-fatal — summary just won't load (includes abort)
		}
	}

	function handleSelectionChange(pr: PR) {
		setSelectedPr(pr);
		clearTimeout(debounceTimer);

		const key = cacheKey(pr);
		const cached = summaryCache.get(key);
		if (cached) {
			summaryController?.abort();
			setSummary(cached);
			return;
		}

		setSummary(undefined);
		debounceTimer = setTimeout(() => fetchSummary(pr), 300);
	}

	function handleRefreshSelected() {
		const repo = currentRepoSlug();
		clearTimeout(debounceTimer);
		summaryController?.abort();
		if (!repo) {
			loadPRs();
			return;
		}
		summaryCache.clear();
		setSummary(undefined);
		loadRepo(repo);
	}

	function handleRefreshAll() {
		clearTimeout(debounceTimer);
		summaryController?.abort();
		loadPRs();
	}

	onMount(loadPRs);
	onCleanup(() => {
		for (const c of repoControllers.values()) c.abort();
		summaryController?.abort();
		clearTimeout(debounceTimer);
	});

	return (
		<AppShell
			prs={prs()}
			loading={loading()}
			repoSlug={currentRepoSlug() ?? "All repos"}
			error={error()}
			onRefreshSelected={handleRefreshSelected}
			onRefreshAll={handleRefreshAll}
			onSelectionChange={handleSelectionChange}
			selectedPr={selectedPr()}
			summary={summary()}
			tabs={tabs()}
			activeTab={activeTab()}
			onTabChange={(index) => {
				setActiveTab(index);
				updateDisplayedPRs();
				const first = visiblePRsForTab(index)[0];
				if (first) {
					handleSelectionChange(first);
				} else {
					setSelectedPr(undefined);
					setSummary(undefined);
				}
			}}
		/>
	);
}

export function createApp(app: Legit) {
	return () => <App app={app} />;
}
