/**
 * Pure navigation/UI state — replaces the data orchestration in pr-store.ts.
 * All data fetching is now handled by TanStack Query; this module manages
 * only view state, tab selection, and detail view toggles.
 */

import { createSignal, type Accessor } from "solid-js";
import type { PR } from "./types";

export type ViewTarget = { view: "list" } | { view: "detail"; pr: PR };

/**
 * A transient message shown in the status bar. `info` has no auto-expire (it
 * represents an in-flight operation that will be replaced by a success/error);
 * `success` auto-expires after 4s; `error` auto-expires after 8s.
 */
export interface StatusMessage {
  text: string;
  kind: "info" | "success" | "error";
}

export interface UIState {
  readonly view: Accessor<ViewTarget>;
  readonly activeTab: Accessor<number>;
  readonly showResolved: Accessor<boolean>;
  readonly showBotComments: Accessor<boolean>;
  readonly statusMessage: Accessor<StatusMessage | null>;

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

export function createUIState(): UIState {
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

  function changeTab(index: number) {
    setActiveTab(index);
  }

  function enterDetail(pr: PR) {
    setView({ view: "detail", pr });
  }

  function exitDetail() {
    setView({ view: "list" });
    setShowResolved(false);
    setShowBotComments(true);
  }

  function toggleResolved() {
    setShowResolved((v) => !v);
  }

  function toggleBotComments() {
    setShowBotComments((v) => !v);
  }

  function setStatusMessage(message: StatusMessage | null): void {
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
  }

  return {
    view,
    activeTab,
    showResolved,
    showBotComments,
    statusMessage,
    changeTab,
    enterDetail,
    exitDetail,
    toggleResolved,
    toggleBotComments,
    setStatusMessage,
  };
}
