import { For, Show, useTerminalDimensions } from "@opentui/solid";
import type { JSX as OpenTuiJSX } from "@opentui/solid";
import { createMemo } from "solid-js";
import type { Accessor } from "solid-js";
import type { PR, CommentCounts } from "../lib/types";
import type { PRIdentity } from "../lib/pr-identity";
import type { MouseEvent } from "@opentui/core";
import { formatAge, formatSize, formatRepoShort } from "../lib/format";
import { theme } from "../lib/theme";
import { derivePRState, type BlockerDisplayTone, type PRDerivedState } from "../lib/pr-state";
import { WORKTREE_GLYPH } from "./WorktreeRow";

// ── Flat item type (group headers + PR rows) ─────────────────────────────────

/** A single display item in the PR list: either a group header or a PR row. */
export type FlatItem =
  | { kind: "header"; label: string }
  | { kind: "pr"; prIndex: number; prKey: string; pr: PR };

function prLookupKey(pr: PRIdentity): string {
  return `${pr.repoSlug ?? ""}#${pr.number}`;
}

/**
 * Build a flat display list from groups (including headers).
 * Groups with empty labels (i.e. "none" grouping) produce no header row.
 */
export function buildFlatItems(groups: Array<{ label: string; prs: PR[] }>): FlatItem[] {
  const items: FlatItem[] = [];
  let prIndex = 0;
  for (const group of groups) {
    if (group.label) {
      items.push({ kind: "header", label: group.label });
    }
    for (const pr of group.prs) {
      items.push({ kind: "pr", pr, prKey: prLookupKey(pr), prIndex: prIndex++ });
    }
  }
  return items;
}

/**
 * Map a PR selection index to its row position in a flat items list.
 * Used to compute the scroll target when groups are present.
 */
export function prIndexToDisplayRow(items: FlatItem[], prIndex: number): number {
  let prCount = 0;
  for (let i = 0; i < items.length; i++) {
    const item = items[i]!;
    if (item.kind === "pr") {
      if (prCount === prIndex) return i;
      prCount++;
    }
  }
  return prIndex; // fallback: flat list
}

interface PRListProps {
  /** Flat list of PRs (backward compat — used when `items` is not provided). */
  prs?: PR[];
  /** Pre-built flat items list (with optional group headers). Overrides `prs`. */
  items?: FlatItem[];
  selectedIndex: number;
  showRepo?: boolean;
  currentUser?: string;
  onSelect?: (index: number) => void;
  /** Which optional columns are visible (responsive). Shows all if omitted. */
  visibleColumns?: VisibleColumns;
  /** Lookup function for derived PR state. */
  getPRState?: (pr: PR) => PRDerivedState;
  /** Queue/refresh marker state for this PR row. */
  getRefreshState?: (pr: PR) => "queued" | "refreshing" | undefined;
}

// Column widths — fixed columns; title gets remaining space via flexGrow
const COL = {
  worktree: 2,
  pr: 7,
  repo: 14,
  author: 14,
  size: 14,
  age: 6,
  review: 18,
  threads: 10,
  blocker: 14,
} as const;

/** Set of optional columns that can be hidden at narrow widths. */
export interface VisibleColumns {
  author: boolean;
  size: boolean;
  age: boolean;
  review: boolean;
  threads: boolean;
  blocker: boolean;
}

/**
 * Compute which optional columns to show given the available list width.
 * Columns are hidden progressively from least to most important:
 *   blocker → threads → review → size → author → age
 */
export function computeVisibleColumns(listWidth: number, showRepo: boolean): VisibleColumns {
  // Base: Worktree(2) + PR(7) + Title(30 min) = 39, plus Repo(14) if shown
  const base = COL.worktree + COL.pr + 30 + (showRepo ? COL.repo : 0);
  let budget = listWidth - base;

  // Add columns in priority order (most important first)
  const cols: VisibleColumns = {
    age: false,
    author: false,
    size: false,
    review: false,
    threads: false,
    blocker: false,
  };

  // age (6) — very compact, show early
  if (budget >= COL.age) {
    cols.age = true;
    budget -= COL.age;
  }
  // author (14)
  if (budget >= COL.author) {
    cols.author = true;
    budget -= COL.author;
  }
  // size (14)
  if (budget >= COL.size) {
    cols.size = true;
    budget -= COL.size;
  }
  // review (18)
  if (budget >= COL.review) {
    cols.review = true;
    budget -= COL.review;
  }
  // threads (10)
  if (budget >= COL.threads) {
    cols.threads = true;
    budget -= COL.threads;
  }
  // blocker (14)
  if (budget >= COL.blocker) {
    cols.blocker = true;
    budget -= COL.blocker;
  }

  return cols;
}

/**
 * Build the per-span parts for the Threads cell.
 * Returns an array of `{ text, fg }` items (empty array = nothing to show).
 */
function threadParts(
  comments: CommentCounts | undefined,
  selected: boolean,
): Array<{ text: string; fg: string }> {
  if (!comments || comments.unresolved === 0) return [];
  const parts: Array<{ text: string; fg: string }> = [];
  if (comments.unresolvedHuman > 0) {
    parts.push({
      text: `${comments.unresolvedHuman}H`,
      fg: selected ? theme.selectedFg : theme.warning,
    });
  }
  if (comments.unresolvedHuman > 0 && comments.unresolvedBot > 0) {
    parts.push({ text: " ", fg: theme.neutral });
  }
  if (comments.unresolvedBot > 0) {
    parts.push({
      text: `${comments.unresolvedBot}B`,
      fg: selected ? theme.selectedFg : theme.muted,
    });
  }
  return parts;
}

/** Compute the fg color for the review column (priority: conflict > draft > default). */
function reviewCellFg(pr: PR, selected: boolean): string | undefined {
  if (selected) return theme.selectedFg;
  if (pr.mergeable === "CONFLICTING") return theme.error;
  if (pr.isDraft) return theme.warning;
  return undefined;
}

function blockerToneColor(tone: BlockerDisplayTone): string {
  switch (tone) {
    case "self":
      return theme.selfHighlight;
    case "warning":
      return theme.warning;
    case "muted":
      return theme.muted;
  }
}

function GroupHeaderRow(props: { label: string }) {
  return (
    <box height={1} width="100%">
      <text wrapMode="none" truncate={true}>
        <span style={{ fg: theme.accent }}>── {props.label} </span>
      </text>
    </box>
  );
}

function Cell(props: {
  width?: number;
  flexGrow?: number;
  minWidth?: number;
  paddingRight?: number;
  children: OpenTuiJSX.Element;
}) {
  return (
    <box
      width={props.width}
      flexGrow={props.flexGrow}
      flexShrink={props.width !== undefined ? 0 : 1}
      minWidth={props.minWidth}
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
  showRepo?: boolean;
  currentUser?: string;
  id: string;
  onMouseDown?: (e: MouseEvent) => void;
  visibleColumns?: VisibleColumns;
  getPRState?: (pr: PR) => PRDerivedState;
  getRefreshState?: (pr: PR) => "queued" | "refreshing" | undefined;
}) {
  const fg = () => (props.selected ? theme.selectedFg : undefined);
  const prState = createMemo(
    () =>
      props.getPRState?.(props.pr) ??
      derivePRState(props.pr, {
        currentUser: props.currentUser,
      }),
  );
  const reviewText = () => {
    const conflict = props.pr.mergeable === "CONFLICTING" ? "! " : "";
    const draft = props.pr.isDraft ? "draft " : "";
    return conflict + draft + prState().reviewText;
  };
  const comments = (): CommentCounts | undefined => prState().commentCounts;
  const blockerCell = () => prState().blockerDisplay;
  const enrichmentLoading = () => prState().loading;
  const refreshState = () => props.getRefreshState?.(props.pr);
  return (
    <box
      id={props.id}
      flexDirection="row"
      width="100%"
      height={1}
      backgroundColor={props.selected ? theme.selectedBg : undefined}
      onMouseDown={props.onMouseDown}
    >
      <Cell width={COL.worktree} paddingRight={1}>
        <Show
          when={refreshState()}
          fallback={
            <Show when={prState().worktree}>
              <span style={{ fg: props.selected ? theme.selectedFg : theme.accent }}>
                {WORKTREE_GLYPH}
              </span>
            </Show>
          }
        >
          {(state) => (
            <span
              style={{
                fg: props.selected
                  ? theme.selectedFg
                  : state() === "refreshing"
                    ? theme.accent
                    : theme.warning,
              }}
            >
              {state() === "refreshing" ? "⟳" : "◌"}
            </span>
          )}
        </Show>
      </Cell>
      <Cell width={COL.pr} paddingRight={1}>
        <span style={{ fg: props.selected ? theme.selectedFg : theme.accent }}>
          #{props.pr.number}
        </span>
      </Cell>
      <Show when={props.showRepo}>
        <Cell width={COL.repo} paddingRight={1}>
          <span style={{ fg: props.selected ? theme.selectedFg : theme.selfHighlight }}>
            {formatRepoShort(props.pr.repoSlug)}
          </span>
        </Cell>
      </Show>
      <Cell flexGrow={1} minWidth={10}>
        <span style={{ fg: fg() }}>{props.pr.title}</span>
      </Cell>
      <box width={props.showRepo ? 2 : 3} />
      <Show when={props.visibleColumns?.author !== false}>
        <>
          <Cell width={COL.author} paddingRight={1}>
            <span style={{ fg: props.selected ? theme.selectedFg : theme.success }}>
              {` ${props.pr.author}`}
            </span>
          </Cell>
          <box width={1} />
        </>
      </Show>
      <Show when={props.visibleColumns?.size !== false}>
        <Cell width={COL.size} paddingRight={1}>
          <span style={{ fg: fg() }}>{formatSize(props.pr.additions, props.pr.deletions)}</span>
        </Cell>
      </Show>
      <Show when={props.visibleColumns?.age !== false}>
        <Cell width={COL.age} paddingRight={1}>
          <span style={{ fg: fg() }}>{formatAge(props.pr.createdAt)}</span>
        </Cell>
      </Show>
      <Show when={props.visibleColumns?.review !== false}>
        <Cell width={COL.review} paddingRight={1}>
          <span style={{ fg: reviewCellFg(props.pr, props.selected) }}>{reviewText()}</span>
        </Cell>
      </Show>
      <Show when={props.visibleColumns?.threads !== false}>
        <Cell width={COL.threads} paddingRight={props.currentUser ? 1 : 0}>
          <Show
            when={comments() !== undefined}
            fallback={
              <Show when={enrichmentLoading()}>
                <span style={{ fg: theme.muted }}>…</span>
              </Show>
            }
          >
            <For each={threadParts(comments(), props.selected)}>
              {(part) => <span style={{ fg: part().fg }}>{part().text}</span>}
            </For>
          </Show>
        </Cell>
      </Show>
      <Show when={props.currentUser && props.visibleColumns?.blocker !== false}>
        <Cell width={COL.blocker}>
          <Show
            when={!enrichmentLoading() && blockerCell()}
            fallback={
              <Show when={enrichmentLoading()}>
                <span style={{ fg: theme.muted }}>…</span>
              </Show>
            }
          >
            {(display: Accessor<{ text: string; tone: BlockerDisplayTone }>) => (
              <span
                style={{
                  fg: props.selected ? theme.selectedFg : blockerToneColor(display().tone),
                }}
              >
                {display().text}
              </span>
            )}
          </Show>
        </Cell>
      </Show>
    </box>
  );
}

export function PRListHeader(props: {
  showRepo?: boolean;
  currentUser?: string;
  visibleColumns?: VisibleColumns;
}) {
  return (
    <box flexDirection="row" width="100%" height={1}>
      <Cell width={COL.worktree} paddingRight={1}>
        <span />
      </Cell>
      <Cell width={COL.pr} paddingRight={1}>
        <b>PR</b>
      </Cell>
      <Show when={props.showRepo}>
        <Cell width={COL.repo} paddingRight={1}>
          <b>Repo</b>
        </Cell>
      </Show>
      <Cell flexGrow={1} minWidth={10}>
        <b>Title</b>
      </Cell>
      <box width={props.showRepo ? 2 : 3} />
      <Show when={props.visibleColumns?.author !== false}>
        <>
          <Cell width={COL.author} paddingRight={1}>
            <b>Author</b>
          </Cell>
          <box width={1} />
        </>
      </Show>
      <Show when={props.visibleColumns?.size !== false}>
        <Cell width={COL.size} paddingRight={1}>
          <b>Size</b>
        </Cell>
      </Show>
      <Show when={props.visibleColumns?.age !== false}>
        <Cell width={COL.age} paddingRight={1}>
          <b>Age</b>
        </Cell>
      </Show>
      <Show when={props.visibleColumns?.review !== false}>
        <Cell width={COL.review} paddingRight={1}>
          <b>Review</b>
        </Cell>
      </Show>
      <Show when={props.visibleColumns?.threads !== false}>
        <Cell width={COL.threads} paddingRight={props.currentUser ? 1 : 0}>
          <b>Threads</b>
        </Cell>
      </Show>
      <Show when={props.currentUser && props.visibleColumns?.blocker !== false}>
        <Cell width={COL.blocker}>
          <b>Blocker</b>
        </Cell>
      </Show>
    </box>
  );
}

export function PRList(props: PRListProps) {
  const dims = useTerminalDimensions();

  // Resolve flat items — prefer `items` prop; fall back to converting `prs`
  const resolvedItems = createMemo((): FlatItem[] => {
    if (props.items) return props.items;
    return (props.prs ?? []).map((pr, i) => ({
      kind: "pr",
      pr,
      prKey: prLookupKey(pr),
      prIndex: i,
    }));
  });

  const resolvedVisibleColumns = createMemo<VisibleColumns>(() => {
    if (props.visibleColumns) return props.visibleColumns;
    return computeVisibleColumns(Math.max(0, dims().width - 12), props.showRepo ?? false);
  });

  const hasPRs = () => resolvedItems().some((item) => item.kind === "pr");

  return (
    <box flexDirection="column" width="100%">
      <Show when={hasPRs()} fallback={<text>No open pull requests</text>}>
        <For each={resolvedItems()}>
          {(item) => {
            const row = item();
            if (row.kind === "header") {
              return <GroupHeaderRow label={row.label} />;
            }
            return (
              <PRRow
                id={`pr-row-${row.prKey}`}
                pr={row.pr}
                selected={row.prIndex === props.selectedIndex}
                showRepo={props.showRepo}
                currentUser={props.currentUser}
                visibleColumns={resolvedVisibleColumns()}
                getPRState={props.getPRState}
                getRefreshState={props.getRefreshState}
                onMouseDown={(e: MouseEvent) => {
                  e.preventDefault();
                  props.onSelect?.(row.prIndex);
                }}
              />
            );
          }}
        </For>
      </Show>
    </box>
  );
}
