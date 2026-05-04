import { describe, test, expect } from "bun:test";
import { createBrowserActions } from "../src/lib/browser-actions";
import type { StatusMessage } from "../src/lib/ui-state";
import { makePR } from "./helpers";

interface ExecCall {
  cmd: string;
  args: string[];
}

function harness(options: { execError?: Error; defaultRepoSlug?: string } = {}) {
  const calls: ExecCall[] = [];
  const messages: (StatusMessage | null)[] = [];
  const exec = (cmd: string, args: string[], cb: (err: Error | null) => void): void => {
    calls.push({ cmd, args });
    cb(options.execError ?? null);
  };
  const [actions] = createBrowserActions({
    defaultRepoSlug: options.defaultRepoSlug ?? "owner/fallback",
    setStatusMessage: (msg) => messages.push(msg),
    exec,
  });
  return { actions, calls, messages };
}

describe("createBrowserActions", () => {
  test("openInBrowser opens the GitHub PR URL", () => {
    const { actions, calls } = harness();
    const pr = makePR({ number: 7, repoSlug: "acme/widgets" });
    actions.openInBrowser(pr);
    expect(calls).toEqual([{ cmd: "open", args: ["https://github.com/acme/widgets/pull/7"] }]);
  });

  test("openInBrowser falls back to defaultRepoSlug when pr.repoSlug is missing", () => {
    const { actions, calls } = harness({ defaultRepoSlug: "owner/fallback" });
    const pr = makePR({ number: 3 });
    pr.repoSlug = undefined;
    actions.openInBrowser(pr);
    expect(calls).toEqual([{ cmd: "open", args: ["https://github.com/owner/fallback/pull/3"] }]);
  });

  test("openInBrowser routes exec failures through setStatusMessage", () => {
    const { actions, messages } = harness({ execError: new Error("boom") });
    actions.openInBrowser(makePR({ number: 1, repoSlug: "a/b" }));
    expect(messages).toEqual([{ text: "Failed to open browser: boom", kind: "error" }]);
  });

  test("openInDevin opens the Devin review URL and labels failures as Devin", () => {
    const successHarness = harness();
    successHarness.actions.openInDevin(makePR({ number: 9, repoSlug: "acme/widgets" }));
    expect(successHarness.calls).toEqual([
      {
        cmd: "open",
        args: ["https://app.devin.ai/review/acme/widgets/pull/9"],
      },
    ]);

    const failHarness = harness({ execError: new Error("nope") });
    failHarness.actions.openInDevin(makePR({ number: 9, repoSlug: "acme/widgets" }));
    expect(failHarness.messages).toEqual([{ text: "Failed to open Devin: nope", kind: "error" }]);
  });

  test("openUrl opens an arbitrary URL via the open command", () => {
    const { actions, calls } = harness();
    actions.openUrl("https://example.com/path?x=1");
    expect(calls).toEqual([{ cmd: "open", args: ["https://example.com/path?x=1"] }]);
  });
});
