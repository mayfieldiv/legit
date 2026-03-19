import { For, Show } from "solid-js";
import type { PR } from "../lib/types";
import { formatAge, formatSize, formatReviewDecision } from "../lib/format";

interface PRListProps {
	prs: PR[];
	selectedIndex: number;
}

// Column widths — fixed columns; title gets remaining space via flexGrow
const COL = {
	pr: 7,
	author: 14,
	size: 18,
	age: 6,
	review: 18,
} as const;

function Cell(props: {
	width?: number;
	flexGrow?: number;
	paddingRight?: number;
	children: any;
}) {
	return (
		<box
			width={props.width}
			flexGrow={props.flexGrow}
			paddingRight={props.paddingRight}
			overflow="hidden"
		>
			<text wrapMode="none" truncate={true}>{props.children}</text>
		</box>
	);
}

function PRRow(props: { pr: PR; selected: boolean; id: string }) {
	const pr = props.pr;
	const fg = () => (props.selected ? "white" : undefined);
	return (
		<box
			id={props.id}
			flexDirection="row"
			width="100%"
			height={1}
			background={props.selected ? "blue" : undefined}
		>
			<Cell width={COL.pr} paddingRight={1}>
				<span color={props.selected ? "white" : "cyan"}>#{pr.number}</span>
			</Cell>
			<Cell flexGrow={1} paddingRight={1}>
				<span color={fg()}>{pr.title}</span>
				<Show when={pr.isDraft}>
					<span color="yellow"> draft</span>
				</Show>
			</Cell>
			<Cell width={COL.author} paddingRight={1}>
				<span color={props.selected ? "white" : "green"}>{pr.author}</span>
			</Cell>
			<Cell width={COL.size} paddingRight={1}>
				<span color={fg()}>{formatSize(pr.additions, pr.deletions)}</span>
			</Cell>
			<Cell width={COL.age} paddingRight={1}>
				<span color={fg()}>{formatAge(pr.createdAt)}</span>
			</Cell>
			<Cell width={COL.review}>
				<span color={fg()}>{formatReviewDecision(pr.reviewDecision)}</span>
			</Cell>
		</box>
	);
}

function HeaderRow() {
	return (
		<box flexDirection="row" width="100%" height={1}>
			<Cell width={COL.pr} paddingRight={1}><span bold>PR</span></Cell>
			<Cell flexGrow={1} paddingRight={1}><span bold>Title</span></Cell>
			<Cell width={COL.author} paddingRight={1}><span bold>Author</span></Cell>
			<Cell width={COL.size} paddingRight={1}><span bold>Size</span></Cell>
			<Cell width={COL.age} paddingRight={1}><span bold>Age</span></Cell>
			<Cell width={COL.review}><span bold>Review</span></Cell>
		</box>
	);
}

export function PRListHeader() {
	return <HeaderRow />;
}

export function PRList(props: PRListProps) {
	return (
		<box flexDirection="column" width="100%">
			<Show
				when={props.prs.length > 0}
				fallback={
					<text>No open pull requests</text>
				}
			>
				<For each={props.prs}>
					{(pr, index) => (
						<PRRow
							id={`pr-row-${index()}`}
							pr={pr}
							selected={index() === props.selectedIndex}
						/>
					)}
				</For>
			</Show>
		</box>
	);
}
