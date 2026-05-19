/**
 * Browser-open side effects for the TUI. All execution failures route
 * through the shared status-message channel from `createUIState` rather
 * than a dedicated error signal.
 */

import { execFile as nodeExecFile } from "child_process";
import type { PR } from "./types";
import type { StatusMessage } from "./ui-state";

export interface BrowserActions {
  openInBrowser(pr: PR): void;
  openInDevin(pr: PR): void;
  openUrl(url: string): void;
}

export type ExecOpen = (cmd: string, args: string[], cb: (err: Error | null) => void) => void;

export interface BrowserActionsDeps {
  /** Repo slug used when a PR's `repoSlug` is undefined. */
  defaultRepoSlug: string;
  setStatusMessage: (msg: StatusMessage | null) => void;
  /** Override for the underlying exec call. Defaults to `child_process.execFile`. */
  exec?: ExecOpen;
}

/** Build a GitHub PR URL from a repo slug and PR number. */
export function prUrl(repoSlug: string, number: number): string {
  return `https://github.com/${repoSlug}/pull/${number}`;
}

/** Build a Devin review URL from a repo slug and PR number. */
export function devinUrl(repoSlug: string, number: number): string {
  const [owner, repo] = repoSlug.split("/");
  return `https://app.devin.ai/review/${owner}/${repo}/pull/${number}`;
}

export function createBrowserActions(deps: BrowserActionsDeps): readonly [BrowserActions] {
  const { defaultRepoSlug, setStatusMessage } = deps;
  const exec: ExecOpen =
    deps.exec ??
    ((cmd, args, cb) => {
      nodeExecFile(cmd, args, cb);
    });

  function reportFailure(label: string, err: Error): void {
    setStatusMessage({
      text: `Failed to open ${label}: ${err.message}`,
      kind: "error",
    });
  }

  function detectOpenCommand(): readonly [cmd: string, baseArgs: string[]] {
    switch (process.platform) {
      case "darwin":
        return ["open", []];
      case "win32":
        // Use `start` via cmd.exe so we respect the default browser/file handler.
        return ["cmd", ["/c", "start", ""]];
      default:
        // Match lazygit's behaviour on Linux and other Unix-y platforms.
        return ["xdg-open", []];
    }
  }

  const [openCmd, openBaseArgs] = detectOpenCommand();

  function open(label: string, url: string): void {
    exec(openCmd, [...openBaseArgs, url], (err) => {
      if (err) reportFailure(label, err);
    });
  }

  const actions: BrowserActions = {
    openInBrowser(pr: PR) {
      open("browser", prUrl(pr.repoSlug ?? defaultRepoSlug, pr.number));
    },
    openInDevin(pr: PR) {
      open("Devin", devinUrl(pr.repoSlug ?? defaultRepoSlug, pr.number));
    },
    openUrl(url: string) {
      open("browser", url);
    },
  };

  return [actions] as const;
}
