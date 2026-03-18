import { For, Show } from "solid-js";
import type { PR } from "../lib/types";
import { formatAge, formatSize, formatReviewDecision } from "../lib/format";

interface PRListProps {
	prs: PR[];
	selectedIndex: number;
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
				{/* Header */}
				<box flexDirection="row" width="100%">
					<text width={8}>
						<span bold>PR</span>
					</text>
					<text width={45}>
						<span bold>Title</span>
					</text>
					<text width={14}>
						<span bold>Author</span>
					</text>
					<text width={14}>
						<span bold>Size</span>
					</text>
					<text width={8}>
						<span bold>Age</span>
					</text>
					<text width={18}>
						<span bold>Review</span>
					</text>
				</box>

				{/* Rows */}
				<For each={props.prs}>
					{(pr, index) => {
						const isSelected = () => index() === props.selectedIndex;
						const draft = () => pr.isDraft ? " draft" : "";
						return (
							<box
								flexDirection="row"
								width="100%"
								background={isSelected() ? "blue" : undefined}
							>
								<text width={8}>
									<span color={isSelected() ? "white" : "cyan"}>
										#{pr.number}
									</span>
								</text>
								<text width={45}>
									<span color={isSelected() ? "white" : undefined}>
										{pr.title}
									</span>
									<Show when={pr.isDraft}>
										<span color="yellow"> draft</span>
									</Show>
								</text>
								<text width={14}>
									<span color={isSelected() ? "white" : "green"}>
										{pr.author}
									</span>
								</text>
								<text width={14}>
									<span>{formatSize(pr.additions, pr.deletions)}</span>
								</text>
								<text width={8}>
									<span>{formatAge(pr.createdAt)}</span>
								</text>
								<text width={18}>
									<span>{formatReviewDecision(pr.reviewDecision)}</span>
								</text>
							</box>
						);
					}}
				</For>
			</Show>
		</box>
	);
}
