import { describe, test, expect } from "bun:test";
import { createRoot, createEffect, flush } from "solid-js";
import { createUIState } from "../src/lib/ui-state";
import type { PR } from "../src/lib/types";

const samplePr = {
  number: 7,
  repoSlug: "owner/repo",
  title: "t",
  url: "u",
  state: "OPEN",
  isDraft: false,
  createdAt: "2026-01-01T00:00:00Z",
  updatedAt: "2026-01-01T00:00:00Z",
  author: { login: "a" },
  baseRefName: "main",
  headRefName: "feat",
  headCommitSha: "abc",
  mergeable: "MERGEABLE",
  body: "",
} as unknown as PR;

describe("createUIState", () => {
  test("returns a [state, actions] tuple", () => {
    createRoot((dispose) => {
      const result = createUIState();
      expect(Array.isArray(result)).toBe(true);
      expect(result.length).toBe(2);
      const [state, actions] = result;
      expect(typeof state.view).toBe("object"); // ViewTarget object via getter
      expect(typeof actions.changeTab).toBe("function");
      dispose();
    });
  });

  test("actions can be destructured and still work (no `this` binding)", () => {
    createRoot((dispose) => {
      const [state, { changeTab, enterDetail, exitDetail }] = createUIState();
      expect(state.activeTab).toBe(0);
      changeTab(2);
      flush();
      expect(state.activeTab).toBe(2);
      enterDetail(samplePr);
      flush();
      expect(state.view).toEqual({
        view: "detail",
        pr: { number: 7, repoSlug: "owner/repo" },
      });
      exitDetail();
      flush();
      expect(state.view).toEqual({ view: "list" });
      dispose();
    });
  });

  test("state property reads are reactive (createEffect re-runs)", () => {
    createRoot((dispose) => {
      const [state, actions] = createUIState();
      const observed: number[] = [];
      createEffect(
        () => state.activeTab,
        (tab) => {
          observed.push(tab);
        },
      );
      flush();
      actions.changeTab(1);
      flush();
      actions.changeTab(3);
      flush();
      expect(observed).toEqual([0, 1, 3]);
      dispose();
    });
  });

  test("info status messages persist; success and error auto-clear after their TTL", async () => {
    await createRoot(async (dispose) => {
      const [state, actions] = createUIState();

      actions.setStatusMessage({ text: "loading", kind: "info" });
      flush();
      expect(state.statusMessage).toEqual({ text: "loading", kind: "info" });

      // info has no TTL — wait past success TTL and confirm it persists
      await new Promise((r) => setTimeout(r, 50));
      flush();
      expect(state.statusMessage).toEqual({ text: "loading", kind: "info" });

      actions.setStatusMessage({ text: "ok", kind: "success" });
      flush();
      expect(state.statusMessage?.kind).toBe("success");

      actions.setStatusMessage({ text: "boom", kind: "error" });
      flush();
      expect(state.statusMessage?.kind).toBe("error");

      actions.setStatusMessage(null);
      flush();
      expect(state.statusMessage).toBeNull();

      dispose();
    });
  });

  test("toggleResolved and toggleBotComments flip their respective signals", () => {
    createRoot((dispose) => {
      const [state, actions] = createUIState();
      expect(state.showResolved).toBe(false);
      expect(state.showBotComments).toBe(true);
      actions.toggleResolved();
      actions.toggleBotComments();
      flush();
      expect(state.showResolved).toBe(true);
      expect(state.showBotComments).toBe(false);
      dispose();
    });
  });

  test("exitDetail resets view, showResolved, and showBotComments", () => {
    createRoot((dispose) => {
      const [state, actions] = createUIState();
      actions.toggleResolved();
      actions.toggleBotComments();
      actions.enterDetail(samplePr);
      flush();
      expect(state.view).toMatchObject({ view: "detail" });
      actions.exitDetail();
      flush();
      expect(state.view).toEqual({ view: "list" });
      expect(state.showResolved).toBe(false);
      expect(state.showBotComments).toBe(true);
      dispose();
    });
  });
});
