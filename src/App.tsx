import { createResource, createSignal } from "solid-js";
import { AppShell } from "./components/AppShell";
import type { Legit } from "./lib/legit";
import type { PR } from "./lib/types";

export interface AppProps {
	app: Legit;
}

export function App(props: AppProps) {
	const [error, setError] = createSignal("");
	const [loadingMessage, setLoadingMessage] = createSignal("Loading pull requests...");

	const [prs, { refetch }] = createResource<PR[]>(
		async () => {
			try {
				setError("");
				setLoadingMessage("Loading pull requests...");
				return await props.app.fetchPRs(undefined, setLoadingMessage);
			} catch (err: any) {
				setError(err.message ?? String(err));
				return [];
			}
		},
		{ initialValue: [] },
	);

	return (
		<AppShell
			prs={prs()}
			loading={prs.loading}
			loadingMessage={loadingMessage()}
			repoSlug={props.app.repoSlug}
			error={error()}
			onRefresh={refetch}
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
