import { Show, Switch, Match, createMemo } from "solid-js";
import { useKeyboard, useTerminalDimensions } from "@opentui/solid";
import { ListView } from "./ListView";
import { SummaryPanel } from "./SummaryPanel";
import { DetailView } from "./DetailView";
import type {
	PR,
	PRDetail,
	CheckRun,
	Review,
	FullReviewThread,
	IssueComment,
	FileCategorization,
} from "../lib/types";
import type { GroupByKey } from "../lib/group-filter-engine";
import type { ViewTarget } from "../lib/ui-state";
import type { BlockerOptions } from "../lib/blocker-engine";
import { theme } from "../lib/theme";
import { computeVisibleColumns } from "./PRList";

export type { ViewTarget } from "../lib/ui-state";

interface AppShellProps {
	prs: PR[];
	loading: boolean;
	repoSlug: string;
	showRepo?: boolean;
	currentUser?: string;
	/** Initial grouping key for the list view. Default: "smart-status". */
	groupBy?: GroupByKey;
	resetKey?: number | string;
	view: ViewTarget;
	error?: string;
	onRefreshSelected: () => void;
	onRefreshAllActive: () => void;
	onSelectionChange?: (pr: PR) => void;
	onOpenInBrowser?: (pr: PR) => void;
	onOpenInDevin?: (pr: PR) => void;
	onEnterDetail: (pr: PR) => void;
	selectedPr?: PR;
	// Summary panel data (individual queries, no PRSummary)
	summaryThreads?: FullReviewThread[];
	summaryChecks?: CheckRun[];
	summaryReviews?: Review[];
	summaryFiles?: FileCategorization;
	summaryLoading?: boolean;
	/** Blocker data lookup for grouping engine. */
	getBlockerData?: (pr: PR) => BlockerOptions | undefined;
	// Detail view
	detailPr?: PRDetail;
	detailThreads?: FullReviewThread[];
	detailComments?: IssueComment[];
	detailLoading?: boolean;
	showResolved?: boolean;
	showBotComments?: boolean;
	onExitDetail?: () => void;
	onToggleResolved?: () => void;
	onToggleBotComments?: () => void;
	onOpenUrl?: (url: string) => void;
	onRefreshDetail?: () => void;
	tabs?: string[];
	activeTab?: number;
	onTabChange?: (index: number) => void;
}

/** Summary panel width: full at wide widths, narrower when tight, hidden when very narrow. */
const SUMMARY_FULL = 50;
const SUMMARY_NARROW = 36;
const SUMMARY_DIVIDER = 1;
/** Minimum terminal width to show the summary panel at all. */
const SUMMARY_MIN_TERM_WIDTH = 80;

export function AppShell(props: AppShellProps) {
	const tabCount = () => props.tabs?.length ?? 0;
	const inListView = () => props.view.view === "list";
	const dims = useTerminalDimensions();

	/** Whether to show the summary panel. */
	const showSummary = () => dims().width >= SUMMARY_MIN_TERM_WIDTH;

	/** Width of the summary panel (0 when hidden). */
	const summaryWidth = () => {
		if (!showSummary()) return 0;
		return dims().width >= 140 ? SUMMARY_FULL : SUMMARY_NARROW;
	};

	/** Available width for the list (excluding summary + divider). */
	const listWidth = () => {
		const sw = summaryWidth();
		return dims().width - (sw > 0 ? sw + SUMMARY_DIVIDER : 0);
	};

	/** Responsive column visibility. */
	const visibleColumns = createMemo(() =>
		computeVisibleColumns(listWidth(), props.showRepo ?? false),
	);

	useKeyboard((event) => {
		if (!inListView()) return;
		if (!props.onTabChange || tabCount() === 0) return;
		const current = props.activeTab ?? 0;
		const name = event.name;

		if (name === "l" || name === "right" || name === "]") {
			props.onTabChange(Math.min(tabCount() - 1, current + 1));
			return;
		}
		if (name === "h" || name === "left" || name === "[") {
			props.onTabChange(Math.max(0, current - 1));
			return;
		}

		if (name === "0") {
			props.onTabChange(0);
			return;
		}
		if (/^[1-9]$/.test(name)) {
			const index = Number(name);
			if (index < tabCount()) {
				props.onTabChange(index);
			}
		}
	});

	return (
		<box flexDirection="column" width="100%" height="100%">
			{/* Header */}
			<box flexDirection="row" width="100%" height={1}>
				<text>
					<span style={{ fg: theme.accent, bold: true }}>legit</span>
					<Show when={inListView()}>
						<span> — </span>
						<b>{props.repoSlug}</b>
						<span> — {props.prs.length} open PRs</span>
					</Show>
				</text>
			</box>

			<Show when={tabCount() > 0 && inListView()}>
				<box flexDirection="row" width="100%" height={1}>
					<text>
						{props.tabs!.map((tab, i) => {
							const selected = i === (props.activeTab ?? 0);
							return `${selected ? "[" : " "}${tab}${selected ? "]" : " "} `;
						})}
					</text>
				</box>
			</Show>

			{/* Error */}
			<Show when={props.error}>
				<text>
					<span style={{ fg: theme.error }}>Error: {props.error}</span>
				</text>
			</Show>

			{/* Content — hide when error with no data (first-load failure) */}
			<Show
				when={props.prs.length > 0 || (!props.loading && !props.error)}
				fallback={
					<Show when={props.loading}>
						<text>
							<span style={{ fg: theme.warning }}>Loading pull requests...</span>
						</text>
					</Show>
				}
			>
				<Switch>
					<Match when={props.view.view === "list"}>
						<box flexDirection="row" flexGrow={1} width="100%">
							<ListView
								prs={props.prs}
								showRepo={props.showRepo}
								currentUser={props.currentUser}
								groupBy={props.groupBy ?? "smart-status"}
								resetKey={props.resetKey}
								getBlockerData={props.getBlockerData}
								onRefreshSelected={props.onRefreshSelected}
								onRefreshAll={props.onRefreshAllActive}
								onEnterDetail={props.onEnterDetail}
								onSelectionChange={props.onSelectionChange}
								onOpenInBrowser={props.onOpenInBrowser}
								onOpenInDevin={props.onOpenInDevin}
								visibleColumns={visibleColumns()}
							/>
							<Show when={showSummary()}>
								<box width={1} height="100%">
									<text>│</text>
								</box>
								<box width={summaryWidth()}>
									<SummaryPanel
										pr={props.selectedPr}
										currentUser={props.currentUser}
										threads={props.summaryThreads}
										checks={props.summaryChecks}
										reviews={props.summaryReviews}
										files={props.summaryFiles}
										loading={props.summaryLoading}
									/>
								</box>
							</Show>
						</box>
					</Match>
					<Match when={props.view.view === "detail"}>
						<DetailView
							pr={props.detailPr}
							threads={props.detailThreads ?? []}
							comments={props.detailComments ?? []}
							loading={props.detailLoading ?? false}
							showResolved={props.showResolved ?? false}
							showBotComments={props.showBotComments ?? true}
							onExit={props.onExitDetail}
							onToggleResolved={props.onToggleResolved}
							onToggleBotComments={props.onToggleBotComments}
							onOpenInBrowser={() => {
								const pr = props.detailPr;
								if (pr) props.onOpenInBrowser?.(pr);
							}}
							onOpenInDevin={() => {
								const pr = props.detailPr;
								if (pr) props.onOpenInDevin?.(pr);
							}}
							onOpenUrl={props.onOpenUrl}
							onRefresh={props.onRefreshDetail}
						/>
					</Match>
				</Switch>
			</Show>
		</box>
	);
}
