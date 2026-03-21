import { Show, For } from "solid-js";
import type { PR, PRSummary, CheckRun } from "../lib/types";
import { formatAge, formatSize, formatReviewState, sortCheckRuns } from "../lib/format";

interface SummaryPanelProps {
	summary: PRSummary | undefined;
	pr: PR | undefined;
}

function CheckIcon(props: { check: CheckRun }) {
	if (props.check.status !== "completed") {
		return <span style={{ fg: "yellow" }}>●</span>;
	}
	switch (props.check.conclusion) {
		case "success":
			return <span style={{ fg: "green" }}>✓</span>;
		case "failure":
		case "timed_out":
		case "cancelled":
			return <span style={{ fg: "red" }}>✗</span>;
		case "action_required":
			return <span style={{ fg: "yellow" }}>✗</span>;
		case "neutral":
			return <span>–</span>;
		default:
			return <span>?</span>;
	}
}

function ReviewIcon(props: { state: string }) {
	switch (props.state) {
		case "APPROVED":
			return <span style={{ fg: "green" }}>✓</span>;
		case "CHANGES_REQUESTED":
			return <span style={{ fg: "red" }}>✗</span>;
		case "COMMENTED":
			return <span style={{ fg: "cyan" }}>●</span>;
		case "DISMISSED":
			return <span style={{ fg: "gray" }}>–</span>;
		default:
			return <span>?</span>;
	}
}

function Section(props: { label: string; children: any }) {
	return (
		<box flexDirection="column" width="100%">
			<text>
				<span style={{ fg: "gray" }}>{props.label}</span>
			</text>
			{props.children}
		</box>
	);
}

export function SummaryPanel(props: SummaryPanelProps) {
	const pr = () => props.summary ?? props.pr;
	const summary = () => props.summary;

	return (
		<box flexDirection="column" width="100%" height="100%" paddingLeft={1} paddingRight={1}>
			<Show
				when={pr()}
				fallback={
					<text>
						<span style={{ fg: "gray" }}>No PR selected</span>
					</text>
				}
			>
				{/* Title */}
				<text wrapMode="word">
					<b>{pr()!.title}</b>
				</text>

				{/* Meta */}
				<text>
					<span style={{ fg: "green" }}>{pr()!.author}</span>
					<span> #{pr()!.number}</span>
					<Show when={pr()!.isDraft}>
						<span style={{ fg: "yellow" }}> draft</span>
					</Show>
				</text>

				{/* Dates */}
				<text>
					<span style={{ fg: "gray" }}>created </span>
					<span>{formatAge(pr()!.createdAt)}</span>
					<span style={{ fg: "gray" }}> updated </span>
					<span>{formatAge(pr()!.updatedAt)}</span>
				</text>

				{/* Merge status */}
				<Show when={pr()!.mergeable === "CONFLICTING"}>
					<text>
						<span style={{ fg: "red" }}>⚠ conflict</span>
					</text>
				</Show>
				<Show when={pr()!.mergeable === "MERGEABLE"}>
					<text>
						<span style={{ fg: "green" }}>✓ mergeable</span>
					</text>
				</Show>
				<Show when={pr()!.mergeable === "UNKNOWN"}>
					<text>
						<span style={{ fg: "gray" }}>? merge unknown</span>
					</text>
				</Show>

				{/* Labels */}
				<Show when={pr()!.labels.length > 0}>
					<text>
						<span style={{ fg: "gray" }}>labels: </span>
						<span>{pr()!.labels.join(", ")}</span>
					</text>
				</Show>

				{/* Assignees */}
				<Show when={pr()!.assignees.length > 0}>
					<text>
						<span style={{ fg: "gray" }}>assignees: </span>
						<span>{pr()!.assignees.join(", ")}</span>
					</text>
				</Show>

				{/* --- Extended fields (only when summary loaded) --- */}
				<Show when={summary()}>
					{/* Size breakdown */}
					<Show when={summary()!.files.breakdown.total.files > 0}>
						<Section label="size">
							<For
								each={(
									["code", "test", "generated", "docs", "config"] as const
								).filter((cat) => summary()!.files.breakdown[cat].files > 0)}
							>
								{(cat) => (
									<text>
										<span>
											{"  "}
											{cat}:{" "}
										</span>
										<span>
											{formatSize(
												summary()!.files.breakdown[cat].additions,
												summary()!.files.breakdown[cat].deletions,
											)}
										</span>
									</text>
								)}
							</For>
						</Section>
					</Show>

					{/* Reviewers */}
					<Show when={summary()!.reviews.length > 0}>
						<Section label="reviewers">
							<For each={summary()!.reviews}>
								{(review) => (
									<text>
										<span>{"  "}</span>
										<ReviewIcon state={review.state} />
										<span> {review.user} </span>
										<span style={{ fg: "gray" }}>
											{formatReviewState(review.state)}
										</span>
									</text>
								)}
							</For>
						</Section>
					</Show>

					{/* Requested reviewers (not yet reviewed) */}
					<Show when={pr()!.requestedReviewers.length > 0}>
						<Section label="requested">
							<For each={pr()!.requestedReviewers}>
								{(reviewer) => (
									<text>
										<span>{"  "}</span>
										<span style={{ fg: "yellow" }}>○</span>
										<span> {reviewer} </span>
										<span style={{ fg: "gray" }}>pending</span>
									</text>
								)}
							</For>
						</Section>
					</Show>

					{/* Comments */}
					<Show when={summary()!.comments.total > 0}>
						<text>
							<span style={{ fg: "gray" }}>comments: </span>
							<span>{summary()!.comments.unresolved} unresolved</span>
							<span style={{ fg: "gray" }}>
								{" "}
								({summary()!.comments.human} human, {summary()!.comments.bot} bot)
							</span>
						</text>
					</Show>

					{/* CI Checks */}
					<Show when={summary()!.checks.length > 0}>
						<Section label="checks">
							<scrollbox flexGrow={1} width="100%">
								<box flexDirection="column" width="100%">
									<For each={sortCheckRuns(summary()!.checks)}>
										{(check) => (
											<text>
												<span>{"  "}</span>
												<CheckIcon check={check} />
												<span> {check.name}</span>
											</text>
										)}
									</For>
								</box>
							</scrollbox>
						</Section>
					</Show>
				</Show>

				{/* Loading indicator when summary not yet loaded */}
				<Show when={!summary() && pr()}>
					<text>
						<span style={{ fg: "gray" }}>Loading details...</span>
					</text>
				</Show>
			</Show>
		</box>
	);
}
