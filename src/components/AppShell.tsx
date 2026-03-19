import { createSignal, Show, Switch, Match } from "solid-js";
import { ListView } from "./ListView";
import type { PR } from "../lib/types";

export type ViewTarget =
	| { view: "list" }
	| { view: "detail"; pr: PR };

interface AppShellProps {
	prs: PR[];
	loading: boolean;
	repoSlug: string;
	error?: string;
	onRefresh: () => void;
}

export function AppShell(props: AppShellProps) {
	const [view, setView] = createSignal<ViewTarget>({ view: "list" });

	return (
		<box flexDirection="column" width="100%" height="100%">
			{/* Header */}
			<box flexDirection="row" width="100%" height={1}>
				<text>
					<span bold color="cyan">legit</span>
					<span> — </span>
					<span bold>{props.repoSlug}</span>
					<span> — {props.prs.length} open PRs</span>
				</text>
			</box>

			{/* Error */}
			<Show when={props.error}>
				<text>
					<span color="red">Error: {props.error}</span>
				</text>
			</Show>

			{/* Content */}
			<Show
				when={!props.loading}
				fallback={
					<text>
						<span color="yellow">Loading pull requests...</span>
					</text>
				}
			>
				<Switch>
					<Match when={view().view === "list"}>
						<ListView
							prs={props.prs}
							onRefresh={props.onRefresh}
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
