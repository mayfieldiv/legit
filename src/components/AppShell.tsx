import { createSignal, Show } from "solid-js";
import { useKeyboard } from "@opentui/solid";
import { PRList } from "./PRList";
import type { PR } from "../lib/github-client";

interface AppShellProps {
	prs: PR[];
	loading: boolean;
	repoSlug: string;
	onRefresh: () => void;
}

export function AppShell(props: AppShellProps) {
	const [selectedIndex, setSelectedIndex] = createSignal(0);

	useKeyboard((event) => {
		const name = event.name;

		if (name === "j" || name === "down") {
			setSelectedIndex((i) => Math.min(i + 1, props.prs.length - 1));
		} else if (name === "k" || name === "up") {
			setSelectedIndex((i) => Math.max(i - 1, 0));
		} else if (name === "r") {
			props.onRefresh();
		}
	});

	return (
		<box flexDirection="column" width="100%" height="100%">
			{/* Header */}
			<box flexDirection="row" width="100%">
				<text>
					<span bold color="cyan">legit</span>
					<span> — </span>
					<span bold>{props.repoSlug}</span>
					<span> — {props.prs.length} open PRs</span>
				</text>
			</box>

			{/* Content */}
			<Show
				when={!props.loading}
				fallback={
					<text>
						<span color="yellow">Loading pull requests...</span>
					</text>
				}
			>
				<PRList prs={props.prs} selectedIndex={selectedIndex()} />
			</Show>
		</box>
	);
}
