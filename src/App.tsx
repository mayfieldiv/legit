import { createSignal, onMount } from "solid-js";
import { AppShell } from "./components/AppShell";
import { Legit } from "./lib/legit";
import type { PR } from "./lib/types";

const app = new Legit();

const App = () => {
	const [prs, setPrs] = createSignal<PR[]>([]);
	const [loading, setLoading] = createSignal(true);
	const [error, setError] = createSignal("");

	async function fetchPRs() {
		try {
			setLoading(true);
			setError("");
			const data = await app.fetchPRs();
			setPrs(data);
		} catch (err: any) {
			setError(err.message ?? String(err));
		} finally {
			setLoading(false);
		}
	}

	onMount(() => {
		fetchPRs();
	});

	return (
		<AppShell
			prs={prs()}
			loading={loading()}
			repoSlug={app.repoSlug}
			error={error()}
			onRefresh={fetchPRs}
		/>
	);
};

export default App;
