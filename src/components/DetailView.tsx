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
 *
 * Individual reply comments within threads are selectable via j/k and can be
 * opened in the browser with o (deep-linking to the specific reply).
 */

import { Show, For, createMemo, createSignal, createEffect, on } from "solid-js";
import { useKeyboard } from "@opentui/solid";
import { MarkdownBody } from "../lib/markdown";
import { createDetailsController, DetailsCtx, type DetailsController } from "../lib/details-store";
import { formatAge, formatSize, sortCheckRuns, checkIcon, checksSummary } from "../lib/format";
import type { FullReviewThread, IssueComment, PRDetail, ReviewComment } from "../lib/types";
import type { MouseEvent } from "@opentui/core";
import type { ScrollBoxRenderable } from "@opentui/core";
import { StatusBar } from "./StatusBar";
import { theme } from "../lib/theme";

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
	onOpenInDevin?: () => void;
	onOpenUrl?: (url: string) => void;
	onRefresh?: () => void;
}

// ── Focus selection model ───────────────────────────────────────────────────

export type FocusableItem =
	| { kind: "body" }
	| { kind: "thread"; thread: FullReviewThread; url: string }
	| { kind: "reply"; comment: ReviewComment; url: string }
	| { kind: "comment"; comment: IssueComment; url: string };

const ROUNDED_BORDER = {
	topLeft: "╭",
	topRight: "╮",
	bottomLeft: "╰",
	bottomRight: "╯",
	horizontal: "─",
	vertical: "│",
	topT: "┬",
	bottomT: "┴",
	leftT: "├",
	rightT: "┤",
	cross: "┼",
};
const INVISIBLE_BORDER = {
	topLeft: " ",
	topRight: " ",
	bottomLeft: " ",
	bottomRight: " ",
	horizontal: " ",
	vertical: " ",
	topT: " ",
	bottomT: " ",
	leftT: " ",
	rightT: " ",
	cross: " ",
};

function FocusableCard(props: {
	focused: boolean;
	id: string;
	first?: boolean;
	indent?: number;
	onMouseDown?: () => void;
	children: any;
}) {
	return (
		<box
			id={props.id}
			border={true}
			customBorderChars={props.focused ? ROUNDED_BORDER : INVISIBLE_BORDER}
			borderColor={theme.border}
			width="100%"
			flexDirection="column"
			marginTop={props.first ? 0 : -1}
			paddingLeft={props.indent ?? 0}
			zIndex={props.focused ? 1 : 0}
			onMouseDown={(e: MouseEvent) => {
				e.preventDefault();
				props.onMouseDown?.();
			}}
		>
			{props.children}
		</box>
	);
}

// ── Thread / comment sub-components ─────────────────────────────────────────

/** Renders a thread header (file path + resolved status) and its root comment. */
function ThreadCard(props: { thread: FullReviewThread; showBotComments: boolean }) {
	const visibleComments = createMemo(() => {
		if (props.showBotComments) return props.thread.comments;
		return props.thread.comments.filter((c) => !c.isBot);
	});

	// Only show the root (first visible) comment; replies are rendered
	// as separate focusable items outside this card.
	const rootComment = createMemo(() => {
		const first = visibleComments()[0];
		return first ? [first] : [];
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
						<span style={{ fg: theme.accent }}>{location()}</span>
						<span
							style={{ fg: props.thread.isResolved ? theme.success : theme.warning }}
						>
							{props.thread.isResolved ? " ✓ resolved" : " ● unresolved"}
						</span>
					</text>
				</box>
				<For each={rootComment()}>{(comment) => <CommentRow comment={comment} />}</For>
			</box>
		</Show>
	);
}

/** Renders an individual reply within a review thread. Indented with ↳ prefix. */
function ReplyRow(props: { comment: ReviewComment }) {
	return (
		<box flexDirection="column" width="100%" paddingLeft={2}>
			<box width="100%" height={1}>
				<text truncate={true}>
					<span style={{ fg: theme.muted }}>↳ </span>
					<span style={{ fg: props.comment.isBot ? theme.muted : theme.success }}>
						{props.comment.author}
					</span>
					<Show when={props.comment.isBot}>
						<span style={{ fg: theme.muted }}> [bot]</span>
					</Show>
					<span style={{ fg: theme.muted }}> · {formatAge(props.comment.createdAt)}</span>
				</text>
			</box>
			<box width="100%">
				<MarkdownBody source={props.comment.body} />
			</box>
		</box>
	);
}

function CommentRow(props: { comment: ReviewComment | IssueComment }) {
	return (
		<box flexDirection="column" width="100%" paddingLeft={2}>
			<box width="100%" height={1}>
				<text truncate={true}>
					<span style={{ fg: props.comment.isBot ? theme.muted : theme.success }}>
						{props.comment.author}
					</span>
					<Show when={props.comment.isBot}>
						<span style={{ fg: theme.muted }}> [bot]</span>
					</Show>
					<span style={{ fg: theme.muted }}> · {formatAge(props.comment.createdAt)}</span>
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

	const counts = createMemo(() => checksSummary(checks()));

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

	// ── Helper: visible comments for a specific thread ───────────────────
	const getVisibleThreadComments = (thread: FullReviewThread) => {
		if (props.showBotComments) return thread.comments;
		return thread.comments.filter((c) => !c.isBot);
	};

	// ── Focus selection ─────────────────────────────────────────────────
	const [focusedIndex, setFocusedIndex] = createSignal(0);

	const focusableItems = createMemo<FocusableItem[]>(() => {
		const items: FocusableItem[] = [{ kind: "body" }];
		for (const thread of visibleThreads()) {
			const visComments = getVisibleThreadComments(thread);
			if (visComments.length > 0) {
				items.push({ kind: "thread", thread, url: visComments[0]!.url });
				for (let i = 1; i < visComments.length; i++) {
					items.push({
						kind: "reply",
						comment: visComments[i]!,
						url: visComments[i]!.url,
					});
				}
			}
		}
		for (const comment of visibleComments()) {
			items.push({ kind: "comment", comment, url: comment.url });
		}
		return items;
	});

	// ── Render data for thread section ──────────────────────────────────
	type ThreadRenderItem =
		| { kind: "root"; thread: FullReviewThread; focusIdx: number; isFirst: boolean }
		| { kind: "reply"; comment: ReviewComment; focusIdx: number };

	const threadRenderItems = createMemo<ThreadRenderItem[]>(() => {
		const items = focusableItems();
		const result: ThreadRenderItem[] = [];
		let isFirst = true;
		for (let i = 0; i < items.length; i++) {
			const item = items[i]!;
			if (item.kind === "thread") {
				result.push({ kind: "root", thread: item.thread, focusIdx: i, isFirst });
				isFirst = false;
			} else if (item.kind === "reply") {
				result.push({ kind: "reply", comment: item.comment, focusIdx: i });
			}
		}
		return result;
	});

	// ── Render data for conversation section ────────────────────────────
	type CommentRenderItem = { comment: IssueComment; focusIdx: number; isFirst: boolean };

	const commentRenderItems = createMemo<CommentRenderItem[]>(() => {
		const items = focusableItems();
		const result: CommentRenderItem[] = [];
		let isFirst = true;
		for (let i = 0; i < items.length; i++) {
			const item = items[i]!;
			if (item.kind === "comment") {
				result.push({ comment: item.comment, focusIdx: i, isFirst });
				isFirst = false;
			}
		}
		return result;
	});

	// Clamp focus index when the focusable items list changes
	createEffect(
		on(focusableItems, (items) => {
			const idx = focusedIndex();
			if (items.length === 0) {
				setFocusedIndex(0);
			} else if (idx >= items.length) {
				setFocusedIndex(items.length - 1);
			}
		}),
	);

	// Auto-scroll focused item into view
	let scrollRef: ScrollBoxRenderable | undefined;

	createEffect(
		on(
			() => focusedIndex(),
			(idx) => {
				if (scrollRef) {
					scrollRef.scrollChildIntoView(`focusable-${idx}`);
				}
			},
			{ defer: true },
		),
	);

	// ── Details controllers (one per focusable card) ─────────────────────────────
	const detailsControllers = new Map<number, DetailsController>();
	function getDetailsController(idx: number): DetailsController {
		let ctrl = detailsControllers.get(idx);
		if (!ctrl) {
			ctrl = createDetailsController();
			detailsControllers.set(idx, ctrl);
		}
		return ctrl;
	}

	// ── Keyboard ───────────────────────────────────────────────────────────
	useKeyboard((event) => {
		const name = event.name;
		if (name === "escape") {
			props.onExit?.();
		} else if (name === "j" || name === "down") {
			const items = focusableItems();
			if (items.length > 0) {
				setFocusedIndex((i) => Math.min(i + 1, items.length - 1));
			}
		} else if (name === "k" || name === "up") {
			setFocusedIndex((i) => Math.max(i - 1, 0));
		} else if (name === "t") {
			props.onToggleResolved?.();
		} else if (name === "b") {
			props.onToggleBotComments?.();
		} else if (name === "o") {
			const idx = focusedIndex();
			const items = focusableItems();
			const item = items[idx];
			if (
				item &&
				(item.kind === "thread" || item.kind === "reply" || item.kind === "comment")
			) {
				props.onOpenUrl?.(item.url);
			} else {
				props.onOpenInBrowser?.();
			}
		} else if (name === "d") {
			props.onOpenInDevin?.();
		} else if (name === "r" && !event.shift) {
			props.onRefresh?.();
		} else if (name === "return") {
			const idx = focusedIndex();
			const ctrl = detailsControllers.get(idx);
			if (ctrl) ctrl.toggleAll();
		}
	});

	return (
		<box flexDirection="column" width="100%" height="100%">
			{/* ── Header (pinned) ──────────────────────────────────── */}
			<Show
				when={props.pr}
				fallback={
					<Show when={props.loading}>
						<text>
							<span style={{ fg: theme.warning }}>Loading PR detail...</span>
						</text>
					</Show>
				}
			>
				{(pr) => (
					<>
						<box width="100%" height={1}>
							<text wrapMode="none" truncate={true}>
								<span style={{ bold: true }}>
									#{pr().number} {pr().title}
								</span>
							</text>
						</box>
						<box width="100%" height={1}>
							<text truncate={true}>
								<span style={{ fg: theme.success }}>{pr().author}</span>
								<Show when={pr().repoSlug}>
									<span style={{ fg: theme.muted }}> · </span>
									<span style={{ fg: theme.info }}>{pr().repoSlug}</span>
								</Show>
								<span style={{ fg: theme.muted }}>
									{" "}
									· created {formatAge(pr().createdAt)}
								</span>
								<span style={{ fg: theme.muted }}>
									{" "}
									· updated {formatAge(pr().updatedAt)}
								</span>
								<span style={{ fg: theme.muted }}>
									{" "}
									· {formatSize(pr().additions, pr().deletions)}
								</span>
								<Show when={pr().isDraft}>
									<span style={{ fg: theme.warning }}> draft</span>
								</Show>
							</text>
						</box>
						<Show when={pr().repoSlug}>
							<box width="100%" height={1}>
								<text wrapMode="none" truncate={true}>
									<span style={{ fg: theme.info, underline: true }}>
										https://github.com/{pr().repoSlug}/pull/{pr().number}
									</span>
								</text>
							</box>
						</Show>
						<Show when={pr().headRef}>
							<box width="100%" height={1}>
								<text wrapMode="none" truncate={true}>
									<span style={{ fg: theme.accent }}>{pr().headRef}</span>
									<span style={{ fg: theme.muted }}> → </span>
									<span style={{ fg: theme.accent }}>{pr().baseRef}</span>
								</text>
							</box>
						</Show>
						<box width="100%" height={1}>
							<text>
								<span style={{ fg: theme.muted }}>
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
					<scrollbox
						ref={(el: ScrollBoxRenderable) => {
							scrollRef = el;
							el.focusable = false;
						}}
						flexGrow={1}
						width="100%"
					>
						{/* Description (focusable-0, unstyled) */}
						<box id="focusable-0" flexDirection="column" width="100%">
							<DetailsCtx.Provider value={getDetailsController(0)}>
								<Show
									when={pr().body.trim()}
									fallback={
										<text>
											<span style={{ fg: theme.muted }}>No description.</span>
										</text>
									}
								>
									<MarkdownBody source={pr().body} />
								</Show>
							</DetailsCtx.Provider>
						</box>

						{/* CI Checks */}
						<Show when={checks().length > 0}>
							<box width="100%" height={1}>
								<text>{""}</text>
							</box>
							<box width="100%">
								<text>
									<span style={{ bold: true, fg: theme.accent }}>
										## CI Checks
									</span>
									<span style={{ fg: theme.muted }}>
										{" "}
										{counts().passed}/{counts().total} passed
									</span>
									<Show when={counts().failed > 0}>
										<span style={{ fg: theme.error }}>
											{" "}
											· {counts().failed} failed
										</span>
									</Show>
									<Show when={counts().pending > 0}>
										<span style={{ fg: theme.warning }}>
											{" "}
											· {counts().pending} pending
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
									<span style={{ bold: true, fg: theme.accent }}>
										## Review Threads
									</span>
									<span style={{ fg: theme.muted }}>
										{" "}
										{visibleThreads().length} shown
									</span>
									<Show when={hiddenThreadCount() > 0}>
										<span style={{ fg: theme.muted }}>
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
											<span style={{ fg: theme.muted }}>
												All threads resolved or hidden.
											</span>
										</text>
									</box>
								}
							>
								<For each={threadRenderItems()}>
									{(item) => (
										<Show
											when={item.kind === "root" ? item : undefined}
											fallback={
												<FocusableCard
													id={`focusable-${item.focusIdx}`}
													focused={focusedIndex() === item.focusIdx}
													indent={4}
													onMouseDown={() =>
														setFocusedIndex(item.focusIdx)
													}
												>
													<DetailsCtx.Provider
														value={getDetailsController(item.focusIdx)}
													>
														<ReplyRow
															comment={
																(
																	item as ThreadRenderItem & {
																		kind: "reply";
																	}
																).comment
															}
														/>
													</DetailsCtx.Provider>
												</FocusableCard>
											}
										>
											{(rootItem) => (
												<FocusableCard
													id={`focusable-${rootItem().focusIdx}`}
													focused={focusedIndex() === rootItem().focusIdx}
													first={rootItem().isFirst}
													onMouseDown={() =>
														setFocusedIndex(rootItem().focusIdx)
													}
												>
													<DetailsCtx.Provider
														value={getDetailsController(
															rootItem().focusIdx,
														)}
													>
														<ThreadCard
															thread={rootItem().thread}
															showBotComments={props.showBotComments}
														/>
													</DetailsCtx.Provider>
												</FocusableCard>
											)}
										</Show>
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
									<span style={{ bold: true, fg: theme.accent }}>
										## Conversation
									</span>
									<span style={{ fg: theme.muted }}>
										{" "}
										{visibleComments().length} comment
										{visibleComments().length !== 1 ? "s" : ""}
									</span>
								</text>
							</box>
							<For each={commentRenderItems()}>
								{(item) => (
									<FocusableCard
										id={`focusable-${item.focusIdx}`}
										focused={focusedIndex() === item.focusIdx}
										first={item.isFirst}
										onMouseDown={() => setFocusedIndex(item.focusIdx)}
									>
										<DetailsCtx.Provider
											value={getDetailsController(item.focusIdx)}
										>
											<CommentRow comment={item.comment} />
										</DetailsCtx.Provider>
									</FocusableCard>
								)}
							</For>
						</Show>
					</scrollbox>
				)}
			</Show>

			{/* ── Status bar ────────────────────────────────────── */}
			<Show when={props.pr}>
				<StatusBar>
					{" · "}Esc back · t {props.showResolved ? "hide" : "show"} resolved · b{" "}
					{props.showBotComments ? "hide" : "show"} bots
				</StatusBar>
			</Show>
		</box>
	);
}
