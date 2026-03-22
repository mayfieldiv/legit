import { For, Show } from "solid-js";
import type { PR } from "../lib/types";
import type { MouseEvent } from "@opentui/core";
import { formatAge, formatSize, formatReviewDecision } from "../lib/format";

interface PRListProps {
	prs: PR[];
	selectedIndex: number;
	onSelect?: (index: number) => void;
}

// Column widths — fixed columns; title gets remaining space via flexGrow
const COL = {
	pr: 7,
	author: 14,
	size: 18,
	age: 6,
	review: 18,
} as const;

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
	id: string;
	onMouseDown?: (e: MouseEvent) => void;
}) {
	const fg = () => (props.selected ? "white" : undefined);
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
			<Cell flexGrow={1} paddingRight={3}>
				<span style={{ fg: fg() }}>{props.pr.title}</span>
				<Show when={props.pr.isDraft}>
					<span style={{ fg: "yellow" }}> draft</span>
				</Show>
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
			<Cell width={COL.review}>
				<span style={{ fg: fg() }}>{formatReviewDecision(props.pr.reviewDecision)}</span>
			</Cell>
		</box>
	);
}

export function PRListHeader() {
	return (
		<box flexDirection="row" width="100%" height={1}>
			<Cell width={COL.pr} paddingRight={1}>
				<b>PR</b>
			</Cell>
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
			<Cell width={COL.review}>
				<b>Review</b>
			</Cell>
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
