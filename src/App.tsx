import { createResource } from "solid-js";
import { AppShell } from "./components/AppShell";
import { Legit } from "./lib/legit";
import type { PR } from "./lib/types";

const app = new Legit();

const App = () => {
	const [prs, { refetch }] = createResource<PR[]>(
		async () => app.fetchPRs(),
		{ initialValue: [] },
	);

	return (
		<AppShell
			prs={prs() ?? []}
			loading={prs.loading}
			repoSlug={app.repoSlug}
			error={prs.error?.message}
			onRefresh={refetch}
		/>
	);
};

export default App;
