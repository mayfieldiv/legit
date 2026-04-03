import { useKeyboard } from "@opentui/solid";
import { createSignal, createMemo, createEffect, on, Show } from "solid-js";
import { PRList, PRListHeader, buildFlatItems, prIndexToDisplayRow } from "./PRList";
import type { FlatItem } from "./PRList";
import { GroupPanel, GROUP_BY_OPTIONS } from "./GroupPanel";
import { createListSelection } from "../lib/list-selection";
import { processPRList } from "../lib/group-filter-engine";
import type { GroupByKey } from "../lib/group-filter-engine";
import type { PR } from "../lib/types";
import type { ScrollBoxRenderable } from "@opentui/core";
import { StatusBar } from "./StatusBar";

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
	onEnterDetail: (pr: PR) => void;
	onSelectionChange?: (pr: PR) => void;
	onOpenInBrowser?: (pr: PR) => void;
	onOpenInDevin?: (pr: PR) => void;
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
	/** True while the user is actively typing a filter query. */
	const [filterEditing, setFilterEditing] = createSignal(false);
	/** True when a filter is applied (text submitted but not editing). */
	const filterActive = () => !filterEditing() && filterText() !== "";

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

	/** The currently selected PR, derived reactively from the index + display list. */
	const selectedPR = createMemo(() => selection.selectedItem(displayPRs()));

	// Notify parent whenever the selected PR changes identity.
	createEffect(
		on(selectedPR, (pr) => {
			if (pr) {
				_anchor = { repoSlug: pr.repoSlug, number: pr.number };
				props.onSelectionChange?.(pr);
			}
		}),
	);

	// ── Selection anchoring ────────────────────────────────────────────────────
	// When background data arrives and re-groups the list, keep the highlight on
	// the same PR by identity (repo slug + number) rather than the same index.
	// `_anchor` is set whenever the user explicitly changes selection and is
	// cleared on tab/reset so a fresh selection can be established.
	let _anchor: { repoSlug: string | undefined; number: number } | null = null;

	function prMatchesAnchor(pr: PR): boolean {
		return _anchor !== null && pr.number === _anchor.number && pr.repoSlug === _anchor.repoSlug;
	}

	// Re-anchor the selection index whenever the displayed list changes.
	createEffect(
		on(
			displayPRs,
			(prs) => {
				if (_anchor === null) return;

				// If the current selection already points to the right PR, nothing to do.
				const current = selection.selectedItem(prs);
				if (current && prMatchesAnchor(current)) return;

				// Find the anchored PR in the (possibly re-ordered) list and move to it.
				const idx = prs.findIndex(prMatchesAnchor);
				if (idx >= 0 && idx !== selection.index()) {
					const prevIdx = selection.index();
					selection.select(idx);
					ensureVisible(idx >= prevIdx ? "down" : "up");
				}
			},
			{ defer: true },
		),
	);

	// Reset when tab/dataset changes — clear anchor so it reinitialises to the new first PR.
	createEffect(
		on(
			() => props.resetKey,
			() => {
				_anchor = null;
				selection.select(0);
				scrollRef?.scrollTo(0);
			},
			{ defer: true },
		),
	);

	// Reset when filter changes — try to keep the same PR, fall back to index 0.
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
		}
	}

	function selectIndex(index: number) {
		selection.select(index);
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

		// Filter editing: typing characters into the filter input
		if (filterEditing()) {
			if (name === "down") {
				navigate("down");
				return;
			}
			if (name === "up") {
				navigate("up");
				return;
			}
			if (name === "return") {
				// Submit: lock in the filter and return to normal navigation
				if (filterText()) {
					setFilterEditing(false);
				} else {
					// Empty filter — just exit editing
					setFilterEditing(false);
				}
				return;
			}
			if (name === "escape") {
				setFilterText("");
				setFilterEditing(false);
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

		// Filter active (submitted): normal nav but Esc exits filter
		if (filterActive()) {
			if (name === "escape") {
				setFilterText("");
				return;
			}
			// Fall through to normal mode for all other keys
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
				props.onEnterDetail(pr);
			}
		} else if (name === "o") {
			const pr = selection.selectedItem(displayPRs());
			if (pr) props.onOpenInBrowser?.(pr);
		} else if (name === "d") {
			const pr = selection.selectedItem(displayPRs());
			if (pr) props.onOpenInDevin?.(pr);
		} else if (name === "/") {
			setFilterEditing(true);
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

			{/* Filter bar — editing mode (typing) */}
			<Show when={filterEditing()}>
				<box height={1} width="100%">
					<text>
						<span style={{ fg: "cyan" }}>Filter: </span>
						<span>{filterText()}</span>
						<span style={{ fg: "cyan" }}>█</span>
						<span style={{ fg: "gray" }}> Enter to submit · Esc to clear</span>
					</text>
				</box>
			</Show>

			{/* Filter bar — active mode (submitted) */}
			<Show when={filterActive()}>
				<box height={1} width="100%">
					<text>
						<span style={{ fg: "cyan" }}>Filter: </span>
						<span>matches for </span>
						<span style={{ fg: "cyan" }}>'{filterText()}'</span>
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

			{/* ── Status bar ──────────────────────────────────────── */}
			<StatusBar>{" · "}/ filter · g group</StatusBar>
		</box>
	);
}
