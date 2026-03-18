import { useKeyboard } from "@opentui/solid";
import { PRList } from "./PRList";
import { createListSelection } from "../lib/list-selection";
import type { PR } from "../lib/types";
import type { ViewTarget } from "./AppShell";

interface ListViewProps {
	prs: PR[];
	onRefresh: () => void;
	onNavigate: (target: ViewTarget) => void;
}

export function ListView(props: ListViewProps) {
	const selection = createListSelection(() => props.prs.length);

	useKeyboard((event) => {
		const name = event.name;

		if (name === "j" || name === "down") {
			selection.moveDown();
		} else if (name === "k" || name === "up") {
			selection.moveUp();
		} else if (name === "r") {
			props.onRefresh();
		} else if (name === "return") {
			const pr = selection.selectedItem(props.prs);
			if (pr) {
				props.onNavigate({ view: "detail", pr });
			}
		}
	});

	return <PRList prs={props.prs} selectedIndex={selection.index()} />;
}
