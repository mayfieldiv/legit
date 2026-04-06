/**
 * Pure navigation/UI state — replaces the data orchestration in pr-store.ts.
 * All data fetching is now handled by TanStack Query; this module manages
 * only view state, tab selection, and detail view toggles.
 */

import { batch, createSignal, type Accessor } from "solid-js";
import type { PR } from "./types";

export type ViewTarget = { view: "list" } | { view: "detail"; pr: PR };

export interface UIState {
	readonly view: Accessor<ViewTarget>;
	readonly activeTab: Accessor<number>;
	readonly showResolved: Accessor<boolean>;
	readonly showBotComments: Accessor<boolean>;

	changeTab(index: number): void;
	enterDetail(pr: PR): void;
	exitDetail(): void;
	toggleResolved(): void;
	toggleBotComments(): void;
}

export function createUIState(): UIState {
	const [view, setView] = createSignal<ViewTarget>({ view: "list" });
	const [activeTab, setActiveTab] = createSignal(0);
	const [showResolved, setShowResolved] = createSignal(false);
	const [showBotComments, setShowBotComments] = createSignal(true);

	function changeTab(index: number) {
		setActiveTab(index);
	}

	function enterDetail(pr: PR) {
		setView({ view: "detail", pr });
	}

	function exitDetail() {
		batch(() => {
			setView({ view: "list" });
			setShowResolved(false);
			setShowBotComments(true);
		});
	}

	function toggleResolved() {
		setShowResolved((v) => !v);
	}

	function toggleBotComments() {
		setShowBotComments((v) => !v);
	}

	return {
		view,
		activeTab,
		showResolved,
		showBotComments,
		changeTab,
		enterDetail,
		exitDetail,
		toggleResolved,
		toggleBotComments,
	};
}
