import { Show, For, createMemo } from "solid-js";
import type { PR, PRSummary, CheckRun, FileCategory } from "../lib/types";
import { formatAge, formatSize, formatReviewState, sortCheckRuns } from "../lib/format";
import { computeBlocker, tierLabel } from "../lib/blocker-engine";

/** Max number of individual check lines to show before collapsing. */
const MAX_VISIBLE_CHECKS = 6;

interface SummaryPanelProps {
	summary: PRSummary | undefined;
	pr: PR | undefined;
	currentUser?: string;
}

function checkIcon(check: CheckRun): { icon: string; fg: string } {
	if (check.status !== "completed") {
		return { icon: "●", fg: "yellow" };
	}
	switch (check.conclusion) {
		case "success":
			return { icon: "✓", fg: "green" };
		case "failure":
		case "timed_out":
		case "cancelled":
			return { icon: "✗", fg: "red" };
		case "action_required":
			return { icon: "✗", fg: "yellow" };
		case "neutral":
			return { icon: "–", fg: "white" };
		case "skipped":
			return { icon: "⊘", fg: "gray" };
		case "stale":
			return { icon: "⟳", fg: "yellow" };
		default:
			return { icon: "?", fg: "white" };
	}
}

function reviewIcon(state: string): { icon: string; fg: string } {
	switch (state) {
		case "APPROVED":
			return { icon: "✓", fg: "green" };
		case "CHANGES_REQUESTED":
			return { icon: "✗", fg: "red" };
		case "COMMENTED":
			return { icon: "●", fg: "cyan" };
		case "DISMISSED":
			return { icon: "–", fg: "gray" };
		default:
			return { icon: "?", fg: "white" };
	}
}

export function SummaryPanel(props: SummaryPanelProps) {
	const pr = () => props.summary ?? props.pr;
	const summary = () => props.summary;

	/** Blocker result — null when summary not loaded or currentUser absent. */
	const blockerResult = createMemo(() => {
		const s = summary();
		const u = props.currentUser;
		if (!s || !u) return null;
		return computeBlocker(s, u, { checks: s.checks, reviews: s.reviews });
	});

	const sizeCategories = (): FileCategory[] => {
		const s = summary();
		if (!s || s.files.breakdown.total.files === 0) return [];
		return (["code", "test", "generated", "docs", "config"] as const).filter(
			(cat) => s.files.breakdown[cat].files > 0,
		);
	};

	return (
		<box flexDirection="column" width="100%" height="100%" paddingLeft={1}>
			<Show
				when={pr()}
				fallback={
					<box height={1}>
						<text>
							<span style={{ fg: "gray" }}>No PR selected</span>
						</text>
					</box>
				}
			>
				{/* Title — wraps naturally */}
				<box width="100%">
					<text>
						<b>{pr()!.title}</b>
					</text>
				</box>

				{/* Meta */}
				<box height={1} width="100%">
					<text truncate={true}>
						<span style={{ fg: "green" }}>{pr()!.author}</span>
						<span> #{pr()!.number}</span>
						<Show when={pr()!.isDraft}>
							<span style={{ fg: "yellow" }}> draft</span>
						</Show>
					</text>
				</box>

				{/* Dates */}
				<box height={1} width="100%">
					<text truncate={true}>
						<span style={{ fg: "gray" }}>created </span>
						<span>{formatAge(pr()!.createdAt)}</span>
						<span style={{ fg: "gray" }}> updated </span>
						<span>{formatAge(pr()!.updatedAt)}</span>
					</text>
				</box>

				{/* Merge status */}
				<box height={1} width="100%">
					<text>
						<Show when={pr()!.mergeable === "CONFLICTING"}>
							<span style={{ fg: "red" }}>⚠ conflict</span>
						</Show>
						<Show when={pr()!.mergeable === "MERGEABLE"}>
							<span style={{ fg: "green" }}>✓ mergeable</span>
						</Show>
						<Show when={pr()!.mergeable === "UNKNOWN"}>
							<span style={{ fg: "gray" }}>? merge unknown</span>
						</Show>
					</text>
				</box>

				{/* Labels */}
				<Show when={pr()!.labels.length > 0}>
					<box height={1} width="100%">
						<text truncate={true}>
							<span style={{ fg: "gray" }}>labels: </span>
							<span>{pr()!.labels.join(", ")}</span>
						</text>
					</box>
				</Show>

				{/* Assignees */}
				<Show when={pr()!.assignees.length > 0}>
					<box height={1} width="100%">
						<text truncate={true}>
							<span style={{ fg: "gray" }}>assignees: </span>
							<span>{pr()!.assignees.join(", ")}</span>
						</text>
					</box>
				</Show>

				{/* --- Blocker (only when summary loaded and currentUser known) --- */}
				<Show when={blockerResult()}>
					{(b) => (
						<box height={1} width="100%">
							<text truncate={true}>
								<span style={{ fg: "gray" }}>blocker: </span>
								<span
									style={{
										fg:
											b().tier === "me-blocking"
												? "magenta"
												: b().tier === "waiting-on-author"
													? "yellow"
													: "gray",
									}}
								>
									{tierLabel(b().tier)}
								</span>
								<Show when={b().blocker}>
									<span style={{ fg: "gray" }}> ({b().blocker})</span>
								</Show>
							</text>
						</box>
					)}
				</Show>

				<Show when={summary()}>
					{/* Size breakdown */}
					<Show when={sizeCategories().length > 0}>
						<box height={1} width="100%">
							<text>
								<span style={{ fg: "gray" }}>size</span>
							</text>
						</box>
						<For each={sizeCategories()}>
							{(cat) => (
								<box height={1} width="100%">
									<text truncate={true}>
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
								</box>
							)}
						</For>
					</Show>

					{/* Reviewers */}
					<Show when={summary()!.reviews.length > 0}>
						<box height={1} width="100%">
							<text>
								<span style={{ fg: "gray" }}>reviewers</span>
							</text>
						</box>
						<For each={summary()!.reviews}>
							{(review) => {
								const ri = reviewIcon(review.state);
								return (
									<box height={1} width="100%">
										<text truncate={true}>
											<span>{"  "}</span>
											<span style={{ fg: ri.fg }}>{ri.icon}</span>
											<span> {review.user} </span>
											<span style={{ fg: "gray" }}>
												{formatReviewState(review.state)}
											</span>
										</text>
									</box>
								);
							}}
						</For>
					</Show>

					{/* Requested reviewers (not yet reviewed) */}
					<Show when={pr()!.requestedReviewers.length > 0}>
						<box height={1} width="100%">
							<text>
								<span style={{ fg: "gray" }}>requested</span>
							</text>
						</box>
						<For each={pr()!.requestedReviewers}>
							{(reviewer) => (
								<box height={1} width="100%">
									<text truncate={true}>
										<span>{"  "}</span>
										<span style={{ fg: "yellow" }}>○</span>
										<span> {reviewer} </span>
										<span style={{ fg: "gray" }}>pending</span>
									</text>
								</box>
							)}
						</For>
					</Show>

					{/* Comments */}
					<Show when={summary()!.comments.total > 0}>
						<box height={1} width="100%">
							<text truncate={true}>
								<span style={{ fg: "gray" }}>comments: </span>
								<span>{summary()!.comments.unresolved} unresolved</span>
								<span style={{ fg: "gray" }}>
									{" "}
									({summary()!.comments.unresolvedHuman} human,{" "}
									{summary()!.comments.unresolvedBot} bot)
								</span>
							</text>
						</box>
					</Show>

					{/* CI Checks */}
					<Show when={summary()!.checks.length > 0}>
						{(() => {
							const sorted = createMemo(() => sortCheckRuns(summary()!.checks));
							const total = createMemo(() => sorted().length);
							const passed = createMemo(
								() =>
									sorted().filter(
										(c) =>
											c.status === "completed" && c.conclusion === "success",
									).length,
							);
							const failed = createMemo(
								() =>
									sorted().filter(
										(c) =>
											c.status === "completed" &&
											(c.conclusion === "failure" ||
												c.conclusion === "timed_out" ||
												c.conclusion === "cancelled"),
									).length,
							);
							const pending = createMemo(
								() => sorted().filter((c) => c.status !== "completed").length,
							);
							const visible = createMemo(() => sorted().slice(0, MAX_VISIBLE_CHECKS));
							const overflow = createMemo(() =>
								Math.max(0, total() - MAX_VISIBLE_CHECKS),
							);

							return (
								<>
									<box height={1} width="100%">
										<text>
											<span style={{ fg: "gray" }}>checks </span>
											<Show when={failed() > 0}>
												<span style={{ fg: "red" }}>
													{failed()} failed{" "}
												</span>
											</Show>
											<Show when={pending() > 0}>
												<span style={{ fg: "yellow" }}>
													{pending()} pending{" "}
												</span>
											</Show>
											<span
												style={{
													fg: passed() === total() ? "green" : "gray",
												}}
											>
												{passed()}/{total()} passed
											</span>
										</text>
									</box>
									<For each={visible()}>
										{(check) => {
											const ci = checkIcon(check);
											return (
												<box height={1} width="100%">
													<text truncate={true}>
														<span>{"  "}</span>
														<span style={{ fg: ci.fg }}>{ci.icon}</span>
														<span> {check.name}</span>
													</text>
												</box>
											);
										}}
									</For>
									<Show when={overflow() > 0}>
										<box height={1} width="100%">
											<text>
												<span style={{ fg: "gray" }}>
													{" "}
													+{overflow()} more
												</span>
											</text>
										</box>
									</Show>
								</>
							);
						})()}
					</Show>
				</Show>

				{/* Loading indicator when summary not yet loaded */}
				<Show when={!summary() && pr()}>
					<box height={1} width="100%">
						<text>
							<span style={{ fg: "gray" }}>Loading details...</span>
						</text>
					</box>
				</Show>
			</Show>
		</box>
	);
}
