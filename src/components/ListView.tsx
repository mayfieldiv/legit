import { useKeyboard } from "@opentui/solid";
import { PRList, PRListHeader } from "./PRList";
import { createListSelection } from "../lib/list-selection";
import type { PR } from "../lib/types";
import type { ViewTarget } from "./AppShell";
import type { ScrollBoxRenderable } from "@opentui/core";

interface ListViewProps {
	prs: PR[];
	onRefresh: () => void;
	onNavigate: (target: ViewTarget) => void;
}

export function ListView(props: ListViewProps) {
	const selection = createListSelection(() => props.prs.length);
	let scrollRef: ScrollBoxRenderable | undefined;

	useKeyboard((event) => {
		const name = event.name;

		if (name === "j" || name === "down") {
			selection.moveDown();
			scrollRef?.scrollBy(1);
		} else if (name === "k" || name === "up") {
			selection.moveUp();
			scrollRef?.scrollBy(-1);
		} else if (name === "r") {
			props.onRefresh();
		} else if (name === "return") {
			const pr = selection.selectedItem(props.prs);
			if (pr) {
				props.onNavigate({ view: "detail", pr });
			}
		}
	});

	return (
		<box flexDirection="column" flexGrow={1} width="100%">
			<PRListHeader />
			<scrollbox
				ref={(el: ScrollBoxRenderable) => { scrollRef = el; }}
				flexGrow={1}
				width="100%"
			>
				<PRList prs={props.prs} selectedIndex={selection.index()} />
			</scrollbox>
		</box>
	);
}
