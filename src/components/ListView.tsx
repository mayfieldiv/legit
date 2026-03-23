import { useKeyboard } from "@opentui/solid";
import { PRList, PRListHeader } from "./PRList";
import { createListSelection } from "../lib/list-selection";
import type { PR } from "../lib/types";
import type { ViewTarget } from "./AppShell";
import type { ScrollBoxRenderable } from "@opentui/core";

interface ListViewProps {
	prs: PR[];
	onRefreshSelected: () => void;
	onRefreshAll: () => void;
	onNavigate: (target: ViewTarget) => void;
	onSelectionChange?: (pr: PR) => void;
}

/**
 * Compute the new scrollTop to keep the selection visible with a 10% margin.
 * Returns null if no scroll is needed.
 *
 * Handles both normal scrolling (selection drifts into margin zone) and
 * desync recovery (selection off-screen after mouse scroll).
 */
export interface ScrollInput {
	idx: number;
	scrollTop: number;
	viewportHeight: number;
	direction: "up" | "down";
}

export function computeScrollTarget({
	idx,
	scrollTop,
	viewportHeight,
	direction,
}: ScrollInput): number | null {
	const margin = Math.max(1, Math.floor(viewportHeight * 0.1));

	// Off-screen: position based on where selection is relative to viewport
	if (idx < scrollTop) {
		return Math.max(0, idx - margin);
	}
	if (idx >= scrollTop + viewportHeight) {
		return Math.max(0, idx - viewportHeight + 1 + margin);
	}

	// In margin zone: scroll in direction of travel
	if (direction === "down" && idx > scrollTop + viewportHeight - 1 - margin) {
		return Math.max(0, idx - viewportHeight + 1 + margin);
	}
	if (direction === "up" && idx < scrollTop + margin) {
		return Math.max(0, idx - margin);
	}

	return null;
}

export function ListView(props: ListViewProps) {
	const selection = createListSelection(() => props.prs.length);
	let scrollRef: ScrollBoxRenderable | undefined;

	function ensureVisible(direction: "up" | "down") {
		if (!scrollRef) return;
		const target = computeScrollTarget({
			idx: selection.index(),
			scrollTop: scrollRef.scrollTop,
			viewportHeight: scrollRef.viewport.height,
			direction,
		});
		if (target !== null) {
			scrollRef.scrollTo(target);
		}
	}

	function navigate(direction: "up" | "down") {
		const prev = selection.index();
		if (direction === "down") selection.moveDown();
		else selection.moveUp();
		if (selection.index() !== prev) {
			ensureVisible(direction);
			const pr = selection.selectedItem(props.prs);
			if (pr) props.onSelectionChange?.(pr);
		}
	}

	function selectIndex(index: number) {
		selection.select(index);
		const pr = selection.selectedItem(props.prs);
		if (pr) props.onSelectionChange?.(pr);
	}

	useKeyboard((event) => {
		const name = event.name;

		if (name === "j" || name === "down") {
			navigate("down");
		} else if (name === "k" || name === "up") {
			navigate("up");
		} else if (name === "r" && !event.shift) {
			props.onRefreshSelected();
		} else if ((name === "r" && event.shift) || name === "R") {
			props.onRefreshAll();
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
				ref={(el: ScrollBoxRenderable) => {
					scrollRef = el;
				}}
				flexGrow={1}
				width="100%"
			>
				<PRList prs={props.prs} selectedIndex={selection.index()} onSelect={selectIndex} />
			</scrollbox>
		</box>
	);
}
