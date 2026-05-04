import { Show, Switch, Match, useTerminalDimensions } from "@opentui/solid";
import { createMemo } from "solid-js";
import { ListView } from "./ListView";
import { SummaryPanel } from "./SummaryPanel";
import { DetailView } from "./DetailView";
import type { GroupByKey } from "../lib/group-filter-engine";
import { theme } from "../lib/theme";
import { computeVisibleColumns } from "./PRList";
import { useAppContext } from "../app-context";

export type { ViewTarget } from "../lib/ui-state";

interface AppShellProps {
  showRepo?: boolean;
  /** Initial grouping key for the list view. Default: "smart-status". */
  groupBy?: GroupByKey;
}

/** Summary panel width: full at wide widths, narrower when tight, hidden when very narrow. */
const SUMMARY_FULL = 50;
const SUMMARY_NARROW = 36;
const SUMMARY_DIVIDER = 1;
/** Minimum terminal width to show the summary panel at all. */
const SUMMARY_MIN_TERM_WIDTH = 80;

export function AppShell(props: AppShellProps) {
  const app = useAppContext();
  const tabCount = () => app.prData.tabs().length;
  const inListView = () => app.detail.view().view === "list";
  const dims = useTerminalDimensions();

  /** Whether to show the summary panel. */
  const showSummary = () => dims().width >= SUMMARY_MIN_TERM_WIDTH;

  /** Width of the summary panel (0 when hidden). */
  const summaryWidth = () => {
    if (!showSummary()) return 0;
    return dims().width >= 140 ? SUMMARY_FULL : SUMMARY_NARROW;
  };

  /** Available width for the list (excluding summary + divider). */
  const listWidth = () => {
    const sw = summaryWidth();
    return dims().width - (sw > 0 ? sw + SUMMARY_DIVIDER : 0);
  };

  /** Responsive column visibility. */
  const visibleColumns = createMemo(() =>
    computeVisibleColumns(listWidth(), props.showRepo ?? false),
  );

  return (
    <box flexDirection="column" width="100%" height="100%">
      {/* Header */}
      <box flexDirection="row" width="100%" height={1}>
        <text>
          <span style={{ fg: theme.accent, bold: true }}>legit</span>
          <Show when={inListView()}>
            <span> — </span>
            <b>{app.prData.repoSlug()}</b>
            <span> — {app.prData.prs().length} open PRs</span>
          </Show>
        </text>
      </box>

      <Show when={tabCount() > 0 && inListView()}>
        <box flexDirection="row" width="100%" height={1}>
          <text>
            {app.prData.tabs().map((tab, i) => {
              const selected = i === app.prData.activeTab();
              return `${selected ? "[" : " "}${tab}${selected ? "]" : " "} `;
            })}
          </text>
        </box>
      </Show>

      {/* Error */}
      <Show when={app.prData.error()}>
        <text>
          <span style={{ fg: theme.error }}>Error: {app.prData.error()}</span>
        </text>
      </Show>

      {/* Content — hide when error with no data (first-load failure) */}
      <Show
        when={app.prData.prs().length > 0 || (!app.prData.loading() && !app.prData.error())}
        fallback={
          <Show when={app.prData.loading()}>
            <text>
              <span style={{ fg: theme.warning }}>Loading pull requests...</span>
            </text>
          </Show>
        }
      >
        <Switch>
          <Match when={app.detail.view().view === "list"}>
            <box flexDirection="row" flexGrow={1} width="100%">
              <ListView
                showRepo={props.showRepo}
                groupBy={props.groupBy ?? "smart-status"}
                resetKey={app.prData.activeTab()}
                visibleColumns={visibleColumns()}
              />
              <Show when={showSummary()}>
                <box width={1} height="100%">
                  <text>│</text>
                </box>
                <box width={summaryWidth()}>
                  <SummaryPanel />
                </box>
              </Show>
            </box>
          </Match>
          <Match when={app.detail.view().view === "detail"}>
            <DetailView />
          </Match>
        </Switch>
      </Show>
    </box>
  );
}
