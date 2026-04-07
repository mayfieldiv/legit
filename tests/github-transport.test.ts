import { describe, test, expect } from "bun:test";
import { createGitHubTransport } from "../src/lib/github-transport";
import {
  createMockFetch,
  SAMPLE_REST_PR,
  SAMPLE_GQL_META,
  makeGraphQLResponse,
  makeSampleFile,
} from "./helpers";

/** Collect all items from an async iterable into an array. */
async function collect<T>(iter: AsyncIterable<T>): Promise<T[]> {
  const items: T[] = [];
  for await (const item of iter) items.push(item);
  return items;
}

const SAMPLE_THREAD = {
  id: "RT_abc",
  isResolved: false,
  path: "src/foo.ts",
  line: 42,
  comments: {
    nodes: [
      {
        id: "RC_1",
        author: { login: "bob", __typename: "User" },
        body: "Needs a null check",
        createdAt: "2026-03-10T00:00:00Z",
        url: "https://github.com/acme/widgets/pull/42#discussion_r1",
      },
    ],
  },
};

const SAMPLE_ISSUE_COMMENT = {
  id: 100,
  user: { login: "bob", type: "User" },
  body: "Looks good overall",
  created_at: "2026-03-10T00:00:00Z",
  html_url: "https://github.com/acme/widgets/pull/42#issuecomment-100",
};

function makeThreadsResponse(
  threads: unknown[],
  hasNextPage = false,
  endCursor: string | null = null,
) {
  return {
    data: {
      repository: {
        pullRequest: {
          reviewThreads: {
            pageInfo: { hasNextPage, endCursor },
            nodes: threads,
          },
        },
      },
    },
  };
}

describe("GitHubTransport", () => {
  describe("listOpenPRs", () => {
    test("yields individual PRs from a single page", async () => {
      const { fetch } = createMockFetch([
        {
          url: "https://api.github.com/repos/acme/widgets/pulls?state=open&per_page=100&page=1",
          response: { status: 200, body: [SAMPLE_REST_PR] },
        },
      ]);
      const transport = createGitHubTransport("fake-token", fetch);
      const prs = await collect(transport.listOpenPRs("acme", "widgets"));
      expect(prs).toHaveLength(1);
      expect(prs[0]!.number).toBe(42);
    });

    test("paginates and yields items from multiple pages", async () => {
      const page1 = Array.from({ length: 100 }, (_, i) => ({
        ...SAMPLE_REST_PR,
        number: i + 1,
      }));
      const page2 = [{ ...SAMPLE_REST_PR, number: 101 }];

      const { fetch } = createMockFetch([
        {
          url: "https://api.github.com/repos/acme/widgets/pulls?state=open&per_page=100&page=1",
          response: { status: 200, body: page1 },
        },
        {
          url: "https://api.github.com/repos/acme/widgets/pulls?state=open&per_page=100&page=2",
          response: { status: 200, body: page2 },
        },
      ]);
      const transport = createGitHubTransport("fake-token", fetch);
      const prs = await collect(transport.listOpenPRs("acme", "widgets"));
      expect(prs).toHaveLength(101);
    });

    test("sends correct Authorization header", async () => {
      const { fetch, calls } = createMockFetch([
        { url: /pulls/, response: { status: 200, body: [] } },
      ]);
      const transport = createGitHubTransport("my-secret-token", fetch);
      await collect(transport.listOpenPRs("acme", "widgets"));
      expect(calls.length).toBeGreaterThan(0);
      const authHeader = (calls[0]!.init?.headers as Record<string, string>)?.["Authorization"];
      expect(authHeader).toBe("Bearer my-secret-token");
    });

    test("throws on API error", async () => {
      const { fetch } = createMockFetch([
        { url: /pulls/, response: { status: 403, body: { message: "rate limited" } } },
      ]);
      const transport = createGitHubTransport("fake-token", fetch);
      expect(collect(transport.listOpenPRs("acme", "widgets"))).rejects.toThrow(/403/);
    });
  });

  describe("getPR", () => {
    test("fetches a single PR", async () => {
      const { fetch } = createMockFetch([
        {
          url: "https://api.github.com/repos/acme/widgets/pulls/42",
          response: { status: 200, body: { ...SAMPLE_REST_PR, body: "Description" } },
        },
      ]);
      const transport = createGitHubTransport("fake-token", fetch);
      const pr = await transport.getPR("acme", "widgets", 42);
      expect(pr.number).toBe(42);
      expect(pr.body).toBe("Description");
    });

    test("throws on 404", async () => {
      const { fetch } = createMockFetch([
        { url: /pulls\/999/, response: { status: 404, body: { message: "Not Found" } } },
      ]);
      const transport = createGitHubTransport("fake-token", fetch);
      expect(transport.getPR("acme", "widgets", 999)).rejects.toThrow(/404/);
    });
  });

  describe("listPRFiles", () => {
    test("yields individual files", async () => {
      const { fetch } = createMockFetch([
        {
          url: "https://api.github.com/repos/acme/widgets/pulls/42/files?per_page=100&page=1",
          response: {
            status: 200,
            body: [makeSampleFile("src/a.ts"), makeSampleFile("src/b.ts")],
          },
        },
      ]);
      const transport = createGitHubTransport("fake-token", fetch);
      const files = await collect(transport.listPRFiles("acme", "widgets", 42));
      expect(files).toHaveLength(2);
      expect(files[0]!.filename).toBe("src/a.ts");
    });

    test("paginates file list", async () => {
      const page1 = Array.from({ length: 100 }, (_, i) => makeSampleFile(`file${i}.ts`));
      const page2 = [makeSampleFile("file100.ts")];

      const { fetch } = createMockFetch([
        {
          url: "https://api.github.com/repos/acme/widgets/pulls/42/files?per_page=100&page=1",
          response: { status: 200, body: page1 },
        },
        {
          url: "https://api.github.com/repos/acme/widgets/pulls/42/files?per_page=100&page=2",
          response: { status: 200, body: page2 },
        },
      ]);
      const transport = createGitHubTransport("fake-token", fetch);
      const files = await collect(transport.listPRFiles("acme", "widgets", 42));
      expect(files).toHaveLength(101);
    });
  });

  describe("fetchReviewStatus", () => {
    test("yields individual review statuses", async () => {
      const { fetch } = createMockFetch([
        {
          url: "https://api.github.com/graphql",
          method: "POST",
          response: { status: 200, body: makeGraphQLResponse([SAMPLE_GQL_META]) },
        },
      ]);
      const transport = createGitHubTransport("fake-token", fetch);
      const statuses = await collect(transport.fetchReviewStatus("acme", "widgets", [42]));
      expect(statuses).toHaveLength(1);
      expect(statuses[0]!.prNumber).toBe(42);
      expect(statuses[0]!.additions).toBe(50);
    });

    test("batches into groups of 25", async () => {
      const prNumbers = Array.from({ length: 51 }, (_, i) => i + 1);
      const batch1 = prNumbers.slice(0, 25).map((n) => ({ ...SAMPLE_GQL_META, number: n }));
      const batch2 = prNumbers.slice(25, 50).map((n) => ({ ...SAMPLE_GQL_META, number: n }));
      const batch3 = [{ ...SAMPLE_GQL_META, number: 51 }];

      const { fetch, calls } = createMockFetch([
        {
          url: "https://api.github.com/graphql",
          method: "POST",
          response: { status: 200, body: makeGraphQLResponse(batch1) },
        },
        {
          url: "https://api.github.com/graphql",
          method: "POST",
          response: { status: 200, body: makeGraphQLResponse(batch2) },
        },
        {
          url: "https://api.github.com/graphql",
          method: "POST",
          response: { status: 200, body: makeGraphQLResponse(batch3) },
        },
      ]);
      const transport = createGitHubTransport("fake-token", fetch);
      const statuses = await collect(transport.fetchReviewStatus("acme", "widgets", prNumbers));
      expect(statuses).toHaveLength(51);
      const gqlCalls = calls.filter((c) => c.url.includes("graphql"));
      expect(gqlCalls).toHaveLength(3);
    });

    test("skips failed batches gracefully", async () => {
      // Two batches: first succeeds, second returns 502 (after retries)
      const prNumbers = Array.from({ length: 30 }, (_, i) => i + 1);
      const batch1 = prNumbers.slice(0, 25).map((n) => ({ ...SAMPLE_GQL_META, number: n }));

      const { fetch } = createMockFetch([
        {
          url: "https://api.github.com/graphql",
          method: "POST",
          response: { status: 200, body: makeGraphQLResponse(batch1) },
        },
        // 3 attempts for batch 2 (initial + 2 retries), all fail with 502
        {
          url: "https://api.github.com/graphql",
          method: "POST",
          response: { status: 502, body: "Bad Gateway" },
        },
        {
          url: "https://api.github.com/graphql",
          method: "POST",
          response: { status: 502, body: "Bad Gateway" },
        },
        {
          url: "https://api.github.com/graphql",
          method: "POST",
          response: { status: 502, body: "Bad Gateway" },
        },
      ]);
      const transport = createGitHubTransport("fake-token", fetch);
      const statuses = await collect(transport.fetchReviewStatus("acme", "widgets", prNumbers));
      // Only batch 1 (25 PRs) succeeded; batch 2 was skipped
      expect(statuses).toHaveLength(25);
    });

    test("retries 5xx errors and succeeds on retry", async () => {
      const prNumbers = [42];
      const batch1 = [{ ...SAMPLE_GQL_META, number: 42 }];

      const { fetch, calls } = createMockFetch([
        // First attempt: 502
        {
          url: "https://api.github.com/graphql",
          method: "POST",
          response: { status: 502, body: "Bad Gateway" },
        },
        // Second attempt: success
        {
          url: "https://api.github.com/graphql",
          method: "POST",
          response: { status: 200, body: makeGraphQLResponse(batch1) },
        },
      ]);
      const transport = createGitHubTransport("fake-token", fetch);
      const statuses = await collect(transport.fetchReviewStatus("acme", "widgets", prNumbers));
      expect(statuses).toHaveLength(1);
      expect(statuses[0]!.prNumber).toBe(42);
      const gqlCalls = calls.filter((c) => c.url.includes("graphql"));
      expect(gqlCalls).toHaveLength(2);
    });

    test("yields nothing for empty prNumbers", async () => {
      const { fetch } = createMockFetch([]);
      const transport = createGitHubTransport("fake-token", fetch);
      const statuses = await collect(transport.fetchReviewStatus("acme", "widgets", []));
      expect(statuses).toEqual([]);
    });
  });

  describe("fetchFullReviewThreads", () => {
    test("yields full thread data with comments", async () => {
      const { fetch } = createMockFetch([
        {
          url: "https://api.github.com/graphql",
          method: "POST",
          response: { status: 200, body: makeThreadsResponse([SAMPLE_THREAD]) },
        },
      ]);
      const transport = createGitHubTransport("fake-token", fetch);
      const threads = await collect(transport.fetchFullReviewThreads("acme", "widgets", 42));
      expect(threads).toHaveLength(1);
      expect(threads[0]!.id).toBe("RT_abc");
      expect(threads[0]!.isResolved).toBe(false);
      expect(threads[0]!.path).toBe("src/foo.ts");
      expect(threads[0]!.line).toBe(42);
      expect(threads[0]!.comments.nodes).toHaveLength(1);
      expect(threads[0]!.comments.nodes[0]!.body).toBe("Needs a null check");
    });

    test("paginates across multiple pages", async () => {
      const thread1 = { ...SAMPLE_THREAD, id: "RT_1" };
      const thread2 = { ...SAMPLE_THREAD, id: "RT_2" };

      const { fetch } = createMockFetch([
        {
          url: "https://api.github.com/graphql",
          method: "POST",
          response: {
            status: 200,
            body: makeThreadsResponse([thread1], true, "cursor1"),
          },
        },
        {
          url: "https://api.github.com/graphql",
          method: "POST",
          response: { status: 200, body: makeThreadsResponse([thread2]) },
        },
      ]);
      const transport = createGitHubTransport("fake-token", fetch);
      const threads = await collect(transport.fetchFullReviewThreads("acme", "widgets", 42));
      expect(threads).toHaveLength(2);
      expect(threads[0]!.id).toBe("RT_1");
      expect(threads[1]!.id).toBe("RT_2");
    });

    test("yields nothing when PR has no threads", async () => {
      const { fetch } = createMockFetch([
        {
          url: "https://api.github.com/graphql",
          method: "POST",
          response: { status: 200, body: makeThreadsResponse([]) },
        },
      ]);
      const transport = createGitHubTransport("fake-token", fetch);
      const threads = await collect(transport.fetchFullReviewThreads("acme", "widgets", 42));
      expect(threads).toEqual([]);
    });
  });

  describe("listIssueComments", () => {
    test("yields individual comments", async () => {
      const { fetch } = createMockFetch([
        {
          url: "https://api.github.com/repos/acme/widgets/issues/42/comments?per_page=100&page=1",
          response: { status: 200, body: [SAMPLE_ISSUE_COMMENT] },
        },
      ]);
      const transport = createGitHubTransport("fake-token", fetch);
      const comments = await collect(transport.listIssueComments("acme", "widgets", 42));
      expect(comments).toHaveLength(1);
      expect(comments[0]!.id).toBe(100);
      expect(comments[0]!.body).toBe("Looks good overall");
      expect(comments[0]!.user!.login).toBe("bob");
    });

    test("paginates comment list", async () => {
      const page1 = Array.from({ length: 100 }, (_, i) => ({
        ...SAMPLE_ISSUE_COMMENT,
        id: i + 1,
      }));
      const page2 = [{ ...SAMPLE_ISSUE_COMMENT, id: 101 }];

      const { fetch } = createMockFetch([
        {
          url: "https://api.github.com/repos/acme/widgets/issues/42/comments?per_page=100&page=1",
          response: { status: 200, body: page1 },
        },
        {
          url: "https://api.github.com/repos/acme/widgets/issues/42/comments?per_page=100&page=2",
          response: { status: 200, body: page2 },
        },
      ]);
      const transport = createGitHubTransport("fake-token", fetch);
      const comments = await collect(transport.listIssueComments("acme", "widgets", 42));
      expect(comments).toHaveLength(101);
    });

    test("yields nothing for PR with no comments", async () => {
      const { fetch } = createMockFetch([
        {
          url: "https://api.github.com/repos/acme/widgets/issues/42/comments?per_page=100&page=1",
          response: { status: 200, body: [] },
        },
      ]);
      const transport = createGitHubTransport("fake-token", fetch);
      const comments = await collect(transport.listIssueComments("acme", "widgets", 42));
      expect(comments).toEqual([]);
    });
  });
});
