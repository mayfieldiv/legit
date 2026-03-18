import { createSignal, createEffect, Show } from "solid-js";
import { useKeyboard } from "@opentui/solid";
import { PRList } from "./PRList";
import type { PR } from "../lib/types";

interface AppShellProps {
	prs: PR[];
	loading: boolean;
	repoSlug: string;
	error?: string;
	onRefresh: () => void;
}

export function AppShell(props: AppShellProps) {
	const [selectedIndex, setSelectedIndex] = createSignal(0);

	// Clamp selectedIndex when PR list changes (refresh, initial load, etc.)
	createEffect(() => {
		const maxIndex = Math.max(0, props.prs.length - 1);
		setSelectedIndex((i) => Math.min(i, maxIndex));
	});

	useKeyboard((event) => {
		const name = event.name;

		if (name === "j" || name === "down") {
			if (props.prs.length > 0) {
				setSelectedIndex((i) => Math.min(i + 1, props.prs.length - 1));
			}
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
			<Show when={props.error}>
				<text>
					<span color="red">Error: {props.error}</span>
				</text>
			</Show>
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
