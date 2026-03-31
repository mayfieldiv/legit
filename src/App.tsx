import { createSignal } from "solid-js";
import { execFile } from "child_process";
import { AppShell } from "./components/AppShell";
import { createPRStore } from "./lib/pr-store";
import type { Legit } from "./lib/legit";
import type { PR } from "./lib/types";

/** Build a GitHub PR URL from a repo slug and PR number. */
export function prUrl(repoSlug: string, number: number): string {
	return `https://github.com/${repoSlug}/pull/${number}`;
}

export interface AppProps {
	app: Legit;
}

export function App(props: AppProps) {
	const store = createPRStore(props.app);

	const [browserError, setBrowserError] = createSignal("");

	function handleOpenInBrowser(pr: PR) {
		setBrowserError("");
		execFile("open", [prUrl(pr.repoSlug ?? props.app.repoSlug, pr.number)], (err) => {
			if (err) setBrowserError(`Failed to open browser: ${err.message}`);
		});
	}

	const displayRepoSlug = () => {
		const tab = store.activeTab();
		return tab === 0 ? "All repos" : (store.tabs()[tab] ?? "All repos");
	};

	return (
		<AppShell
			view={store.view()}
			prs={store.prs()}
			loading={store.loading()}
			repoSlug={displayRepoSlug()}
			showRepo={store.showRepo()}
			currentUser={props.app.currentUser}
			resetKey={store.activeTab()}
			error={store.error() || browserError()}
			tabs={store.tabs()}
			activeTab={store.activeTab()}
			selectedPr={store.selectedPr()}
			summary={store.summary()}
			onSelectionChange={store.selectPr}
			onTabChange={store.changeTab}
			onRefreshAllActive={store.refreshAllActive}
			onRefreshSelected={store.refreshSelected}
			onEnterDetail={store.enterDetail}
			onOpenInBrowser={handleOpenInBrowser}
		/>
	);
}

export function createApp(app: Legit) {
	return () => <App app={app} />;
}
