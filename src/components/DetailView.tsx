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

import { Show, For, useKeyboard } from "@opentui/solid";
import type { JSX as OpenTuiJSX } from "@opentui/solid";
import { createMemo, createSignal, createEffect } from "solid-js";
import { MarkdownBody } from "../lib/markdown";
import { createDetailsController, DetailsCtx, type DetailsController } from "../lib/details-store";
import {
  formatAge,
  formatSize,
  formatMergeable,
  sortCheckRuns,
  checkIcon,
  checksSummary,
} from "../lib/format";
import type { FullReviewThread, IssueComment, PRDetail, ReviewComment } from "../lib/types";
import { classifyThread } from "../lib/blocker-engine";
import type { BorderCharacters, MouseEvent, ScrollBoxRenderable } from "@opentui/core";
import { StatusBar } from "./StatusBar";
import { theme } from "../lib/theme";
import type { Accessor } from "solid-js";
import { WorktreeRow } from "./WorktreeRow";
import { useAppContext } from "../app-context";

// ── Context data ────────────────────────────────────────────────────────────

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
} satisfies BorderCharacters;

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
} satisfies BorderCharacters;

function FocusableCard(props: {
  focused: boolean;
  id: string;
  first?: boolean;
  indent?: number;
  onMouseDown?: () => void;
  children: OpenTuiJSX.Element;
}) {
  return (
    <box
      id={props.id}
      border={true}
      borderStyle="rounded"
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
            {(() => {
              if (props.thread.isResolved) {
                return <span style={{ fg: theme.success }}>{" ✓ resolved"}</span>;
              }
              const status = classifyThread(props.thread);
              if (status === "awaiting-reviewer") {
                return <span style={{ fg: theme.info }}>{" ◐ awaiting reviewer"}</span>;
              }
              return <span style={{ fg: theme.warning }}>{" ● unreplied"}</span>;
            })()}
          </text>
        </box>
        <For each={rootComment()}>{(comment) => <CommentRow comment={comment()} />}</For>
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

function prIdentityKey(pr: PRDetail | undefined): string {
  if (!pr) return "";
  return `${pr.repoSlug ?? ""}#${pr.number}`;
}

export function DetailView() {
  const app = useAppContext();
  const detailPr = app.detail.pr;
  const threads = () => app.detail.threads() ?? [];
  const comments = app.detail.comments;
  const checks = createMemo(() => sortCheckRuns(app.detail.checks() ?? []));

  const counts = createMemo(() => checksSummary(checks()));

  // ── Filtered threads ─────────────────────────────────────────────────
  const visibleThreads = createMemo(() => {
    let visible = threads();
    if (!app.detail.showResolved()) {
      visible = visible.filter((t) => !t.isResolved);
    }
    if (!app.detail.showBotComments()) {
      visible = visible.filter((t) => t.comments.some((c) => !c.isBot));
    }
    return visible;
  });

  const hiddenThreadCount = createMemo(() => threads().length - visibleThreads().length);

  // ── Filtered issue comments ──────────────────────────────────────────
  const visibleComments = createMemo(() => {
    if (app.detail.showBotComments()) return comments();
    return comments().filter((c) => !c.isBot);
  });

  // ── Helper: visible comments for a specific thread ───────────────────
  const getVisibleThreadComments = (thread: FullReviewThread) => {
    if (app.detail.showBotComments()) return thread.comments;
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
    () => focusableItems(),
    (items) => {
      const idx = focusedIndex();
      if (items.length === 0) {
        setFocusedIndex(0);
      } else if (idx >= items.length) {
        setFocusedIndex(items.length - 1);
      }
    },
  );

  // Auto-scroll focused item into view
  let scrollRef: ScrollBoxRenderable | undefined;

  let didProcessFocusedIndex = false;
  createEffect(
    () => focusedIndex(),
    (idx) => {
      if (!didProcessFocusedIndex) {
        didProcessFocusedIndex = true;
        return;
      }
      if (scrollRef) {
        scrollRef.scrollChildIntoView(`focusable-${idx}`);
      }
    },
  );

  // ── Details controllers (one per focusable card) ─────────────────────────────
  // Scoped to the currently focused PR. When the PR identity changes (the user
  // navigates to a different PR detail) we drop the prior controllers — their
  // <details> registrations are no longer rendered, and reusing the Map by
  // focusable index would accumulate stale signal references over a session.
  // We skip the initial run because <details> register during the same render
  // pass that establishes this effect; clearing on the initial fire would wipe
  // the registrations we want to keep.
  const detailsControllers = new Map<number, DetailsController>();
  let prevPrIdentity: string | undefined;
  createEffect(
    () => prIdentityKey(detailPr()),
    (next) => {
      // Treat the empty-string sentinel (detailPr() === undefined) the same as
      // the initial undefined: those states never registered controllers, so
      // transitioning out of them must NOT clear. Otherwise the cache-miss →
      // detail-fetch landed transition would wipe the controllers that the
      // very same render just registered, and Enter-toggleAll would no-op.
      if (prevPrIdentity && next !== prevPrIdentity) {
        detailsControllers.clear();
      }
      prevPrIdentity = next;
    },
  );
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
      app.actions.exitDetail();
    } else if (name === "j" || name === "down") {
      const items = focusableItems();
      if (items.length > 0) {
        setFocusedIndex((i) => Math.min(i + 1, items.length - 1));
      }
    } else if (name === "k" || name === "up") {
      setFocusedIndex((i) => Math.max(i - 1, 0));
    } else if (name === "t") {
      app.actions.toggleResolved();
    } else if (name === "b") {
      app.actions.toggleBotComments();
    } else if (name === "o") {
      const idx = focusedIndex();
      const items = focusableItems();
      const item = items[idx];
      if (item && (item.kind === "thread" || item.kind === "reply" || item.kind === "comment")) {
        app.actions.openUrl(item.url);
      } else {
        const currentPr = detailPr();
        if (currentPr) app.actions.openInBrowser(currentPr);
      }
    } else if (name === "d") {
      const currentPr = detailPr();
      if (currentPr) app.actions.openInDevin(currentPr);
    } else if (name === "w") {
      const currentPr = detailPr();
      if (currentPr) app.actions.createWorktree(currentPr);
    } else if (name === "r" && !event.shift) {
      app.actions.refreshDetail();
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
        when={detailPr()}
        fallback={
          <Show when={app.detail.loading()}>
            <text>
              <span style={{ fg: theme.warning }}>Loading PR detail...</span>
            </text>
          </Show>
        }
      >
        {(pr: Accessor<PRDetail>) => (
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
                <span style={{ fg: theme.muted }}> · created {formatAge(pr().createdAt)}</span>
                <span style={{ fg: theme.muted }}> · updated {formatAge(pr().updatedAt)}</span>
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
                  {(() => {
                    const m = formatMergeable(pr().mergeable);
                    return (
                      <>
                        <span style={{ fg: theme.muted }}> · </span>
                        <span style={{ fg: m.fg }}>{m.text}</span>
                      </>
                    );
                  })()}
                </text>
              </box>
            </Show>
            <Show when={app.detail.worktree()}>
              {(wt) => <WorktreeRow path={wt().path} maxWidth={80} />}
            </Show>
            <box width="100%" height={1}>
              <text>
                <span style={{ fg: theme.muted }}>────────────────────────────────────────</span>
              </text>
            </box>
          </>
        )}
      </Show>

      {/* ── Scrollable body ──────────────────────────────────── */}
      <Show when={detailPr()}>
        {(pr: Accessor<PRDetail>) => (
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
              <DetailsCtx value={getDetailsController(0)}>
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
              </DetailsCtx>
            </box>

            {/* CI Checks */}
            <Show when={checks().length > 0}>
              <box width="100%" height={1}>
                <text>{""}</text>
              </box>
              <box width="100%">
                <text>
                  <span style={{ bold: true, fg: theme.accent }}>## CI Checks</span>
                  <span style={{ fg: theme.muted }}>
                    {" "}
                    {counts().passed}/{counts().total} passed
                  </span>
                  <Show when={counts().failed > 0}>
                    <span style={{ fg: theme.error }}> · {counts().failed} failed</span>
                  </Show>
                  <Show when={counts().pending > 0}>
                    <span style={{ fg: theme.warning }}> · {counts().pending} pending</span>
                  </Show>
                </text>
              </box>
              <For each={checks()}>
                {(check) => {
                  const ci = () => checkIcon(check());
                  return (
                    <box width="100%" height={1}>
                      <text truncate={true}>
                        <span>{"  "}</span>
                        <span style={{ fg: ci().fg }}>{ci().icon}</span>
                        <span> {check().name}</span>
                      </text>
                    </box>
                  );
                }}
              </For>
            </Show>

            {/* Review Threads */}
            <Show when={threads().length > 0}>
              <box width="100%" height={1}>
                <text>{""}</text>
              </box>
              <box width="100%">
                <text>
                  <span style={{ bold: true, fg: theme.accent }}>## Review Threads</span>
                  <span style={{ fg: theme.muted }}> {visibleThreads().length} shown</span>
                  <Show when={hiddenThreadCount() > 0}>
                    <span style={{ fg: theme.muted }}> · {hiddenThreadCount()} hidden</span>
                  </Show>
                </text>
              </box>
              <Show
                when={visibleThreads().length > 0}
                fallback={
                  <box width="100%" paddingLeft={2}>
                    <text>
                      <span style={{ fg: theme.muted }}>All threads resolved or hidden.</span>
                    </text>
                  </box>
                }
              >
                <For each={threadRenderItems()}>
                  {(item) => (
                    <Show
                      when={
                        (item().kind === "root" ? item() : undefined) as
                          | Extract<ThreadRenderItem, { kind: "root" }>
                          | undefined
                      }
                      fallback={
                        <FocusableCard
                          id={`focusable-${item().focusIdx}`}
                          focused={focusedIndex() === item().focusIdx}
                          indent={4}
                          onMouseDown={() => setFocusedIndex(item().focusIdx)}
                        >
                          <DetailsCtx value={getDetailsController(item().focusIdx)}>
                            <ReplyRow
                              comment={
                                (
                                  item() as ThreadRenderItem & {
                                    kind: "reply";
                                  }
                                ).comment
                              }
                            />
                          </DetailsCtx>
                        </FocusableCard>
                      }
                    >
                      {(
                        rootItem: Accessor<{
                          kind: "root";
                          thread: FullReviewThread;
                          focusIdx: number;
                          isFirst: boolean;
                        }>,
                      ) => (
                        <FocusableCard
                          id={`focusable-${rootItem().focusIdx}`}
                          focused={focusedIndex() === rootItem().focusIdx}
                          first={rootItem().isFirst}
                          onMouseDown={() => setFocusedIndex(rootItem().focusIdx)}
                        >
                          <DetailsCtx value={getDetailsController(rootItem().focusIdx)}>
                            <ThreadCard
                              thread={rootItem().thread}
                              showBotComments={app.detail.showBotComments()}
                            />
                          </DetailsCtx>
                        </FocusableCard>
                      )}
                    </Show>
                  )}
                </For>
              </Show>
            </Show>

            {/* Conversation (issue comments) */}
            <Show when={comments().length > 0}>
              <box width="100%" height={1}>
                <text>{""}</text>
              </box>
              <box width="100%">
                <text>
                  <span style={{ bold: true, fg: theme.accent }}>## Conversation</span>
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
                    id={`focusable-${item().focusIdx}`}
                    focused={focusedIndex() === item().focusIdx}
                    first={item().isFirst}
                    onMouseDown={() => setFocusedIndex(item().focusIdx)}
                  >
                    <DetailsCtx value={getDetailsController(item().focusIdx)}>
                      <CommentRow comment={item().comment} />
                    </DetailsCtx>
                  </FocusableCard>
                )}
              </For>
            </Show>
          </scrollbox>
        )}
      </Show>

      {/* ── Status bar ────────────────────────────────────── */}
      <Show when={detailPr()}>
        <StatusBar networkStats={app.status.networkStats()} statusMessage={app.status.message()}>
          {" · "}Esc back · t {app.detail.showResolved() ? "hide" : "show"} resolved · b{" "}
          {app.detail.showBotComments() ? "hide" : "show"} bots · w worktree
        </StatusBar>
      </Show>
    </box>
  );
}
