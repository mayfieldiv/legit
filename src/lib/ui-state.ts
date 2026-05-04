/**
 * Pure navigation/UI state — replaces the data orchestration in pr-store.ts.
 * All data fetching is now handled by TanStack Query; this module manages
 * only view state, tab selection, and detail view toggles.
 */

import { createSignal } from "solid-js";
import { prKey, type PRIdentity } from "./pr-identity";
import type { PR } from "./types";

export type ViewTarget = { view: "list" } | { view: "detail"; pr: PRIdentity };

/**
 * A transient message shown in the status bar. `info` has no auto-expire (it
 * represents an in-flight operation that will be replaced by a success/error);
 * `success` auto-expires after 4s; `error` auto-expires after 8s.
 */
export interface StatusMessage {
  text: string;
  kind: "info" | "success" | "error";
}

/**
 * Reactive view-state slice. Every property is a getter, so reads track
 * uniformly inside reactive scopes (no mixing of accessor calls and property
 * reads). Pair this with `UIActions` via the `[state, actions]` tuple
 * returned by `createUIState`.
 */
export interface UIState {
  readonly view: ViewTarget;
  readonly activeTab: number;
  readonly showResolved: boolean;
  readonly showBotComments: boolean;
  readonly statusMessage: StatusMessage | null;
}

export interface UIActions {
  changeTab(index: number): void;
  enterDetail(pr: PR): void;
  exitDetail(): void;
  toggleResolved(): void;
  toggleBotComments(): void;
  /**
   * Post a message to the status bar. `info` messages persist until replaced
   * or cleared; `success` / `error` auto-clear after their respective TTL.
   */
  setStatusMessage(message: StatusMessage | null): void;
}

const AUTO_EXPIRE_MS: Partial<Record<StatusMessage["kind"], number>> = {
  success: 4_000,
  error: 8_000,
};

export function createUIState(): readonly [UIState, UIActions] {
  const [view, setView] = createSignal<ViewTarget>({ view: "list" });
  const [activeTab, setActiveTab] = createSignal(0);
  const [showResolved, setShowResolved] = createSignal(false);
  const [showBotComments, setShowBotComments] = createSignal(true);
  const [statusMessage, setStatusMessageSignal] = createSignal<StatusMessage | null>(null);

  let expireTimer: ReturnType<typeof setTimeout> | undefined;
  const clearTimer = () => {
    if (expireTimer !== undefined) {
      clearTimeout(expireTimer);
      expireTimer = undefined;
    }
  };

  const state: UIState = {
    get view() {
      return view();
    },
    get activeTab() {
      return activeTab();
    },
    get showResolved() {
      return showResolved();
    },
    get showBotComments() {
      return showBotComments();
    },
    get statusMessage() {
      return statusMessage();
    },
  };

  const actions: UIActions = {
    changeTab(index: number) {
      setActiveTab(index);
    },
    enterDetail(pr: PR) {
      setView({ view: "detail", pr: prKey(pr) });
    },
    exitDetail() {
      setView({ view: "list" });
      setShowResolved(false);
      setShowBotComments(true);
    },
    toggleResolved() {
      setShowResolved((v) => !v);
    },
    toggleBotComments() {
      setShowBotComments((v) => !v);
    },
    setStatusMessage(message: StatusMessage | null): void {
      clearTimer();
      setStatusMessageSignal(message);
      if (message) {
        const ttl = AUTO_EXPIRE_MS[message.kind];
        if (ttl !== undefined) {
          expireTimer = setTimeout(() => {
            expireTimer = undefined;
            setStatusMessageSignal(null);
          }, ttl);
        }
      }
    },
  };

  return [state, actions] as const;
}
