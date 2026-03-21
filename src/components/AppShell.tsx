import { createSignal, Show, Switch, Match } from "solid-js";
import { ListView } from "./ListView";
import type { PR } from "../lib/types";

export type ViewTarget = { view: "list" } | { view: "detail"; pr: PR };

interface AppShellProps {
	prs: PR[];
	loading: boolean;
	repoSlug: string;
	error?: string;
	onRefreshSelected: () => void;
	onRefreshAll: () => void;
}

export function AppShell(props: AppShellProps) {
	const [view, setView] = createSignal<ViewTarget>({ view: "list" });

	return (
		<box flexDirection="column" width="100%" height="100%">
			{/* Header */}
			<box flexDirection="row" width="100%" height={1}>
				<text>
					<span style={{ fg: "cyan", bold: true }}>legit</span>
					<span> — </span>
					<b>{props.repoSlug}</b>
					<span> — {props.prs.length} open PRs</span>
				</text>
			</box>

			{/* Error */}
			<Show when={props.error}>
				<text>
					<span style={{ fg: "red" }}>Error: {props.error}</span>
				</text>
			</Show>

			{/* Content — hide when error with no data (first-load failure) */}
			<Show
				when={props.prs.length > 0 || (!props.loading && !props.error)}
				fallback={
					<Show when={props.loading}>
						<text>
							<span style={{ fg: "yellow" }}>Loading pull requests...</span>
						</text>
					</Show>
				}
			>
				<Switch>
					<Match when={view().view === "list"}>
						<ListView
							prs={props.prs}
							onRefreshSelected={props.onRefreshSelected}
							onRefreshAll={props.onRefreshAll}
							onNavigate={setView}
						/>
					</Match>
					<Match when={view().view === "detail"}>
						{/* DetailView placeholder — slice #7 */}
						<text>Detail view (not yet implemented)</text>
					</Match>
				</Switch>
			</Show>
		</box>
	);
}
