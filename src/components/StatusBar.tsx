import { createMemo } from "solid-js";
import type { JSX as OpenTuiJSX } from "@opentui/solid";
import type { GitHubNetworkStats } from "../lib/concurrency";
import { theme } from "../lib/theme";

/**
 * A single-line status bar showing keyboard shortcut hints.
 *
 * Renders shared shortcuts (o GitHub, d Devin, r refresh) plus
 * any view-specific extras passed as children.
 * Optional `networkStats` is shown right-aligned: in-flight HTTP calls and waiting
 * work (queries fetching but not yet represented in the HTTP in-flight count).
 */
export function StatusBar(props: {
  children?: OpenTuiJSX.Element;
  networkStats?: GitHubNetworkStats;
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

  return (
    <box flexDirection="row" width="100%" height={1}>
      <box flexGrow={1}>
        <text>
          <span style={{ fg: theme.muted }}>
            j/k nav · ↵ open · o GitHub · d Devin · r refresh
            {props.children}
          </span>
        </text>
      </box>
      <text>
        <span style={{ fg: networkFg() }}>{networkLabel()}</span>
      </text>
    </box>
  );
}
