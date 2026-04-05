import { Show, For, createMemo } from "solid-js";
import type { PR, PRSummary, FileCategory } from "../lib/types";
import {
	formatAge,
	formatSize,
	formatReviewState,
	sortCheckRuns,
	checkIcon,
	reviewIcon,
	formatMergeable,
	blockerTierColor,
	checksSummary,
} from "../lib/format";
import { computeBlocker, tierLabel } from "../lib/blocker-engine";
import { theme } from "../lib/theme";

/** Max number of individual check lines to show before collapsing. */
const MAX_VISIBLE_CHECKS = 6;

interface SummaryPanelProps {
	summary: PRSummary | undefined;
	pr: PR | undefined;
	currentUser?: string;
}

export function SummaryPanel(props: SummaryPanelProps) {
	const pr = () => props.summary ?? props.pr;
	const summary = () => props.summary;

	/** Blocker result — null when summary not loaded or currentUser absent. */
	const blockerResult = createMemo(() => {
		const s = summary();
		const u = props.currentUser;
		if (!s || !u) return null;
		return computeBlocker(s, u, { checks: s.checks, reviews: s.reviews, threads: s.threads });
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
							<span style={{ fg: theme.muted }}>No PR selected</span>
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
						<span style={{ fg: theme.success }}>{pr()!.author}</span>
						<span> #{pr()!.number}</span>
						<Show when={pr()!.isDraft}>
							<span style={{ fg: theme.warning }}> draft</span>
						</Show>
					</text>
				</box>

				{/* Branches */}
				<Show when={pr()!.headRef}>
					<box width="100%">
						<text>
							<span style={{ fg: theme.accent }}>{pr()!.headRef}</span>
							<span style={{ fg: theme.muted }}> → </span>
							<span style={{ fg: theme.accent }}>{pr()!.baseRef}</span>
						</text>
					</box>
				</Show>

				{/* Dates */}
				<box height={1} width="100%">
					<text truncate={true}>
						<span style={{ fg: theme.muted }}>created </span>
						<span>{formatAge(pr()!.createdAt)}</span>
						<span style={{ fg: theme.muted }}> updated </span>
						<span>{formatAge(pr()!.updatedAt)}</span>
					</text>
				</box>

				{/* Merge status */}
				<box height={1} width="100%">
					<text>
						{(() => {
							const m = formatMergeable(pr()!.mergeable);
							return <span style={{ fg: m.fg }}>{m.text}</span>;
						})()}
					</text>
				</box>

				{/* Labels */}
				<Show when={pr()!.labels.length > 0}>
					<box height={1} width="100%">
						<text truncate={true}>
							<span style={{ fg: theme.muted }}>labels: </span>
							<span>{pr()!.labels.join(", ")}</span>
						</text>
					</box>
				</Show>

				{/* Assignees */}
				<Show when={pr()!.assignees.length > 0}>
					<box height={1} width="100%">
						<text truncate={true}>
							<span style={{ fg: theme.muted }}>assignees: </span>
							<span>{pr()!.assignees.join(", ")}</span>
						</text>
					</box>
				</Show>

				{/* --- Blocker (only when summary loaded and currentUser known) --- */}
				<Show when={blockerResult()}>
					{(b) => (
						<box height={1} width="100%">
							<text truncate={true}>
								<span style={{ fg: theme.muted }}>blocker: </span>
								<span style={{ fg: blockerTierColor(b().tier) }}>
									{tierLabel(b().tier)}
								</span>
								<Show when={b().blocker}>
									<span style={{ fg: theme.muted }}> ({b().blocker})</span>
								</Show>
							</text>
						</box>
					)}
				</Show>

				{/* Comments — shown right after blocker so unresolved threads are prominent */}
				<Show when={(summary()?.comments.unresolved ?? 0) > 0}>
					<box height={1} width="100%">
						<text truncate={true}>
							<span style={{ fg: theme.muted }}>comments: </span>
							<span>{summary()!.comments.unresolved} unresolved</span>
							<span style={{ fg: theme.muted }}>
								{" "}
								({summary()!.comments.unresolvedHuman} human,{" "}
								{summary()!.comments.unresolvedBot} bot)
							</span>
						</text>
					</box>
				</Show>

				<Show when={summary()}>
					{/* Size breakdown */}
					<Show when={sizeCategories().length > 0}>
						<box height={1} width="100%">
							<text>
								<span style={{ fg: theme.muted }}>size</span>
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
								<span style={{ fg: theme.muted }}>reviewers</span>
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
											<span style={{ fg: theme.muted }}>
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
								<span style={{ fg: theme.muted }}>requested</span>
							</text>
						</box>
						<For each={pr()!.requestedReviewers}>
							{(reviewer) => (
								<box height={1} width="100%">
									<text truncate={true}>
										<span>{"  "}</span>
										<span style={{ fg: theme.warning }}>○</span>
										<span> {reviewer} </span>
										<span style={{ fg: theme.muted }}>pending</span>
									</text>
								</box>
							)}
						</For>
					</Show>

					{/* CI Checks */}
					<Show when={summary()!.checks.length > 0}>
						{(() => {
							const sorted = createMemo(() => sortCheckRuns(summary()!.checks));
							const counts = createMemo(() => checksSummary(sorted()));
							const visible = createMemo(() => sorted().slice(0, MAX_VISIBLE_CHECKS));
							const overflow = createMemo(() =>
								Math.max(0, counts().total - MAX_VISIBLE_CHECKS),
							);

							return (
								<>
									<box height={1} width="100%">
										<text>
											<span style={{ fg: theme.muted }}>checks </span>
											<Show when={counts().failed > 0}>
												<span style={{ fg: theme.error }}>
													{counts().failed} failed{" "}
												</span>
											</Show>
											<Show when={counts().pending > 0}>
												<span style={{ fg: theme.warning }}>
													{counts().pending} pending{" "}
												</span>
											</Show>
											<span
												style={{
													fg:
														counts().passed === counts().total
															? theme.success
															: theme.muted,
												}}
											>
												{counts().passed}/{counts().total} passed
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
												<span style={{ fg: theme.muted }}>
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
							<span style={{ fg: theme.muted }}>Loading details...</span>
						</text>
					</box>
				</Show>
			</Show>
		</box>
	);
}
