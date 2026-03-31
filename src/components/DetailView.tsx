/**
 * Full-page PR detail view.
 *
 * Replaces the list+summary layout when a user drills into a PR.
 * Commit 10: shell (header + loading), description (markdown), CI checks.
 * Commit 11: review threads + conversation.
 * Commit 12: keybindings.
 */

import { Show, For, createMemo } from "solid-js";
import { MarkdownBody } from "../lib/markdown";
import { formatAge, formatSize, sortCheckRuns } from "../lib/format";
import type { PRDetail, CheckRun } from "../lib/types";
import type { FullReviewThread, IssueComment } from "../lib/types";

// ── Props ───────────────────────────────────────────────────────────────────

export interface DetailViewProps {
	pr: PRDetail | undefined;
	threads: FullReviewThread[];
	comments: IssueComment[];
	loading: boolean;
	showResolved: boolean;
	showBotComments: boolean;
}

// ── Check icon (shared with SummaryPanel — could extract later) ─────────

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

// ── Component ───────────────────────────────────────────────────────────────

export function DetailView(props: DetailViewProps) {
	const checks = createMemo(() => {
		const pr = props.pr;
		if (!pr || !("checks" in pr)) return [];
		return sortCheckRuns((pr as any).checks ?? []);
	});

	const passed = createMemo(
		() => checks().filter((c) => c.status === "completed" && c.conclusion === "success").length,
	);
	const failed = createMemo(
		() =>
			checks().filter(
				(c) =>
					c.status === "completed" &&
					(c.conclusion === "failure" ||
						c.conclusion === "timed_out" ||
						c.conclusion === "cancelled"),
			).length,
	);
	const pending = createMemo(() => checks().filter((c) => c.status !== "completed").length);

	return (
		<box flexDirection="column" width="100%" height="100%">
			{/* ── Header (pinned) ──────────────────────────────────── */}
			<Show
				when={props.pr}
				fallback={
					<Show when={props.loading}>
						<text>
							<span style={{ fg: "yellow" }}>Loading PR detail...</span>
						</text>
					</Show>
				}
			>
				{(pr) => (
					<>
						<box width="100%">
							<text>
								<span style={{ bold: true }}>
									#{pr().number} {pr().title}
								</span>
							</text>
						</box>
						<box width="100%" height={1}>
							<text truncate={true}>
								<span style={{ fg: "green" }}>{pr().author}</span>
								<span style={{ fg: "gray" }}>
									{" "}
									· created {formatAge(pr().createdAt)}
								</span>
								<span style={{ fg: "gray" }}>
									{" "}
									· updated {formatAge(pr().updatedAt)}
								</span>
								<span style={{ fg: "gray" }}>
									{" "}
									· {formatSize(pr().additions, pr().deletions)}
								</span>
								<Show when={pr().isDraft}>
									<span style={{ fg: "yellow" }}> draft</span>
								</Show>
							</text>
						</box>
						<box width="100%" height={1}>
							<text>
								<span style={{ fg: "gray" }}>
									────────────────────────────────────────
								</span>
							</text>
						</box>
					</>
				)}
			</Show>

			{/* ── Scrollable body ──────────────────────────────────── */}
			<Show when={props.pr}>
				{(pr) => (
					<scrollbox flexGrow={1} width="100%">
						{/* Description */}
						<box flexDirection="column" width="100%">
							<Show
								when={pr().body.trim()}
								fallback={
									<text>
										<span style={{ fg: "gray" }}>No description.</span>
									</text>
								}
							>
								<MarkdownBody source={pr().body} />
							</Show>

							{/* CI Checks */}
							<Show when={checks().length > 0}>
								<box width="100%" height={1}>
									<text>{""}</text>
								</box>
								<box width="100%">
									<text>
										<span style={{ bold: true, fg: "cyan" }}>## CI Checks</span>
										<span style={{ fg: "gray" }}>
											{" "}
											{passed()}/{checks().length} passed
										</span>
										<Show when={failed() > 0}>
											<span style={{ fg: "red" }}> · {failed()} failed</span>
										</Show>
										<Show when={pending() > 0}>
											<span style={{ fg: "yellow" }}>
												{" "}
												· {pending()} pending
											</span>
										</Show>
									</text>
								</box>
								<For each={checks()}>
									{(check) => {
										const ci = checkIcon(check);
										return (
											<box width="100%" height={1}>
												<text truncate={true}>
													<span>{"  "}</span>
													<span style={{ fg: ci.fg }}>{ci.icon}</span>
													<span> {check.name}</span>
												</text>
											</box>
										);
									}}
								</For>
							</Show>
						</box>
					</scrollbox>
				)}
			</Show>
		</box>
	);
}
