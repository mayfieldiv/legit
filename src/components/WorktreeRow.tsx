/**
 * Shared rendering of the git-worktree indicator: the Nerd Font branch glyph,
 * the `worktree: ~/…` label, and a middle-truncated path.
 */

import { theme } from "../lib/theme";
import { abbreviateHome, truncateMiddle } from "../lib/format";

/** Nerd Font `nf-dev-git_branch` glyph. Used wherever the worktree indicator appears. */
export const WORKTREE_GLYPH = "\ue725";

export function WorktreeRow(props: { path: string; maxWidth: number }) {
  return (
    <box height={1} width="100%">
      <text wrapMode="none" truncate={true}>
        <span style={{ fg: theme.accent }}>{WORKTREE_GLYPH}</span>
        <span style={{ fg: theme.muted }}> worktree: </span>
        <span>{truncateMiddle(abbreviateHome(props.path), props.maxWidth)}</span>
      </text>
    </box>
  );
}
