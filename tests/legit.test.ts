import { describe, test, expect, afterAll } from "bun:test";
import { Legit, type AuthExecutor, parseRemoteUrl } from "../src/lib/legit";
import type { PR } from "../src/lib/types";
import {
  cleanupTmpDirs,
  makeTmpGitRepo,
  tmpConfigPath,
  mockAuthExec,
  makeSampleRestPR,
  createTestLegit,
  createMockFetch,
  makeGraphQLResponse,
  SAMPLE_GQL_META,
} from "./helpers";
import { mkdtempSync } from "fs";
import { join } from "path";
import { tmpdir } from "os";

afterAll(cleanupTmpDirs);

// ── Repo detection ──────────────────────────────────────────────────────────

describe("Legit.repo", () => {
  test("detects owner/repo from SSH remote", () => {
    const dir = makeTmpGitRepo("git@github.com:acme/widgets.git");
    const app = new Legit({ cwd: dir });
    expect(app.repo).toEqual({ owner: "acme", repo: "widgets" });
  });

  test("detects owner/repo from HTTPS remote", () => {
    const dir = makeTmpGitRepo("https://github.com/acme/widgets.git");
    const app = new Legit({ cwd: dir });
    expect(app.repo).toEqual({ owner: "acme", repo: "widgets" });
  });

  test("detects owner/repo from HTTPS remote without .git suffix", () => {
    const dir = makeTmpGitRepo("https://github.com/acme/widgets");
    const app = new Legit({ cwd: dir });
    expect(app.repo).toEqual({ owner: "acme", repo: "widgets" });
  });

  test("throws when git repo has no remote", () => {
    const dir = makeTmpGitRepo();
    const app = new Legit({ cwd: dir });
    expect(() => app.repo).toThrow(/No git remote/);
  });

  test("throws when directory is not a git repo", () => {
    const dir = mkdtempSync(join(tmpdir(), "legit-test-"));
    const app = new Legit({ cwd: dir });
    expect(() => app.repo).toThrow();
  });

  test("throws when directory does not exist", () => {
    const app = new Legit({ cwd: "/nonexistent/path" });
    expect(() => app.repo).toThrow();
  });

  test("defaults to process.cwd() when no cwd provided", () => {
    const app = new Legit();
    expect(app.repo).toEqual({ owner: "mayfieldiv", repo: "legit" });
  });
});

describe("parseRemoteUrl", () => {
  test("parses SSH URL with .git suffix", () => {
    expect(parseRemoteUrl("git@github.com:owner/repo.git")).toEqual({
      owner: "owner",
      repo: "repo",
    });
  });

  test("parses SSH URL without .git suffix", () => {
    expect(parseRemoteUrl("git@github.com:owner/repo")).toEqual({
      owner: "owner",
      repo: "repo",
    });
  });

  test("parses HTTPS URL with .git suffix", () => {
    expect(parseRemoteUrl("https://github.com/owner/repo.git")).toEqual({
      owner: "owner",
      repo: "repo",
    });
  });

  test("parses HTTPS URL without .git suffix", () => {
    expect(parseRemoteUrl("https://github.com/owner/repo")).toEqual({
      owner: "owner",
      repo: "repo",
    });
  });

  test("parses SSH URL with dots in repo name", () => {
    expect(parseRemoteUrl("git@github.com:angular/angular.js.git")).toEqual({
      owner: "angular",
      repo: "angular.js",
    });
  });

  test("parses SSH URL with dots in repo name without .git suffix", () => {
    expect(parseRemoteUrl("git@github.com:socketio/socket.io")).toEqual({
      owner: "socketio",
      repo: "socket.io",
    });
  });

  test("parses HTTPS URL with dots in repo name", () => {
    expect(parseRemoteUrl("https://github.com/highlightjs/highlight.js.git")).toEqual({
      owner: "highlightjs",
      repo: "highlight.js",
    });
  });

  test("parses HTTPS URL with dots in repo name without .git suffix", () => {
    expect(parseRemoteUrl("https://github.com/kubernetes/kubernetes.io")).toEqual({
      owner: "kubernetes",
      repo: "kubernetes.io",
    });
  });

  test("throws on non-GitHub URL", () => {
    expect(() => parseRemoteUrl("git@gitlab.com:owner/repo.git")).toThrow(/Cannot parse/);
  });

  test("throws on malformed URL", () => {
    expect(() => parseRemoteUrl("not-a-url")).toThrow(/Cannot parse/);
  });
});

// ── Auth resolution ─────────────────────────────────────────────────────────

describe("Legit.auth", () => {
  test("resolves token and user from gh CLI", () => {
    const app = createTestLegit({
      authExec: mockAuthExec({
        "gh auth token": "ghp_abc123",
        "gh api user --jq .login": "mayfieldiv",
      }),
    });
    expect(app.auth).toEqual({
      user: "mayfieldiv",
      token: "ghp_abc123",
      tokenSource: "gh-cli",
    });
  });

  test("throws when gh auth token fails", () => {
    const app = createTestLegit({ authExec: mockAuthExec({}) });
    expect(() => app.auth).toThrow(/Could not resolve GitHub token/);
  });

  test("throws when gh api user fails", () => {
    const app = createTestLegit({
      authExec: mockAuthExec({ "gh auth token": "ghp_abc123" }),
    });
    expect(() => app.auth).toThrow(/Could not determine GitHub username/);
  });

  test("trims whitespace from token and user", () => {
    const app = createTestLegit({
      authExec: mockAuthExec({
        "gh auth token": "  ghp_abc123\n",
        "gh api user --jq .login": "  mayfieldiv\n",
      }),
    });
    expect(app.auth.token).toBe("ghp_abc123");
    expect(app.auth.user).toBe("mayfieldiv");
  });

  test("accessing repo does not trigger auth resolution", () => {
    let authCalled = false;
    const authExec: AuthExecutor = () => {
      authCalled = true;
      return "fake\n";
    };
    const app = createTestLegit({ authExec });
    const _repo = app.repo;
    expect(authCalled).toBe(false);
  });

  test("auth is cached — second access returns same value", () => {
    let callCount = 0;
    const authExec: AuthExecutor = (cmd, args) => {
      callCount++;
      const key = [cmd, ...args].join(" ");
      if (key === "gh auth token") return "ghp_fake\n";
      if (key === "gh api user --jq .login") return "testuser\n";
      throw new Error(`Unexpected: ${key}`);
    };
    const app = createTestLegit({ authExec });
    const a1 = app.auth;
    const a2 = app.auth;
    expect(a1).toBe(a2);
    expect(callCount).toBe(2); // token + user, called once each
  });
});

// ── Config ──────────────────────────────────────────────────────────────────

describe("Legit.config", () => {
  test("loads and auto-saves user from auth", () => {
    const configPath = tmpConfigPath();
    const app = createTestLegit({ configPath });
    const config = app.config;
    expect(config.user).toBe("testuser");

    const { readFileSync } = require("fs");
    const saved = JSON.parse(readFileSync(configPath, "utf-8"));
    expect(saved.user).toBe("testuser");
  });
});

// ── PR fetching ─────────────────────────────────────────────────────────────

describe("Legit.fetchPRs", () => {
  test("returns PR data end-to-end", async () => {
    const app = createTestLegit();
    let prs: PR[] = [];
    for await (const snapshot of app.fetchPRs()) {
      prs = snapshot;
    }
    expect(prs).toHaveLength(1);
    expect(prs[0]!.number).toBe(42);
    expect(prs[0]!.title).toBe("PR #42");
    expect(prs[0]!.additions).toBe(50);
    expect(prs[0]!.reviewDecision).toBe("APPROVED");
  });

  test("auto-adds detected repo to config", async () => {
    const configPath = tmpConfigPath();
    const app = createTestLegit({ configPath });
    for await (const _snapshot of app.fetchPRs()) {
      // consume the iterable
    }

    const { readFileSync } = require("fs");
    const saved = JSON.parse(readFileSync(configPath, "utf-8"));
    expect(saved.repos).toContainEqual({ slug: "acme/widgets" });
  });

  test("with explicit repo overrides detected repo", async () => {
    const { fetch, calls } = createMockFetch([
      { url: /\/pulls/, response: { status: 200, body: [] } },
    ]);
    const app = createTestLegit({ httpFetch: fetch });
    for await (const _snapshot of app.fetchPRs("other/repo")) {
      // consume the iterable
    }
    const pullsCall = calls.find((c) => c.url.includes("/pulls"));
    expect(pullsCall?.url).toContain("other/repo");
  });
});

describe("Legit.fetchPR", () => {
  test("returns single PR detail", async () => {
    const { fetch } = createMockFetch([
      {
        url: /\/pulls\/99$/,
        response: {
          status: 200,
          body: { ...makeSampleRestPR(99), body: "## Fix\n\nDoes the thing." },
        },
      },
      {
        url: /\/graphql/,
        method: "POST",
        response: {
          status: 200,
          body: makeGraphQLResponse([{ ...SAMPLE_GQL_META, number: 99 }]),
        },
      },
    ]);
    const app = createTestLegit({ httpFetch: fetch });
    const pr = await app.fetchPR("acme/widgets", 99);
    expect(pr.number).toBe(99);
    expect(pr.body).toBe("## Fix\n\nDoes the thing.");
  });
});

describe("Legit.repoSlug", () => {
  test("returns owner/repo string", () => {
    const app = createTestLegit();
    expect(app.repoSlug).toBe("acme/widgets");
  });
});

describe("Legit worktree path resolution", () => {
  test("resolveWorktreePath defaults to ~/.legit/worktrees/<slug>/<n>-<branch>", () => {
    const app = createTestLegit();
    // Seed the tracked repo so repoConfig returns something
    app.reloadConfig();
    const p = app.resolveWorktreePath("acme/widgets", 1234, "feature/foo");
    expect(p).toBe(`${process.env.HOME}/.legit/worktrees/acme/widgets/1234-feature-foo`);
  });

  test("per-repo worktreeRoot overrides global default", () => {
    const configPath = tmpConfigPath();
    const { saveConfig } = require("../src/lib/config");
    saveConfig(configPath, {
      ...require("../src/lib/config").DEFAULT_CONFIG,
      repos: [{ slug: "acme/widgets", worktreeRoot: "/wts/widgets" }],
    });
    const app = createTestLegit({ configPath });
    const p = app.resolveWorktreePath("acme/widgets", 7, "main");
    expect(p).toBe("/wts/widgets/7-main");
  });

  test("global worktreeRoot joins <slug> under the root", () => {
    const configPath = tmpConfigPath();
    const { saveConfig } = require("../src/lib/config");
    saveConfig(configPath, {
      ...require("../src/lib/config").DEFAULT_CONFIG,
      worktreeRoot: "/srv/wts",
      repos: [{ slug: "acme/widgets" }],
    });
    const app = createTestLegit({ configPath });
    expect(app.resolveWorktreePath("acme/widgets", 7, "main")).toBe("/srv/wts/acme/widgets/7-main");
  });

  test("resolveSourceClone returns absolute path or undefined", () => {
    const configPath = tmpConfigPath();
    const { saveConfig } = require("../src/lib/config");
    saveConfig(configPath, {
      ...require("../src/lib/config").DEFAULT_CONFIG,
      repos: [{ slug: "acme/widgets", sourceClone: "~/src/widgets" }, { slug: "acme/gadgets" }],
    });
    const app = createTestLegit({ configPath });
    expect(app.resolveSourceClone("acme/widgets")).toBe(`${process.env.HOME}/src/widgets`);
    expect(app.resolveSourceClone("acme/gadgets")).toBeUndefined();
    expect(app.resolveSourceClone("acme/unknown")).toBeUndefined();
  });
});
