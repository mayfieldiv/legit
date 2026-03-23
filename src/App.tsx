import { createSignal, onMount, onCleanup } from "solid-js";
import { AppShell } from "./components/AppShell";
import type { Legit } from "./lib/legit";
import type { PR, PRSummary } from "./lib/types";

export interface AppProps {
	app: Legit;
}

export function App(props: AppProps) {
	const [error, setError] = createSignal("");
	const [prs, setPrs] = createSignal<PR[]>([]);
	const [loading, setLoading] = createSignal(true);
	const [selectedPr, setSelectedPr] = createSignal<PR | undefined>();
	const [summary, setSummary] = createSignal<PRSummary | undefined>();

	let controller: AbortController | undefined;
	let summaryController: AbortController | undefined;
	let debounceTimer: ReturnType<typeof setTimeout> | undefined;
	/** Session cache (not keyed by commit); cleared on refresh. */
	const summaryCache = new Map<string, PRSummary>();

	function cacheKey(pr: PR): string {
		return `${props.app.repoSlug}#${pr.number}`;
	}

	async function loadPRs() {
		controller?.abort();
		const ac = new AbortController();
		controller = ac;
		setPrs([]);
		setLoading(true);
		setError("");
		summaryCache.clear();
		setSummary(undefined);
		setSelectedPr(undefined);
		try {
			let first = true;
			for await (const snapshot of props.app.fetchPRs(undefined, ac.signal)) {
				setPrs(snapshot);
				if (first && snapshot.length > 0) {
					first = false;
					handleSelectionChange(snapshot[0]!);
				}
			}
		} catch (err: any) {
			if (!ac.signal.aborted) {
				setError(err.message ?? String(err));
			}
		} finally {
			if (!ac.signal.aborted) {
				setLoading(false);
			}
		}
	}

	async function fetchSummary(pr: PR) {
		summaryController?.abort();
		const ac = new AbortController();
		summaryController = ac;

		const key = cacheKey(pr);
		try {
			const result = await props.app.fetchPRSummary(props.app.repoSlug, pr.number, ac.signal);
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
		const pr = selectedPr();
		if (!pr) return;
		clearTimeout(debounceTimer);
		summaryCache.delete(cacheKey(pr));
		setSummary(undefined);
		fetchSummary(pr);
	}

	function handleRefreshAll() {
		clearTimeout(debounceTimer);
		summaryController?.abort();
		loadPRs();
	}

	onMount(loadPRs);
	onCleanup(() => {
		controller?.abort();
		summaryController?.abort();
		clearTimeout(debounceTimer);
	});

	return (
		<AppShell
			prs={prs()}
			loading={loading()}
			repoSlug={props.app.repoSlug}
			error={error()}
			onRefreshSelected={handleRefreshSelected}
			onRefreshAll={handleRefreshAll}
			onSelectionChange={handleSelectionChange}
			selectedPr={selectedPr()}
			summary={summary()}
		/>
	);
}

export function createApp(app: Legit) {
	return () => <App app={app} />;
}
