import { createSignal, createEffect, onCleanup } from "solid-js";
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

	let loadId = 0;

	async function loadPRs() {
		const myId = ++loadId;
		setPrs([]);
		setLoading(true);
		setError("");
		try {
			for await (const snapshot of props.app.fetchPRs()) {
				if (myId !== loadId) return;
				setPrs(snapshot);
			}
		} catch (err: any) {
			if (myId === loadId) {
				setError(err.message ?? String(err));
			}
		} finally {
			if (myId === loadId) {
				setLoading(false);
			}
		}
	}

	createEffect(() => {
		loadPRs();
	});

	onCleanup(() => {
		loadId++;
	});

	function handleRefresh() {
		loadPRs();
	}

	return (
		<AppShell
			prs={prs()}
			loading={loading()}
			repoSlug={props.app.repoSlug}
			error={error()}
			onRefresh={handleRefresh}
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
