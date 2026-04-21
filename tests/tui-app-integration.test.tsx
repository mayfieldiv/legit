import { describe, test, expect, afterAll, afterEach } from "bun:test";
import { testRender } from "@opentui/solid";
import type { CliRenderer } from "@opentui/core";
import { App, prUrl } from "../src/App";
import {
  cleanupTmpDirs,
  makeSampleRestPR,
  mockHttpFetch,
  createTestLegit,
  createMockFetch,
  makeGraphQLResponse,
  SAMPLE_GQL_META,
} from "./helpers";

// Destroy the renderer after each test to dispose the Solid root and
// unsubscribe TanStack Query observers. Without this, observers from
// previous tests accumulate and exhaust memory.
let activeRenderer: CliRenderer | undefined;

afterAll(() => {
  activeRenderer?.destroy();
  activeRenderer = undefined;
  cleanupTmpDirs();
});

async function testRenderTracked(
  ...args: Parameters<typeof testRender>
): ReturnType<typeof testRender> {
  // Destroy any renderer left over from a previous test (safety net).
  activeRenderer?.destroy();
  const result = await testRender(...args);
  activeRenderer = result.renderer;
  return result;
}

describe("App integration", () => {
  afterEach(async () => {
    activeRenderer?.destroy();
    activeRenderer = undefined;
    // Drain pending microtasks/timers so observer cleanup completes
    // before the next test starts a new reactive graph.
    await new Promise((r) => setTimeout(r, 10));
  });
  test("renders loading state then PR list after fetch", async () => {
    const app = createTestLegit({
      httpFetch: mockHttpFetch([makeSampleRestPR(1), makeSampleRestPR(2)]),
    });

    const { renderOnce, captureCharFrame } = await testRenderTracked(() => <App app={app} />, {
      width: 160,
      height: 20,
    });

    // First render — resource is pending
    await renderOnce();
    const _loadingFrame = captureCharFrame();
    // May show loading or may have already resolved (microtask)
    // Either way, rendering should not throw

    // Give the resource time to resolve
    await new Promise((r) => setTimeout(r, 50));
    await renderOnce();

    const frame = captureCharFrame();
    expect(frame).toContain("acme/widgets");
    expect(frame).toContain("PR #1");
    expect(frame).toContain("PR #2");
  });

  test("shows repo slug in header", async () => {
    const app = createTestLegit();

    const { renderOnce, captureCharFrame } = await testRenderTracked(() => <App app={app} />, {
      width: 160,
      height: 20,
    });

    await new Promise((r) => setTimeout(r, 50));
    await renderOnce();

    const frame = captureCharFrame();
    expect(frame).toContain("acme/widgets");
  });

  test("shows loading progress text while fetching", async () => {
    const { fetch } = createMockFetch([
      {
        url: /pulls/,
        response: { status: 200, body: [] },
      },
    ]);
    const delayedFetch = async (url: string, init?: RequestInit) => {
      await new Promise((r) => setTimeout(r, 25));
      return fetch(url, init);
    };
    const app = createTestLegit({ httpFetch: delayedFetch });

    const { renderOnce, captureCharFrame } = await testRenderTracked(() => <App app={app} />, {
      width: 160,
      height: 20,
    });

    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toContain("Loading pull requests");
  });

  test("shows error when fetch fails", async () => {
    const { fetch } = createMockFetch([
      {
        url: /pulls/,
        response: { status: 500, body: { message: "Server error" } },
      },
      // TanStack Query retries once, so provide a second matching route
      {
        url: /pulls/,
        response: { status: 500, body: { message: "Server error" } },
      },
    ]);

    const app = createTestLegit({ httpFetch: fetch });

    const { renderOnce, captureCharFrame } = await testRenderTracked(() => <App app={app} />, {
      width: 160,
      height: 20,
    });

    // Wait for error to propagate (TanStack Query retries once, needs extra time)
    await new Promise((r) => setTimeout(r, 2000));
    await renderOnce();

    const frame = captureCharFrame();
    expect(frame).toContain("500");
  });

  test("R key refreshes only the current repo tab", async () => {
    const calls: string[] = [];
    const fetch = async (url: string, init?: RequestInit) => {
      calls.push(url);
      if (/\/repos\/acme\/widgets\/pulls\?/.test(url)) {
        return new Response(JSON.stringify([makeSampleRestPR(1)]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (/\/repos\/acme\/gadgets\/pulls\?/.test(url)) {
        return new Response(JSON.stringify([makeSampleRestPR(2)]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.endsWith("/graphql")) {
        const body = JSON.parse(String(init?.body ?? "{}")) as { query?: string };
        const query = body.query ?? "";
        if (query.includes("number: 2")) {
          return new Response(
            JSON.stringify(makeGraphQLResponse([{ ...SAMPLE_GQL_META, number: 2 }])),
            { status: 200, headers: { "Content-Type": "application/json" } },
          );
        }
        return new Response(
          JSON.stringify(makeGraphQLResponse([{ ...SAMPLE_GQL_META, number: 1 }])),
          { status: 200, headers: { "Content-Type": "application/json" } },
        );
      }
      return new Response(JSON.stringify({ message: "Not Found" }), {
        status: 404,
        headers: { "Content-Type": "application/json" },
      });
    };

    const app = createTestLegit({ httpFetch: fetch });
    app.config.repos = [{ slug: "acme/widgets" }, { slug: "acme/gadgets" }];

    const { renderOnce, mockInput } = await testRenderTracked(() => <App app={app} />, {
      width: 160,
      height: 20,
    });

    await new Promise((r) => setTimeout(r, 120));
    await renderOnce();

    const initialWidgets = calls.filter((url) => url.includes("/repos/acme/widgets/pulls")).length;
    const initialGadgets = calls.filter((url) => url.includes("/repos/acme/gadgets/pulls")).length;

    mockInput.pressKey("1");
    await new Promise((r) => setTimeout(r, 20));
    await renderOnce();

    mockInput.pressKey("r", { shift: true });
    await new Promise((r) => setTimeout(r, 120));
    await renderOnce();

    const nextWidgets = calls.filter((url) => url.includes("/repos/acme/widgets/pulls")).length;
    const nextGadgets = calls.filter((url) => url.includes("/repos/acme/gadgets/pulls")).length;
    expect(nextWidgets).toBeGreaterThan(initialWidgets);
    expect(nextGadgets).toBe(initialGadgets);
  });

  test("R key refreshes every tracked repo from the All tab", async () => {
    const calls: string[] = [];
    const fetch = async (url: string, init?: RequestInit) => {
      calls.push(url);
      if (/\/repos\/acme\/widgets\/pulls\?/.test(url)) {
        return new Response(JSON.stringify([makeSampleRestPR(1)]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (/\/repos\/acme\/gadgets\/pulls\?/.test(url)) {
        return new Response(JSON.stringify([makeSampleRestPR(2)]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.endsWith("/graphql")) {
        const body = JSON.parse(String(init?.body ?? "{}")) as { query?: string };
        const query = body.query ?? "";
        if (query.includes("number: 2")) {
          return new Response(
            JSON.stringify(makeGraphQLResponse([{ ...SAMPLE_GQL_META, number: 2 }])),
            { status: 200, headers: { "Content-Type": "application/json" } },
          );
        }
        return new Response(
          JSON.stringify(makeGraphQLResponse([{ ...SAMPLE_GQL_META, number: 1 }])),
          { status: 200, headers: { "Content-Type": "application/json" } },
        );
      }
      return new Response(JSON.stringify({ message: "Not Found" }), {
        status: 404,
        headers: { "Content-Type": "application/json" },
      });
    };

    const app = createTestLegit({ httpFetch: fetch });
    app.config.repos = [{ slug: "acme/widgets" }, { slug: "acme/gadgets" }];

    const { renderOnce, mockInput } = await testRenderTracked(() => <App app={app} />, {
      width: 160,
      height: 20,
    });

    await new Promise((r) => setTimeout(r, 120));
    await renderOnce();

    const initialWidgets = calls.filter((url) => url.includes("/repos/acme/widgets/pulls")).length;
    const initialGadgets = calls.filter((url) => url.includes("/repos/acme/gadgets/pulls")).length;

    mockInput.pressKey("0");
    await new Promise((r) => setTimeout(r, 20));
    await renderOnce();

    mockInput.pressKey("r", { shift: true });
    await new Promise((r) => setTimeout(r, 120));
    await renderOnce();

    const nextWidgets = calls.filter((url) => url.includes("/repos/acme/widgets/pulls")).length;
    const nextGadgets = calls.filter((url) => url.includes("/repos/acme/gadgets/pulls")).length;
    expect(nextWidgets).toBeGreaterThan(initialWidgets);
    expect(nextGadgets).toBeGreaterThan(initialGadgets);
  });

  test("R key keeps current rows visible while refresh-all is in flight", async () => {
    const listBody = [makeSampleRestPR(1)];
    let resolvePrRefresh: (() => void) | undefined;
    const prRefreshGate = new Promise<void>((resolve) => {
      resolvePrRefresh = resolve;
    });
    let listFetchCount = 0;

    const fetch = async (url: string, init?: RequestInit) => {
      if (/\/repos\/acme\/widgets\/pulls\?state=open/.test(url)) {
        listFetchCount++;
        return new Response(JSON.stringify(listBody), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.endsWith("/repos/acme/widgets/pulls/1")) {
        if (listFetchCount > 1) {
          await prRefreshGate;
        }
        return new Response(JSON.stringify({ ...listBody[0], body: "body" }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (/\/repos\/acme\/widgets\/pulls\/1\/reviews/.test(url)) {
        return new Response(JSON.stringify([]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (/\/repos\/acme\/widgets\/pulls\/1\/files/.test(url)) {
        return new Response(JSON.stringify([]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (/\/repos\/acme\/widgets\/commits\/.+\/check-runs/.test(url)) {
        return new Response(JSON.stringify({ check_runs: [] }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.endsWith("/graphql")) {
        const body = JSON.parse(String(init?.body ?? "{}")) as { query?: string };
        const query = body.query ?? "";
        if (query.includes("reviewThreads")) {
          return new Response(
            JSON.stringify({
              data: {
                repository: {
                  pullRequest: {
                    reviewThreads: {
                      pageInfo: { hasNextPage: false, endCursor: null },
                      nodes: [],
                    },
                  },
                },
              },
            }),
            { status: 200, headers: { "Content-Type": "application/json" } },
          );
        }
        return new Response(
          JSON.stringify(makeGraphQLResponse([{ ...SAMPLE_GQL_META, number: 1 }])),
          { status: 200, headers: { "Content-Type": "application/json" } },
        );
      }
      return new Response(JSON.stringify({ message: "Not Found" }), {
        status: 404,
        headers: { "Content-Type": "application/json" },
      });
    };

    const app = createTestLegit({ httpFetch: fetch });

    const { renderOnce, captureCharFrame, mockInput } = await testRenderTracked(
      () => <App app={app} />,
      { width: 180, height: 20 },
    );

    await new Promise((r) => setTimeout(r, 120));
    await renderOnce();

    mockInput.pressKey("r", { shift: true });
    await renderOnce();

    const pendingFrame = captureCharFrame();
    expect(pendingFrame).toContain("PR #1");
    expect(pendingFrame).not.toContain("Loading pull requests");
    expect(pendingFrame).toMatch(/[◌⟳]/);

    resolvePrRefresh?.();
    await new Promise((r) => setTimeout(r, 120));
    await renderOnce();

    const settledFrame = captureCharFrame();
    expect(settledFrame).toContain("PR #1");
  });

  test("manual r jumps ahead of queued refresh-all work", async () => {
    const pr1 = { ...makeSampleRestPR(1), requested_reviewers: [{ login: "testuser" }] };
    const pr2 = makeSampleRestPR(2);
    const pr3 = { ...makeSampleRestPR(3), draft: true };
    const pr4 = { ...makeSampleRestPR(4), draft: true };
    const prs = [pr1, pr2, pr3, pr4];

    let resolvePr1: (() => void) | undefined;
    const pr1Gate = new Promise<void>((resolve) => {
      resolvePr1 = resolve;
    });
    let resolvePr2: (() => void) | undefined;
    const pr2Gate = new Promise<void>((resolve) => {
      resolvePr2 = resolve;
    });
    const refreshOrder: number[] = [];

    async function waitFor(predicate: () => boolean, timeoutMs = 2_000): Promise<void> {
      const start = Date.now();
      while (!predicate()) {
        if (Date.now() - start > timeoutMs) {
          throw new Error(`Timed out after ${timeoutMs}ms`);
        }
        await new Promise((r) => setTimeout(r, 10));
      }
    }

    const fetch = async (url: string, init?: RequestInit) => {
      if (/\/repos\/acme\/widgets\/pulls\?state=open/.test(url)) {
        return new Response(JSON.stringify(prs), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      const prMatch = url.match(/\/repos\/acme\/widgets\/pulls\/(\d+)$/);
      if (prMatch) {
        const number = Number(prMatch[1]);
        refreshOrder.push(number);
        if (number === 1) {
          await pr1Gate;
        } else if (number === 2) {
          await pr2Gate;
        }
        return new Response(
          JSON.stringify({ ...(prs[number - 1] ?? prs[0]), body: `body ${number}` }),
          {
            status: 200,
            headers: { "Content-Type": "application/json" },
          },
        );
      }
      if (/\/repos\/acme\/widgets\/pulls\/\d+\/reviews/.test(url)) {
        return new Response(JSON.stringify([]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (/\/repos\/acme\/widgets\/pulls\/\d+\/files/.test(url)) {
        return new Response(JSON.stringify([]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (/\/repos\/acme\/widgets\/commits\/.+\/check-runs/.test(url)) {
        return new Response(JSON.stringify({ check_runs: [] }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.endsWith("/graphql")) {
        const body = JSON.parse(String(init?.body ?? "{}")) as { query?: string };
        const query = body.query ?? "";
        if (query.includes("reviewThreads")) {
          return new Response(
            JSON.stringify({
              data: {
                repository: {
                  pullRequest: {
                    reviewThreads: {
                      pageInfo: { hasNextPage: false, endCursor: null },
                      nodes: [],
                    },
                  },
                },
              },
            }),
            { status: 200, headers: { "Content-Type": "application/json" } },
          );
        }
        const numbers = Array.from(query.matchAll(/number: (\d+)/g), (match) => Number(match[1]));
        return new Response(
          JSON.stringify(
            makeGraphQLResponse(
              numbers.map((number) => ({
                ...SAMPLE_GQL_META,
                number,
                reviewDecision: "REVIEW_REQUIRED",
                mergeable: "MERGEABLE",
              })),
            ),
          ),
          { status: 200, headers: { "Content-Type": "application/json" } },
        );
      }
      return new Response(JSON.stringify({ message: "Not Found" }), {
        status: 404,
        headers: { "Content-Type": "application/json" },
      });
    };

    const app = createTestLegit({ httpFetch: fetch });

    const { renderOnce, mockInput } = await testRenderTracked(() => <App app={app} />, {
      width: 180,
      height: 20,
    });

    await new Promise((r) => setTimeout(r, 150));
    await renderOnce();

    mockInput.pressKey("r", { shift: true });
    await waitFor(() => refreshOrder.length >= 2);
    expect(refreshOrder.slice(0, 2)).toEqual([1, 2]);

    mockInput.pressKey("j");
    mockInput.pressKey("j");
    mockInput.pressKey("j");
    await renderOnce();
    mockInput.pressKey("r");

    resolvePr1?.();
    await waitFor(() => refreshOrder.length >= 3);
    expect(refreshOrder[2]).toBe(4);

    resolvePr2?.();
    await waitFor(() => refreshOrder.length >= 4);
    expect(refreshOrder).toEqual([1, 2, 4, 3]);
  });

  test("split layout renders list and summary panel separator", async () => {
    const app = createTestLegit({
      httpFetch: mockHttpFetch([makeSampleRestPR(1)]),
    });

    const { renderOnce, captureCharFrame } = await testRenderTracked(() => <App app={app} />, {
      width: 160,
      height: 20,
    });

    await new Promise((r) => setTimeout(r, 50));
    await renderOnce();

    const frame = captureCharFrame();
    expect(frame).toContain("PR #1");
    expect(frame).toContain("│");
  });

  test("loads tracked repos and shows All tab aggregate", async () => {
    const { fetch } = createMockFetch([
      {
        url: /\/repos\/acme\/widgets\/pulls\?/,
        response: { status: 200, body: [makeSampleRestPR(1)] },
      },
      {
        url: /\/graphql/,
        method: "POST",
        response: {
          status: 200,
          body: makeGraphQLResponse([
            { ...SAMPLE_GQL_META, number: 1, additions: 5, deletions: 1 },
          ]),
        },
      },
      {
        url: /\/repos\/acme\/gadgets\/pulls\?/,
        response: { status: 200, body: [makeSampleRestPR(2)] },
      },
      {
        url: /\/graphql/,
        method: "POST",
        response: {
          status: 200,
          body: makeGraphQLResponse([
            { ...SAMPLE_GQL_META, number: 2, additions: 7, deletions: 2 },
          ]),
        },
      },
    ]);
    const app = createTestLegit({ httpFetch: fetch });
    app.config.repos = [{ slug: "acme/widgets" }, { slug: "acme/gadgets" }];

    const { renderOnce, captureCharFrame } = await testRenderTracked(() => <App app={app} />, {
      // Wide enough to show title column with Threads + Blocker + Repo columns.
      width: 180,
      height: 20,
    });

    await new Promise((r) => setTimeout(r, 100));
    await renderOnce();

    const frame = captureCharFrame();
    expect(frame).toContain("All");
    expect(frame).toContain("acme/widgets");
    expect(frame).toContain("acme/gadgets");
    expect(frame).toContain("PR #1");
    expect(frame).toContain("PR #2");
    // Repo column should show short repo names
    expect(frame).toContain("widgets");
    expect(frame).toContain("gadgets");
    expect(frame).toContain("Repo");
  });

  test("switching tabs keeps a PR selected for summary panel", async () => {
    const { fetch } = createMockFetch([
      {
        url: /\/repos\/acme\/widgets\/pulls\?/,
        response: { status: 200, body: [makeSampleRestPR(1)] },
      },
      {
        url: /\/graphql/,
        method: "POST",
        response: {
          status: 200,
          body: makeGraphQLResponse([{ ...SAMPLE_GQL_META, number: 1 }]),
        },
      },
      {
        url: /\/repos\/acme\/gadgets\/pulls\?/,
        response: { status: 200, body: [makeSampleRestPR(2)] },
      },
      {
        url: /\/graphql/,
        method: "POST",
        response: {
          status: 200,
          body: makeGraphQLResponse([{ ...SAMPLE_GQL_META, number: 2 }]),
        },
      },
    ]);
    const app = createTestLegit({ httpFetch: fetch });
    app.config.repos = [{ slug: "acme/widgets" }, { slug: "acme/gadgets" }];

    const { renderOnce, captureCharFrame, mockInput } = await testRenderTracked(
      () => <App app={app} />,
      {
        // Wide enough to show title column with Threads + Blocker + Repo columns.
        width: 180,
        height: 20,
      },
    );

    await new Promise((r) => setTimeout(r, 120));
    await renderOnce();
    mockInput.pressKey("3");
    await new Promise((r) => setTimeout(r, 50));
    await renderOnce();

    const frame = captureCharFrame();
    expect(frame).toContain("PR #2");
    expect(frame).not.toContain("No PR selected");
  });

  test("opens detail view after pressing Enter from the list", async () => {
    const detailPr = { ...makeSampleRestPR(1), body: "Detail body" };
    const detailFetch = async (url: string, init?: RequestInit) => {
      if (/\/repos\/acme\/widgets\/pulls\?state=open/.test(url)) {
        return new Response(JSON.stringify([makeSampleRestPR(1)]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.endsWith("/repos/acme/widgets/pulls/1")) {
        return new Response(JSON.stringify(detailPr), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (/\/repos\/acme\/widgets\/pulls\/1\/files/.test(url)) {
        return new Response(JSON.stringify([]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (/\/repos\/acme\/widgets\/pulls\/1\/reviews/.test(url)) {
        return new Response(JSON.stringify([]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (/\/repos\/acme\/widgets\/commits\/abc123def456\/check-runs/.test(url)) {
        return new Response(JSON.stringify({ check_runs: [] }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (/\/repos\/acme\/widgets\/issues\/1\/comments/.test(url)) {
        return new Response(JSON.stringify([]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.endsWith("/graphql")) {
        const body = JSON.parse(String(init?.body ?? "{}")) as { query?: string };
        const query = body.query ?? "";
        if (query.includes("reviewThreads")) {
          return new Response(
            JSON.stringify({
              data: {
                repository: {
                  pullRequest: {
                    reviewThreads: {
                      pageInfo: { hasNextPage: false, endCursor: null },
                      nodes: [],
                    },
                  },
                },
              },
            }),
            {
              status: 200,
              headers: { "Content-Type": "application/json" },
            },
          );
        }
        return new Response(
          JSON.stringify(
            makeGraphQLResponse([{ ...SAMPLE_GQL_META, number: 1, mergeable: "MERGEABLE" }]),
          ),
          {
            status: 200,
            headers: { "Content-Type": "application/json" },
          },
        );
      }
      return new Response(JSON.stringify({ message: "Not Found" }), {
        status: 404,
        headers: { "Content-Type": "application/json" },
      });
    };

    const app = createTestLegit({ httpFetch: detailFetch });

    const { renderOnce, captureCharFrame, mockInput } = await testRenderTracked(
      () => <App app={app} />,
      { width: 160, height: 20 },
    );

    await new Promise((r) => setTimeout(r, 50));
    await renderOnce();

    mockInput.pressEnter();
    await renderOnce();
    await new Promise((r) => setTimeout(r, 100));
    await renderOnce();

    const frame = captureCharFrame();
    expect(frame).toContain("#1 PR #1");
    expect(frame).toContain("Detail body");
    expect(frame).not.toContain("Loading PR detail");
  });

  test("r key refreshes a single PR without leaving it in Loading details", async () => {
    // Reusable fetch handler that serves enough endpoints to satisfy the
    // streamed list, per-PR refetch, threads, reviews, and checks queries
    // without exhausting any single-shot mock route.
    const listBody = [makeSampleRestPR(1)];
    const prDetailBody = { ...makeSampleRestPR(1), body: "body" };
    const fetch = async (url: string, init?: RequestInit) => {
      if (/\/repos\/acme\/widgets\/pulls\?state=open/.test(url)) {
        return new Response(JSON.stringify(listBody), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.endsWith("/repos/acme/widgets/pulls/1")) {
        return new Response(JSON.stringify(prDetailBody), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (/\/repos\/acme\/widgets\/pulls\/1\/reviews/.test(url)) {
        return new Response(JSON.stringify([]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (/\/repos\/acme\/widgets\/pulls\/1\/files/.test(url)) {
        return new Response(JSON.stringify([]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (/\/repos\/acme\/widgets\/commits\/.+\/check-runs/.test(url)) {
        return new Response(JSON.stringify({ check_runs: [] }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.endsWith("/graphql")) {
        const body = JSON.parse(String(init?.body ?? "{}")) as { query?: string };
        const query = body.query ?? "";
        if (query.includes("reviewThreads")) {
          return new Response(
            JSON.stringify({
              data: {
                repository: {
                  pullRequest: {
                    reviewThreads: {
                      pageInfo: { hasNextPage: false, endCursor: null },
                      nodes: [],
                    },
                  },
                },
              },
            }),
            { status: 200, headers: { "Content-Type": "application/json" } },
          );
        }
        return new Response(
          JSON.stringify(
            makeGraphQLResponse([{ ...SAMPLE_GQL_META, number: 1, mergeable: "MERGEABLE" }]),
          ),
          { status: 200, headers: { "Content-Type": "application/json" } },
        );
      }
      return new Response(JSON.stringify({ message: "Not Found" }), {
        status: 404,
        headers: { "Content-Type": "application/json" },
      });
    };

    const app = createTestLegit({ httpFetch: fetch });

    const { renderOnce, captureCharFrame, mockInput } = await testRenderTracked(
      () => <App app={app} />,
      { width: 180, height: 20 },
    );

    // Initial load + enrichment.
    await new Promise((r) => setTimeout(r, 120));
    await renderOnce();

    let frame = captureCharFrame();
    expect(frame).toContain("PR #1");
    expect(frame).not.toContain("Loading details");

    // Select the PR and press r.
    mockInput.pressKey("j");
    await renderOnce();
    mockInput.pressKey("r");

    // Give the per-PR refetch + enrichment invalidations time to resolve.
    await new Promise((r) => setTimeout(r, 200));
    await renderOnce();

    frame = captureCharFrame();
    expect(frame).toContain("PR #1");
    expect(frame).not.toContain("Loading details");
  });

  test("r key does not strand PR in Loading details group when files fetch is slow", async () => {
    // Simulate the user's scenario: per-PR refetch + enrichment invalidation
    // happen quickly, but the files endpoint is slow. The list grouping must
    // not depend on files, so the PR should not move into "Loading details...".
    const listBody = [makeSampleRestPR(1)];
    const prDetailBody = { ...makeSampleRestPR(1), body: "body" };
    let filesCall = 0;
    const fetch = async (url: string, init?: RequestInit) => {
      if (/\/repos\/acme\/widgets\/pulls\?state=open/.test(url)) {
        return new Response(JSON.stringify(listBody), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.endsWith("/repos/acme/widgets/pulls/1")) {
        return new Response(JSON.stringify(prDetailBody), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (/\/repos\/acme\/widgets\/pulls\/1\/reviews/.test(url)) {
        return new Response(JSON.stringify([]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (/\/repos\/acme\/widgets\/pulls\/1\/files/.test(url)) {
        filesCall++;
        // First call: prompt. Second call (after r): slow.
        if (filesCall > 1) {
          await new Promise((r) => setTimeout(r, 500));
        }
        return new Response(JSON.stringify([]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (/\/repos\/acme\/widgets\/commits\/.+\/check-runs/.test(url)) {
        return new Response(JSON.stringify({ check_runs: [] }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.endsWith("/graphql")) {
        const body = JSON.parse(String(init?.body ?? "{}")) as { query?: string };
        const query = body.query ?? "";
        if (query.includes("reviewThreads")) {
          return new Response(
            JSON.stringify({
              data: {
                repository: {
                  pullRequest: {
                    reviewThreads: {
                      pageInfo: { hasNextPage: false, endCursor: null },
                      nodes: [],
                    },
                  },
                },
              },
            }),
            { status: 200, headers: { "Content-Type": "application/json" } },
          );
        }
        return new Response(
          JSON.stringify(
            makeGraphQLResponse([{ ...SAMPLE_GQL_META, number: 1, mergeable: "MERGEABLE" }]),
          ),
          { status: 200, headers: { "Content-Type": "application/json" } },
        );
      }
      return new Response(JSON.stringify({ message: "Not Found" }), {
        status: 404,
        headers: { "Content-Type": "application/json" },
      });
    };

    const app = createTestLegit({ httpFetch: fetch });

    const { renderOnce, captureCharFrame, mockInput } = await testRenderTracked(
      () => <App app={app} />,
      { width: 180, height: 20 },
    );

    await new Promise((r) => setTimeout(r, 120));
    await renderOnce();

    mockInput.pressKey("j");
    await renderOnce();
    mockInput.pressKey("r");

    // 50ms is enough for enrichment to refresh but NOT enough for the slow files fetch.
    await new Promise((r) => setTimeout(r, 50));
    await renderOnce();

    const frame = captureCharFrame();
    // Neither the list group header nor the summary panel should flash
    // "Loading details…" while the files refetch is still in flight —
    // the files cache preserves prior data through invalidation.
    expect(frame).not.toContain("Loading details");
  });

  test("r key does not strand a PR from a non-cwd repo in Loading details", async () => {
    // cwd repo is acme/widgets (createTestLegit default). The selected PR
    // lives in acme/gadgets — a different tracked repo. After pressing r,
    // the per-PR refetch must preserve the PR's repoSlug so downstream
    // enrichment lookups (threads/reviews) find the right cache entries.
    const prDetailBody = { ...makeSampleRestPR(1), body: "body" };
    let singlePrFetchCount = 0;
    const fetch = async (url: string, init?: RequestInit) => {
      if (/\/repos\/acme\/widgets\/pulls\?state=open/.test(url)) {
        return new Response(JSON.stringify([]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (/\/repos\/acme\/gadgets\/pulls\?state=open/.test(url)) {
        return new Response(JSON.stringify([makeSampleRestPR(1)]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.endsWith("/repos/acme/gadgets/pulls/1")) {
        singlePrFetchCount++;
        return new Response(JSON.stringify(prDetailBody), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (/\/repos\/acme\/gadgets\/pulls\/1\/reviews/.test(url)) {
        return new Response(JSON.stringify([]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (/\/repos\/acme\/gadgets\/pulls\/1\/files/.test(url)) {
        return new Response(JSON.stringify([]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (/\/repos\/acme\/gadgets\/commits\/.+\/check-runs/.test(url)) {
        return new Response(JSON.stringify({ check_runs: [] }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.endsWith("/graphql")) {
        const body = JSON.parse(String(init?.body ?? "{}")) as { query?: string };
        const query = body.query ?? "";
        if (query.includes("reviewThreads")) {
          return new Response(
            JSON.stringify({
              data: {
                repository: {
                  pullRequest: {
                    reviewThreads: {
                      pageInfo: { hasNextPage: false, endCursor: null },
                      nodes: [],
                    },
                  },
                },
              },
            }),
            { status: 200, headers: { "Content-Type": "application/json" } },
          );
        }
        return new Response(
          JSON.stringify(
            makeGraphQLResponse([{ ...SAMPLE_GQL_META, number: 1, mergeable: "MERGEABLE" }]),
          ),
          { status: 200, headers: { "Content-Type": "application/json" } },
        );
      }
      return new Response(JSON.stringify({ message: "Not Found" }), {
        status: 404,
        headers: { "Content-Type": "application/json" },
      });
    };

    const app = createTestLegit({ httpFetch: fetch });
    app.config.repos = [{ slug: "acme/gadgets" }];

    const { renderOnce, captureCharFrame, mockInput } = await testRenderTracked(
      () => <App app={app} />,
      { width: 200, height: 20 },
    );

    // Initial load + enrichment across all tracked repos.
    await new Promise((r) => setTimeout(r, 300));
    await renderOnce();

    mockInput.pressKey("j");
    await renderOnce();

    let frame = captureCharFrame();
    expect(frame).toContain("PR #1");
    expect(frame).not.toContain("Loading details");

    mockInput.pressKey("r");
    await new Promise((r) => setTimeout(r, 400));
    await renderOnce();

    frame = captureCharFrame();
    expect(frame).toContain("PR #1");
    expect(frame).not.toContain("Loading details");
    // Refetch must actually fire (initial load does not hit the single-PR endpoint).
    expect(singlePrFetchCount).toBeGreaterThan(0);
  });

  test("selection resets to first PR when switching back to a tab", async () => {
    const { fetch } = createMockFetch([
      {
        url: /\/repos\/acme\/widgets\/pulls\?/,
        response: {
          status: 200,
          body: [makeSampleRestPR(1), makeSampleRestPR(2), makeSampleRestPR(3)],
        },
      },
      {
        url: /\/graphql/,
        method: "POST",
        response: {
          status: 200,
          body: makeGraphQLResponse([
            { ...SAMPLE_GQL_META, number: 1 },
            { ...SAMPLE_GQL_META, number: 2 },
            { ...SAMPLE_GQL_META, number: 3 },
          ]),
        },
      },
      {
        url: /\/repos\/acme\/gadgets\/pulls\?/,
        response: {
          status: 200,
          body: [makeSampleRestPR(10), makeSampleRestPR(11)],
        },
      },
      {
        url: /\/graphql/,
        method: "POST",
        response: {
          status: 200,
          body: makeGraphQLResponse([
            { ...SAMPLE_GQL_META, number: 10 },
            { ...SAMPLE_GQL_META, number: 11 },
          ]),
        },
      },
    ]);
    const app = createTestLegit({ httpFetch: fetch });
    app.config.repos = [{ slug: "acme/widgets" }, { slug: "acme/gadgets" }];

    const { renderOnce, captureCharFrame, mockInput } = await testRenderTracked(
      () => <App app={app} />,
      { width: 150, height: 20 },
    );

    await new Promise((r) => setTimeout(r, 120));
    await renderOnce();

    let frame = captureCharFrame();
    expect(frame).toContain("PR #1");

    mockInput.pressKey("1");
    await new Promise((r) => setTimeout(r, 50));
    await renderOnce();

    mockInput.pressKey("j");
    mockInput.pressKey("j");
    await renderOnce();
    frame = captureCharFrame();
    expect(frame).toContain("PR #3");

    mockInput.pressKey("0");
    await new Promise((r) => setTimeout(r, 50));
    await renderOnce();

    frame = captureCharFrame();
    expect(frame).toContain("#1");
    expect(frame).toContain("PR #1");
  });
});

describe("prUrl", () => {
  test("builds correct GitHub PR URL", () => {
    expect(prUrl("acme/widgets", 42)).toBe("https://github.com/acme/widgets/pull/42");
  });
});
