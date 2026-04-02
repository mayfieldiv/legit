/**
 * Full-page PR detail view.
 *
 * Replaces the list+summary layout when a user drills into a PR.
 * Commit 10: shell (header + loading), description (markdown), CI checks.
 * Commit 11: review threads + conversation (filtered by showResolved/showBotComments).
 * Commit 12: keybindings.
 *
 * Thread display: unresolved threads shown by default; resolved hidden
 * unless showResolved is true. Bot-only threads hidden when showBotComments
 * is false. Threads grouped by file path. Issue comments shown chronologically.
 */

import { Show, For, createMemo } from "solid-js";
import { useKeyboard } from "@opentui/solid";
import { MarkdownBody } from "../lib/markdown";
import { formatAge, formatSize, sortCheckRuns } from "../lib/format";
import type {
	PRDetail,
	CheckRun,
	FullReviewThread,
	IssueComment,
	ReviewComment,
} from "../lib/types";

// ── Props ───────────────────────────────────────────────────────────────────

export interface DetailViewProps {
	pr: PRDetail | undefined;
	threads: FullReviewThread[];
	comments: IssueComment[];
	loading: boolean;
	showResolved: boolean;
	showBotComments: boolean;
	onExit?: () => void;
	onToggleResolved?: () => void;
	onToggleBotComments?: () => void;
	onOpenInBrowser?: () => void;
	onRefresh?: () => void;
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

// ── Thread / comment sub-components ─────────────────────────────────────────

function ThreadCard(props: { thread: FullReviewThread; showBotComments: boolean }) {
	const visibleComments = createMemo(() => {
		if (props.showBotComments) return props.thread.comments;
		return props.thread.comments.filter((c) => !c.isBot);
	});

	const location = () => {
		const t = props.thread;
		return t.line != null ? `${t.path}:${t.line}` : t.path;
	};

	return (
		<Show when={visibleComments().length > 0}>
			<box flexDirection="column" width="100%" paddingLeft={2}>
				<box width="100%" height={1}>
					<text truncate={true}>
						<span style={{ fg: "cyan" }}>{location()}</span>
						<span style={{ fg: props.thread.isResolved ? "green" : "yellow" }}>
							{props.thread.isResolved ? " ✓ resolved" : " ● unresolved"}
						</span>
					</text>
				</box>
				<For each={visibleComments()}>{(comment) => <CommentRow comment={comment} />}</For>
			</box>
		</Show>
	);
}

function CommentRow(props: { comment: ReviewComment | IssueComment }) {
	return (
		<box flexDirection="column" width="100%" paddingLeft={2}>
			<box width="100%" height={1}>
				<text truncate={true}>
					<span style={{ fg: props.comment.isBot ? "gray" : "green" }}>
						{props.comment.author}
					</span>
					<Show when={props.comment.isBot}>
						<span style={{ fg: "gray" }}> [bot]</span>
					</Show>
					<span style={{ fg: "gray" }}> · {formatAge(props.comment.createdAt)}</span>
				</text>
			</box>
			<box width="100%">
				<MarkdownBody source={props.comment.body} />
			</box>
		</box>
	);
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

	// ── Keyboard ───────────────────────────────────────────────────────────
	useKeyboard((event) => {
		const name = event.name;
		if (name === "escape") {
			props.onExit?.();
		} else if (name === "t") {
			props.onToggleResolved?.();
		} else if (name === "b") {
			props.onToggleBotComments?.();
		} else if (name === "o") {
			props.onOpenInBrowser?.();
		} else if (name === "r" && !event.shift) {
			props.onRefresh?.();
		}
	});

	// ── Filtered threads ─────────────────────────────────────────────────
	const visibleThreads = createMemo(() => {
		let threads = props.threads;
		if (!props.showResolved) {
			threads = threads.filter((t) => !t.isResolved);
		}
		if (!props.showBotComments) {
			threads = threads.filter((t) => t.comments.some((c) => !c.isBot));
		}
		return threads;
	});

	const hiddenThreadCount = createMemo(() => props.threads.length - visibleThreads().length);

	// ── Filtered issue comments ──────────────────────────────────────────
	const visibleComments = createMemo(() => {
		if (props.showBotComments) return props.comments;
		return props.comments.filter((c) => !c.isBot);
	});

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
						<Show when={pr().headRef}>
							<box width="100%" height={1}>
								<text truncate={true}>
									<span style={{ fg: "cyan" }}>{pr().headRef}</span>
									<span style={{ fg: "gray" }}> → </span>
									<span style={{ fg: "cyan" }}>{pr().baseRef}</span>
								</text>
							</box>
						</Show>
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

							{/* Review Threads */}
							<Show when={props.threads.length > 0}>
								<box width="100%" height={1}>
									<text>{""}</text>
								</box>
								<box width="100%">
									<text>
										<span style={{ bold: true, fg: "cyan" }}>
											## Review Threads
										</span>
										<span style={{ fg: "gray" }}>
											{" "}
											{visibleThreads().length} shown
										</span>
										<Show when={hiddenThreadCount() > 0}>
											<span style={{ fg: "gray" }}>
												{" "}
												· {hiddenThreadCount()} hidden
											</span>
										</Show>
									</text>
								</box>
								<Show
									when={visibleThreads().length > 0}
									fallback={
										<box width="100%" paddingLeft={2}>
											<text>
												<span style={{ fg: "gray" }}>
													All threads resolved or hidden.
												</span>
											</text>
										</box>
									}
								>
									<For each={visibleThreads()}>
										{(thread) => (
											<ThreadCard
												thread={thread}
												showBotComments={props.showBotComments}
											/>
										)}
									</For>
								</Show>
							</Show>

							{/* Conversation (issue comments) */}
							<Show when={props.comments.length > 0}>
								<box width="100%" height={1}>
									<text>{""}</text>
								</box>
								<box width="100%">
									<text>
										<span style={{ bold: true, fg: "cyan" }}>
											## Conversation
										</span>
										<span style={{ fg: "gray" }}>
											{" "}
											{visibleComments().length} comment
											{visibleComments().length !== 1 ? "s" : ""}
										</span>
									</text>
								</box>
								<For each={visibleComments()}>
									{(comment) => <CommentRow comment={comment} />}
								</For>
							</Show>
						</box>
					</scrollbox>
				)}
			</Show>

			{/* ── Status bar ────────────────────────────────────── */}
			<Show when={props.pr}>
				<box width="100%" height={1}>
					<text>
						<span style={{ fg: "gray" }}>
							Esc close · o open · r refresh · t{" "}
							{props.showResolved ? "hide" : "show"} resolved · b{" "}
							{props.showBotComments ? "hide" : "show"} bots
						</span>
					</text>
				</box>
			</Show>
		</box>
	);
}
