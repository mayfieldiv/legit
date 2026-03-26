import { For, Show, createMemo } from "solid-js";
import type { PR } from "../lib/types";
import type { MouseEvent } from "@opentui/core";
import { formatAge, formatSize, formatReviewDecision, formatRepoShort } from "../lib/format";
import { computeBlocker } from "../lib/blocker-engine";
import type { Tier } from "../lib/blocker-engine";

// ── Flat item type (group headers + PR rows) ─────────────────────────────────

/** A single display item in the PR list: either a group header or a PR row. */
export type FlatItem = { kind: "header"; label: string } | { kind: "pr"; pr: PR; prIndex: number };

/**
 * Build a flat display list from groups (including headers).
 * Groups with empty labels (i.e. "none" grouping) produce no header row.
 */
export function buildFlatItems(groups: Array<{ label: string; prs: PR[] }>): FlatItem[] {
	const items: FlatItem[] = [];
	let prIndex = 0;
	for (const group of groups) {
		if (group.label) {
			items.push({ kind: "header", label: group.label });
		}
		for (const pr of group.prs) {
			items.push({ kind: "pr", pr, prIndex: prIndex++ });
		}
	}
	return items;
}

/**
 * Map a PR selection index to its row position in a flat items list.
 * Used to compute the scroll target when groups are present.
 */
export function prIndexToDisplayRow(items: FlatItem[], prIndex: number): number {
	let prCount = 0;
	for (let i = 0; i < items.length; i++) {
		const item = items[i]!;
		if (item.kind === "pr") {
			if (prCount === prIndex) return i;
			prCount++;
		}
	}
	return prIndex; // fallback: flat list
}

interface PRListProps {
	/** Flat list of PRs (backward compat — used when `items` is not provided). */
	prs?: PR[];
	/** Pre-built flat items list (with optional group headers). Overrides `prs`. */
	items?: FlatItem[];
	selectedIndex: number;
	showRepo?: boolean;
	currentUser?: string;
	onSelect?: (index: number) => void;
}

// Column widths — fixed columns; title gets remaining space via flexGrow
const COL = {
	pr: 7,
	repo: 14,
	author: 14,
	size: 14,
	age: 6,
	review: 18,
	blocker: 14,
} as const;

/** Compute the text content of the review column. */
function reviewCellText(pr: PR): string {
	const conflict = pr.mergeable === "CONFLICTING" ? "! " : "";
	const draft = pr.isDraft ? "draft " : "";
	const decision = formatReviewDecision(pr.reviewDecision);
	return conflict + draft + decision;
}

/** Compute the fg color for the review column (priority: conflict > draft > default). */
function reviewCellFg(pr: PR, selected: boolean): string | undefined {
	if (selected) return "white";
	if (pr.mergeable === "CONFLICTING") return "red";
	if (pr.isDraft) return "yellow";
	return undefined;
}

function blockerDisplay(
	tier: Tier,
	blocker: string,
	currentUser: string,
): { text: string; fg: string } | null {
	const isMe = blocker === currentUser;
	switch (tier) {
		case "me-blocking":
			return { text: "you", fg: "magenta" };
		case "waiting-on-author":
			return { text: isMe ? "you" : blocker || "author", fg: isMe ? "magenta" : "yellow" };
		case "waiting-on-other":
			return { text: blocker, fg: "gray" };
		case "needs-review":
			return null;
	}
}

function GroupHeaderRow(props: { label: string }) {
	return (
		<box height={1} width="100%">
			<text wrapMode="none" truncate={true}>
				<span style={{ fg: "cyan" }}>── {props.label} </span>
			</text>
		</box>
	);
}

function Cell(props: { width?: number; flexGrow?: number; paddingRight?: number; children: any }) {
	return (
		<box
			width={props.width}
			flexGrow={props.flexGrow}
			paddingRight={props.paddingRight}
			overflow="hidden"
		>
			<text wrapMode="none" truncate={true}>
				{props.children}
			</text>
		</box>
	);
}

function PRRow(props: {
	pr: PR;
	selected: boolean;
	showRepo?: boolean;
	currentUser?: string;
	id: string;
	onMouseDown?: (e: MouseEvent) => void;
}) {
	const fg = () => (props.selected ? "white" : undefined);
	const blockerCell = () => {
		if (!props.currentUser) return null;
		const b = computeBlocker(props.pr, props.currentUser);
		return blockerDisplay(b.tier, b.blocker, props.currentUser);
	};
	return (
		<box
			id={props.id}
			flexDirection="row"
			width="100%"
			height={1}
			backgroundColor={props.selected ? "blue" : undefined}
			onMouseDown={props.onMouseDown}
		>
			<Cell width={COL.pr} paddingRight={1}>
				<span style={{ fg: props.selected ? "white" : "cyan" }}>#{props.pr.number}</span>
			</Cell>
			<Show when={props.showRepo}>
				<Cell width={COL.repo} paddingRight={1}>
					<span style={{ fg: props.selected ? "white" : "magenta" }}>
						{formatRepoShort(props.pr.repoSlug)}
					</span>
				</Cell>
			</Show>
			<Cell flexGrow={1} paddingRight={props.showRepo ? 2 : 3}>
				<span style={{ fg: fg() }}>{props.pr.title}</span>
			</Cell>
			<Cell width={COL.author} paddingRight={1}>
				<span style={{ fg: props.selected ? "white" : "green" }}>{props.pr.author}</span>
			</Cell>
			<Cell width={COL.size} paddingRight={1}>
				<span style={{ fg: fg() }}>
					{formatSize(props.pr.additions, props.pr.deletions)}
				</span>
			</Cell>
			<Cell width={COL.age} paddingRight={1}>
				<span style={{ fg: fg() }}>{formatAge(props.pr.createdAt)}</span>
			</Cell>
			<Cell width={COL.review} paddingRight={props.currentUser ? 1 : 0}>
				<span style={{ fg: reviewCellFg(props.pr, props.selected) }}>
					{reviewCellText(props.pr)}
				</span>
			</Cell>
			<Show when={props.currentUser}>
				<Cell width={COL.blocker}>
					<Show when={blockerCell()}>
						{(display) => (
							<span style={{ fg: props.selected ? "white" : display().fg }}>
								{display().text}
							</span>
						)}
					</Show>
				</Cell>
			</Show>
		</box>
	);
}

export function PRListHeader(props: { showRepo?: boolean; currentUser?: string }) {
	return (
		<box flexDirection="row" width="100%" height={1}>
			<Cell width={COL.pr} paddingRight={1}>
				<b>PR</b>
			</Cell>
			<Show when={props.showRepo}>
				<Cell width={COL.repo} paddingRight={1}>
					<b>Repo</b>
				</Cell>
			</Show>
			<Cell flexGrow={1} paddingRight={3}>
				<b>Title</b>
			</Cell>
			<Cell width={COL.author} paddingRight={1}>
				<b>Author</b>
			</Cell>
			<Cell width={COL.size} paddingRight={1}>
				<b>Size</b>
			</Cell>
			<Cell width={COL.age} paddingRight={1}>
				<b>Age</b>
			</Cell>
			<Cell width={COL.review} paddingRight={props.currentUser ? 1 : 0}>
				<b>Review</b>
			</Cell>
			<Show when={props.currentUser}>
				<Cell width={COL.blocker}>
					<b>Blocker</b>
				</Cell>
			</Show>
		</box>
	);
}

export function PRList(props: PRListProps) {
	// Resolve flat items — prefer `items` prop; fall back to converting `prs`
	const resolvedItems = createMemo((): FlatItem[] => {
		if (props.items) return props.items;
		return (props.prs ?? []).map((pr, i) => ({ kind: "pr", pr, prIndex: i }));
	});

	const hasPRs = () => resolvedItems().some((item) => item.kind === "pr");

	return (
		<box flexDirection="column" width="100%">
			<Show when={hasPRs()} fallback={<text>No open pull requests</text>}>
				<For each={resolvedItems()}>
					{(item) => {
						if (item.kind === "header") {
							return <GroupHeaderRow label={item.label} />;
						}
						return (
							<PRRow
								id={`pr-row-${item.prIndex}`}
								pr={item.pr}
								selected={item.prIndex === props.selectedIndex}
								showRepo={props.showRepo}
								currentUser={props.currentUser}
								onMouseDown={(e: MouseEvent) => {
									e.preventDefault();
									props.onSelect?.(item.prIndex);
								}}
							/>
						);
					}}
				</For>
			</Show>
		</box>
	);
}
