import { createSignal, onMount, onCleanup } from "solid-js";
import { AppShell } from "./components/AppShell";
import type { Legit } from "./lib/legit";
import type { PR } from "./lib/types";

export interface AppProps {
	app: Legit;
}

export function App(props: AppProps) {
	const [error, setError] = createSignal("");
	const [prs, setPrs] = createSignal<PR[]>([]);
	const [loading, setLoading] = createSignal(true);

	let controller: AbortController | undefined;

	async function loadPRs() {
		controller?.abort();
		const ac = new AbortController();
		controller = ac;
		setPrs([]);
		setLoading(true);
		setError("");
		try {
			for await (const snapshot of props.app.fetchPRs(undefined, ac.signal)) {
				setPrs(snapshot);
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

	onMount(loadPRs);

	onCleanup(() => {
		controller?.abort();
	});

	function handleRefreshSelected() {
		// Summary panel cache invalidation will be wired in Task 11
	}

	function handleRefreshAll() {
		loadPRs();
	}

	return (
		<AppShell
			prs={prs()}
			loading={loading()}
			repoSlug={props.app.repoSlug}
			error={error()}
			onRefreshSelected={handleRefreshSelected}
			onRefreshAll={handleRefreshAll}
		/>
	);
}

/**
 * Create a render-ready App component bound to a Legit instance.
 * Used by cli.ts (which is .ts, not .tsx) to avoid JSX in the entry point.
 */
export function createApp(app: Legit) {
	return () => <App app={app} />;
}
