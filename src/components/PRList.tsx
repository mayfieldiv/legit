import { For, Show } from "solid-js";
import type { PR } from "../lib/types";
import type { MouseEvent } from "@opentui/core";
import { formatAge, formatSize, formatReviewDecision, formatRepoShort } from "../lib/format";
import { computeBlocker } from "../lib/blocker-engine";
import type { Tier } from "../lib/blocker-engine";

interface PRListProps {
	prs: PR[];
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
	return (
		<box flexDirection="column" width="100%">
			<Show when={props.prs.length > 0} fallback={<text>No open pull requests</text>}>
				<For each={props.prs}>
					{(pr, index) => (
						<PRRow
							id={`pr-row-${index()}`}
							pr={pr}
							selected={index() === props.selectedIndex}
							showRepo={props.showRepo}
							currentUser={props.currentUser}
							onMouseDown={(e: MouseEvent) => {
								e.preventDefault();
								props.onSelect?.(index());
							}}
						/>
					)}
				</For>
			</Show>
		</box>
	);
}
