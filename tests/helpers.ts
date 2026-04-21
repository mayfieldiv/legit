import { mkdtempSync, rmSync } from "fs";
import { execFileSync } from "child_process";
import { join } from "path";
import { tmpdir } from "os";
import type { PR } from "../src/lib/types";
import type { AuthExecutor, LegitOptions } from "../src/lib/legit";
import { Legit } from "../src/lib/legit";
import type { HttpFetch, GitHubTransport, RawRestPR } from "../src/lib/github-client";

// ── Temp directory management ───────────────────────────────────────────────

const tmpDirs: string[] = [];

export function cleanupTmpDirs(): void {
  for (const dir of tmpDirs) {
    rmSync(dir, { recursive: true, force: true });
  }
  tmpDirs.length = 0;
}

// ── Git repo helpers ────────────────────────────────────────────────────────

export function makeTmpGitRepo(remoteUrl?: string): string {
  const dir = mkdtempSync(join(tmpdir(), "legit-test-"));
  tmpDirs.push(dir);
  execFileSync("git", ["init"], { cwd: dir, stdio: "pipe" });
  if (remoteUrl) {
    execFileSync("git", ["remote", "add", "origin", remoteUrl], {
      cwd: dir,
      stdio: "pipe",
    });
  }
  return dir;
}

export function tmpConfigPath(): string {
  const dir = mkdtempSync(join(tmpdir(), "legit-test-"));
  tmpDirs.push(dir);
  return join(dir, "config.json");
}

// ── Auth mock ───────────────────────────────────────────────────────────────

export function mockAuthExec(
  responses: Record<string, string> = {
    "gh auth token": "ghp_fake123\n",
    "gh api user --jq .login": "testuser\n",
  },
): AuthExecutor {
  return (cmd, args) => {
    const key = [cmd, ...args].join(" ");
    const result = responses[key];
    if (result === undefined) throw new Error(`Command failed: ${key}`);
    return result;
  };
}

// ── HTTP mock (route-based) ─────────────────────────────────────────────────

export interface MockRoute {
  url: string | RegExp;
  method?: string;
  response: { status: number; body: unknown };
}

export interface MockFetch {
  fetch: HttpFetch;
  calls: Array<{ url: string; init?: RequestInit }>;
}

/**
 * Route-based HTTP mock. Routes are matched in order and consumed — each
 * route fires once, so duplicate URL patterns return successive responses.
 * Falls back to 404 if no route matches.
 */
export function createMockFetch(routes: MockRoute[]): MockFetch {
  const remaining = [...routes];
  const calls: Array<{ url: string; init?: RequestInit }> = [];

  const fetch: HttpFetch = async (url, init) => {
    calls.push({ url, init });
    const method = init?.method ?? "GET";

    for (let i = 0; i < remaining.length; i++) {
      const route = remaining[i]!;
      const urlMatch = typeof route.url === "string" ? url === route.url : route.url.test(url);
      const methodMatch = !route.method || route.method === method;

      if (urlMatch && methodMatch) {
        remaining.splice(i, 1);
        return new Response(JSON.stringify(route.response.body), {
          status: route.response.status,
          headers: { "Content-Type": "application/json" },
        });
      }
    }

    return new Response(JSON.stringify({ message: "Not Found" }), {
      status: 404,
    });
  };

  return { fetch, calls };
}

// ── Async iterable utilities ────────────────────────────────────────────────

/** Collect all items from an async iterable into an array. */
export async function collect<T>(iter: AsyncIterable<T>): Promise<T[]> {
  const items: T[] = [];
  for await (const item of iter) items.push(item);
  return items;
}

/** Collect only the last value from an async iterable. */
export async function collectLast<T>(iter: AsyncIterable<T>): Promise<T | undefined> {
  let last: T | undefined;
  for await (const item of iter) last = item;
  return last;
}

// ── Transport mock ──────────────────────────────────────────────────────────

/** Create a mock GitHubTransport with async generators. */
export function createMockTransport(overrides: Partial<GitHubTransport> = {}): GitHubTransport {
  return {
    async *listOpenPRs() {},
    async getPR() {
      return SAMPLE_REST_PR satisfies RawRestPR;
    },
    async *listPRFiles() {},
    async *fetchReviewStatus() {},
    async *listCheckRuns() {},
    async *listReviews() {},
    async *fetchFullReviewThreads() {},
    async *listIssueComments() {},
    ...overrides,
  };
}

/**
 * Convenience mock: returns the given REST PRs from transport.listOpenPRs,
 * and matching GraphQL metadata from transport.fetchReviewStatus.
 */
export function mockTransport(restPRs: RawRestPR[] = []): GitHubTransport {
  return createMockTransport({
    async *listOpenPRs() {
      for (const pr of restPRs) yield pr;
    },
    async *fetchReviewStatus(_owner, _repo, prNumbers) {
      for (const n of prNumbers) {
        yield {
          prNumber: n,
          additions: SAMPLE_GQL_META.additions,
          deletions: SAMPLE_GQL_META.deletions,
          reviewDecision: SAMPLE_GQL_META.reviewDecision,
          mergeable: SAMPLE_GQL_META.mergeable,
          commits: SAMPLE_GQL_META.commits,
        };
      }
    },
  });
}

// ── Sample GitHub API data ──────────────────────────────────────────────────

/** Sample REST PR object as returned by the GitHub REST API. */
export const SAMPLE_REST_PR = {
  number: 42,
  title: "Fix the thing",
  user: { login: "alice", type: "User" },
  created_at: "2026-03-01T00:00:00Z",
  updated_at: "2026-03-15T00:00:00Z",
  draft: false,
  labels: [{ name: "bug" }],
  requested_reviewers: [{ login: "bob" }],
  assignees: [{ login: "alice" }],
};

/** Sample GraphQL PR metadata. */
export const SAMPLE_GQL_META = {
  number: 42,
  additions: 50,
  deletions: 10,
  reviewDecision: "APPROVED",
  mergeable: "MERGEABLE",
  commits: {
    nodes: [{ commit: { committedDate: "2026-03-14T00:00:00Z", oid: "abc123def456" } }],
  },
};

/** Build a sample GraphQL response for a set of PR metadata objects. */
export function makeGraphQLResponse(prMetas: Array<{ number: number } & Record<string, unknown>>) {
  const repository: Record<string, unknown> = {};
  prMetas.forEach((meta, i) => {
    repository[`pr${i}`] = meta;
  });
  return { data: { repository } };
}

/** Create a minimal REST PR with a given number. */
export function makeSampleRestPR(n: number) {
  return {
    ...SAMPLE_REST_PR,
    number: n,
    title: `PR #${n}`,
    labels: [],
    requested_reviewers: [],
    assignees: [],
  };
}

// ── Convenience: simple mock that returns a list of PRs ─────────────────────

/**
 * Convenience mock: returns the given REST PRs from the list endpoint,
 * and matching GraphQL metadata from the graphql endpoint.
 */
export function mockHttpFetch(restPRs: any[] = []): HttpFetch {
  const { fetch } = createMockFetch([
    {
      url: /\/pulls\?/,
      response: { status: 200, body: restPRs },
    },
    {
      url: /\/graphql/,
      method: "POST",
      response: {
        status: 200,
        body: makeGraphQLResponse(
          restPRs.map((pr: any) => ({
            ...SAMPLE_GQL_META,
            number: pr.number,
          })),
        ),
      },
    },
  ]);
  return fetch;
}

// ── PR factory (domain type) ────────────────────────────────────────────────

/** Create a test Legit instance with all external dependencies mocked. */
export function createTestLegit(overrides?: Partial<LegitOptions>): Legit {
  return new Legit({
    cwd: makeTmpGitRepo("git@github.com:acme/widgets.git"),
    configPath: tmpConfigPath(),
    authExec: mockAuthExec(),
    httpFetch: mockHttpFetch([makeSampleRestPR(42)]),
    ...overrides,
  });
}

/** Sample file object as returned by GitHub's PR files endpoint. */
export const SAMPLE_FILE = {
  filename: "src/lib/foo.ts",
  additions: 25,
  deletions: 5,
  changes: 30,
  status: "modified",
};

/** Create a sample file response with a given filename. */
export function makeSampleFile(filename: string, additions = 10, deletions = 3) {
  return { ...SAMPLE_FILE, filename, additions, deletions, changes: additions + deletions };
}

/** Create a fully-populated domain PR with sensible defaults. Override any field. */
export function makePR(overrides: Partial<PR> = {}): PR {
  return {
    number: 42,
    title: "Fix the thing",
    author: "alice",
    createdAt: "2026-03-01T00:00:00Z",
    updatedAt: "2026-03-15T00:00:00Z",
    additions: 50,
    deletions: 10,
    isDraft: false,
    labels: [],
    requestedReviewers: [],
    assignees: [],
    reviewDecision: "",
    mergeable: "MERGEABLE",
    lastCommitDate: "2026-03-14T00:00:00Z",
    headCommitSha: "abc123def456",
    headRef: "my-branch",
    baseRef: "main",
    headRepositoryOwner: "acme",
    state: "OPEN",
    ...overrides,
  };
}
