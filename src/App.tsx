import { createSignal, onMount } from "solid-js";
import { AppShell } from "./components/AppShell";
import { detectRepo } from "./lib/detect-repo";
import { resolveAuth } from "./lib/auth";
import { loadConfig, saveConfig, addRepo } from "./lib/config";
import { createGitHubClient, type PR } from "./lib/github-client";

const CONFIG_PATH =
	process.env.LEGIT_CONFIG_PATH ??
	`${process.env.HOME}/.config/legit/config.json`;

const App = () => {
	const [prs, setPrs] = createSignal<PR[]>([]);
	const [loading, setLoading] = createSignal(true);
	const [repoSlug, setRepoSlug] = createSignal("");
	const [error, setError] = createSignal("");

	async function fetchPRs() {
		try {
			setLoading(true);
			const repo = detectRepo();
			const slug = `${repo.owner}/${repo.repo}`;
			setRepoSlug(slug);

			const auth = resolveAuth();
			let config = loadConfig(CONFIG_PATH);

			if (!config.user) {
				config = { ...config, user: auth.user };
				saveConfig(CONFIG_PATH, config);
			}
			if (!config.repos.includes(slug)) {
				config = addRepo(config, slug);
				saveConfig(CONFIG_PATH, config);
			}

			const client = createGitHubClient(auth.token);
			const data = await client.fetchOpenPRs(slug);
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
			repoSlug={repoSlug()}
			error={error()}
			onRefresh={fetchPRs}
		/>
	);
};

export default App;
