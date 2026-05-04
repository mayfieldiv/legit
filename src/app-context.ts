import { createContext, useContext } from "solid-js";
import type { Accessor } from "solid-js";
import type { JSX as OpenTuiJSX } from "@opentui/solid";
import type { GitHubNetworkStats } from "./lib/concurrency";
import type { StatusMessage, ViewTarget } from "./lib/ui-state";
import type { PRDerivedState, WorktreeInfo } from "./lib/pr-state";
import type {
  CheckRun,
  FileCategorization,
  FullReviewThread,
  IssueComment,
  PR,
  PRDetail,
  Review,
} from "./lib/types";

export type RefreshDisplayState = "queued" | "refreshing" | undefined;

type OpenTuiContext<T> = ReturnType<typeof createContext<T>> &
  ((props: { value: T; children?: OpenTuiJSX.Element }) => OpenTuiJSX.Element);

export interface AppContextValue {
  prData: {
    prs: Accessor<PR[]>;
    loading: Accessor<boolean>;
    error: Accessor<string | undefined>;
    repoSlug: Accessor<string>;
    currentUser: Accessor<string | undefined>;
    selectedPr: Accessor<PRDetail | undefined>;
    tabs: Accessor<string[]>;
    activeTab: Accessor<number>;
  };
  summary: {
    threads: Accessor<FullReviewThread[] | undefined>;
    checks: Accessor<CheckRun[] | undefined>;
    reviews: Accessor<Review[] | undefined>;
    files: Accessor<FileCategorization | undefined>;
    loading: Accessor<boolean>;
    state: Accessor<PRDerivedState | undefined>;
  };
  detail: {
    view: Accessor<ViewTarget>;
    pr: Accessor<PRDetail | undefined>;
    checks: Accessor<CheckRun[] | undefined>;
    threads: Accessor<FullReviewThread[] | undefined>;
    comments: Accessor<IssueComment[]>;
    loading: Accessor<boolean>;
    showResolved: Accessor<boolean>;
    showBotComments: Accessor<boolean>;
    worktree: Accessor<WorktreeInfo | undefined>;
  };
  status: {
    networkStats: Accessor<GitHubNetworkStats | undefined>;
    message: Accessor<StatusMessage | null | undefined>;
  };
  derived: {
    getPRState: (pr: PR) => PRDerivedState;
    getRefreshState: (pr: PR) => RefreshDisplayState;
    worktreeForPr: (pr: PR) => WorktreeInfo | undefined;
  };
  actions: {
    selectPr: (pr: PR) => void;
    changeTab: (index: number) => void;
    refreshSelected: (pr?: PR) => void;
    refreshAll: () => void;
    enterDetail: (pr: PR) => void;
    exitDetail: () => void;
    toggleResolved: () => void;
    toggleBotComments: () => void;
    openInBrowser: (pr: PR) => void;
    openInDevin: (pr: PR) => void;
    openUrl: (url: string) => void;
    refreshDetail: () => void;
    createWorktree: (pr: PR) => void;
  };
}

export const AppCtx = createContext<AppContextValue>() as OpenTuiContext<
  AppContextValue | undefined
>;

export function useAppContext(): AppContextValue {
  let value: AppContextValue | undefined;
  try {
    value = useContext(AppCtx);
  } catch {
    throw new Error("useAppContext must be used within AppCtx");
  }
  if (!value) {
    throw new Error("useAppContext must be used within AppCtx");
  }
  return value;
}
