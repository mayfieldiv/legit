import { useKeyboard } from "@opentui/solid";
import { createSignal, createMemo, createEffect, on, Show } from "solid-js";
import { PRList, PRListHeader, buildFlatItems, prIndexToDisplayRow } from "./PRList";
import type { FlatItem } from "./PRList";
import { GroupPanel, GROUP_BY_OPTIONS } from "./GroupPanel";
import { createListSelection } from "../lib/list-selection";
import { processPRList } from "../lib/group-filter-engine";
import type { GroupByKey } from "../lib/group-filter-engine";
import type { PR } from "../lib/types";
import type { ViewTarget } from "./AppShell";
import type { ScrollBoxRenderable } from "@opentui/core";

interface ListViewProps {
	prs: PR[];
	showRepo?: boolean;
	currentUser?: string;
	/** Initial grouping key. Default: "none". */
	groupBy?: GroupByKey;
	/** When this value changes, the selection resets to index 0. */
	resetKey?: number | string;
	onRefreshSelected: () => void;
	onRefreshAll: () => void;
	onNavigate: (target: ViewTarget) => void;
	onSelectionChange?: (pr: PR) => void;
	onOpenInBrowser?: (pr: PR) => void;
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
	// ── Filter state ──────────────────────────────────────────────────────────
	const [filterText, setFilterText] = createSignal("");
	const [filterMode, setFilterMode] = createSignal(false);

	// ── Grouping state ────────────────────────────────────────────────────────
	const [activeGroupBy, setActiveGroupBy] = createSignal<GroupByKey>(props.groupBy ?? "none");
	const [panelOpen, setPanelOpen] = createSignal(false);
	const [panelIndex, setPanelIndex] = createSignal(0);

	// ── Processed PR list ─────────────────────────────────────────────────────
	const processedResult = createMemo(() =>
		processPRList(props.prs, {
			groupBy: activeGroupBy(),
			filterText: filterText(),
			currentUser: props.currentUser,
		}),
	);

	/** Flat list of matched PRs (for selection tracking). */
	const displayPRs = createMemo<PR[]>(() => processedResult().groups.flatMap((g) => g.prs));

	/** Full flat items list including group headers. */
	const flatItems = createMemo<FlatItem[]>(() => buildFlatItems(processedResult().groups));

	// ── Selection ─────────────────────────────────────────────────────────────
	const selection = createListSelection(() => displayPRs().length);
	let scrollRef: ScrollBoxRenderable | undefined;

	// Reset when tab/dataset changes
	createEffect(
		on(
			() => props.resetKey,
			() => {
				selection.select(0);
				scrollRef?.scrollTo(0);
			},
			{ defer: true },
		),
	);

	// Reset when filter changes
	createEffect(
		on(
			() => filterText(),
			() => {
				selection.select(0);
				scrollRef?.scrollTo(0);
			},
			{ defer: true },
		),
	);

	// Reset when groupBy changes
	createEffect(
		on(
			() => activeGroupBy(),
			() => {
				selection.select(0);
				scrollRef?.scrollTo(0);
			},
			{ defer: true },
		),
	);

	// ── Scroll sync ───────────────────────────────────────────────────────────

	/** Display row of the selected PR (accounts for group header rows). */
	const displayRow = createMemo(() => prIndexToDisplayRow(flatItems(), selection.index()));

	function ensureVisible(direction: "up" | "down") {
		if (!scrollRef) return;
		const target = computeScrollTarget({
			idx: displayRow(),
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
			const pr = selection.selectedItem(displayPRs());
			if (pr) props.onSelectionChange?.(pr);
		}
	}

	function selectIndex(index: number) {
		selection.select(index);
		const pr = selection.selectedItem(displayPRs());
		if (pr) props.onSelectionChange?.(pr);
	}

	// ── Panel helpers ─────────────────────────────────────────────────────────

	function applyPanelSelection() {
		const opt = GROUP_BY_OPTIONS[panelIndex()];
		if (opt) setActiveGroupBy(opt.key);
		setPanelOpen(false);
	}

	// ── Keyboard ──────────────────────────────────────────────────────────────

	useKeyboard((event) => {
		const name = event.name;

		// Grouping panel has priority over all other keys
		if (panelOpen()) {
			if (name === "j" || name === "down") {
				setPanelIndex((i) => Math.min(i + 1, GROUP_BY_OPTIONS.length - 1));
			} else if (name === "k" || name === "up") {
				setPanelIndex((i) => Math.max(i - 1, 0));
			} else if (name === "return") {
				applyPanelSelection();
			} else if (name === "escape") {
				setPanelOpen(false);
			}
			return;
		}

		// Filter mode: navigation via arrow keys only; j/k fall through to text input
		if (filterMode()) {
			if (name === "down") {
				navigate("down");
				return;
			}
			if (name === "up") {
				navigate("up");
				return;
			}
			if (name === "return") {
				const pr = selection.selectedItem(displayPRs());
				if (pr) props.onNavigate({ view: "detail", pr });
				return;
			}
			if (name === "escape") {
				setFilterText("");
				setFilterMode(false);
				return;
			}
			if (name === "backspace") {
				setFilterText((t) => t.slice(0, -1));
				return;
			}
			if (name.length === 1) {
				setFilterText((t) => t + name);
				return;
			}
			return;
		}

		// Normal mode
		if (name === "j" || name === "down") {
			navigate("down");
		} else if (name === "k" || name === "up") {
			navigate("up");
		} else if (name === "r" && !event.shift) {
			props.onRefreshSelected();
		} else if ((name === "r" && event.shift) || name === "R") {
			props.onRefreshAll();
		} else if (name === "return") {
			const pr = selection.selectedItem(displayPRs());
			if (pr) {
				props.onNavigate({ view: "detail", pr });
			}
		} else if (name === "o") {
			const pr = selection.selectedItem(displayPRs());
			if (pr) props.onOpenInBrowser?.(pr);
		} else if (name === "/") {
			setFilterMode(true);
		} else if (name === "g") {
			// Pre-select the current groupBy option in the panel
			const idx = GROUP_BY_OPTIONS.findIndex((o) => o.key === activeGroupBy());
			setPanelIndex(idx >= 0 ? idx : 0);
			setPanelOpen(true);
		}
	});

	// ── Render ────────────────────────────────────────────────────────────────

	return (
		<box flexDirection="column" flexGrow={1} width="100%">
			<PRListHeader showRepo={props.showRepo} currentUser={props.currentUser} />

			{/* Filter bar — visible when filter mode is active */}
			<Show when={filterMode()}>
				<box height={1} width="100%">
					<text>
						<span style={{ fg: "cyan" }}>Filter: </span>
						<span>{filterText()}</span>
						<span style={{ fg: "cyan" }}>█</span>
						<span style={{ fg: "gray" }}> Esc to clear</span>
					</text>
				</box>
			</Show>

			{/* Grouping panel overlay — replaces the list when open */}
			<Show
				when={!panelOpen()}
				fallback={
					<GroupPanel currentGroupBy={activeGroupBy()} selectedIndex={panelIndex()} />
				}
			>
				<Show
					when={displayPRs().length > 0 || filterText() === ""}
					fallback={
						<box height={1}>
							<text>
								<span style={{ fg: "gray" }}>No matching PRs</span>
							</text>
						</box>
					}
				>
					<scrollbox
						ref={(el: ScrollBoxRenderable) => {
							scrollRef = el;
						}}
						flexGrow={1}
						width="100%"
					>
						<PRList
							items={flatItems()}
							selectedIndex={selection.index()}
							showRepo={props.showRepo}
							currentUser={props.currentUser}
							onSelect={selectIndex}
						/>
					</scrollbox>
				</Show>
			</Show>
		</box>
	);
}
