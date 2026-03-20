import { createSignal, Show, Switch, Match } from "solid-js";
import { ListView } from "./ListView";
import type { PR } from "../lib/types";

export type ViewTarget = { view: "list" } | { view: "detail"; pr: PR };

interface AppShellProps {
	prs: PR[];
	loading: boolean;
	loadingMessage?: string;
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
					<span bold color="cyan">
						legit
					</span>
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

			{/* Content — hide when error with no data (first-load failure) */}
			<Show
				when={!props.loading && !(props.error && props.prs.length === 0)}
				fallback={
					<Show when={props.loading}>
						<text>
							<span color="yellow">
								{props.loadingMessage ?? "Loading pull requests..."}
							</span>
						</text>
					</Show>
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
