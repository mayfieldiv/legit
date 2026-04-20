import { createMemo } from "solid-js";
import { Show, type JSX as OpenTuiJSX } from "@opentui/solid";
import type { GitHubNetworkStats } from "../lib/concurrency";
import type { StatusMessage } from "../lib/ui-state";
import { theme } from "../lib/theme";

/**
 * A single-line status bar showing keyboard shortcut hints, or — when a
 * `statusMessage` is active — the message in its place. View-specific
 * shortcut extras are passed as children.
 *
 * Optional `networkStats` is shown right-aligned: in-flight HTTP calls and
 * waiting work (queries fetching but not yet represented in the HTTP in-flight
 * count).
 */
export function StatusBar(props: {
  children?: OpenTuiJSX.Element;
  networkStats?: GitHubNetworkStats;
  statusMessage?: StatusMessage | null;
}) {
  const networkLabel = createMemo(() => {
    const n = props.networkStats;
    if (!n) return "";
    return `${n.inFlight} in-flight · ${n.waiting} waiting`;
  });

  const networkFg = createMemo(() => {
    const n = props.networkStats;
    if (!n) return theme.muted;
    return n.inFlight > 0 || n.waiting > 0 ? theme.accent : theme.muted;
  });

  const messageFg = (kind: StatusMessage["kind"]): string => {
    switch (kind) {
      case "info":
        return theme.accent;
      case "success":
        return theme.success;
      case "error":
        return theme.error;
    }
  };

  return (
    <box flexDirection="row" width="100%" height={1}>
      <box flexGrow={1}>
        <Show
          when={props.statusMessage}
          fallback={
            <text>
              <span style={{ fg: theme.muted }}>
                j/k nav · ↵ open · o GitHub · d Devin · r refresh · w worktree
                {props.children}
              </span>
            </text>
          }
        >
          {(msg) => (
            <text>
              <span style={{ fg: messageFg(msg().kind), bold: msg().kind === "error" }}>
                {msg().text}
              </span>
            </text>
          )}
        </Show>
      </box>
      <text>
        <span style={{ fg: networkFg() }}>{networkLabel()}</span>
      </text>
    </box>
  );
}
