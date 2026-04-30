import { afterAll, afterEach, describe, expect, test } from "bun:test";
import { testRender } from "@opentui/solid";
import type { CliRenderer } from "@opentui/core";
import { App } from "../src/App";
import {
  cleanupTmpDirs,
  createTestLegit,
  makeGraphQLResponse,
  makeSampleRestPR,
  SAMPLE_GQL_META,
} from "./helpers";

let activeRenderer: CliRenderer | undefined;

afterAll(() => {
  activeRenderer?.destroy();
  activeRenderer = undefined;
  cleanupTmpDirs();
});

async function testRenderTracked(
  ...args: Parameters<typeof testRender>
): ReturnType<typeof testRender> {
  activeRenderer?.destroy();
  const result = await testRender(...args);
  activeRenderer = result.renderer;
  return result;
}

describe("mergeable refresh consistency", () => {
  afterEach(async () => {
    activeRenderer?.destroy();
    activeRenderer = undefined;
    await new Promise((resolve) => setTimeout(resolve, 10));
  });

  test("single-PR refresh updates both the list row and summary panel mergeability", async () => {
    let reviewStatusCalls = 0;
    const fetch = async (url: string, init?: RequestInit) => {
      if (/\/repos\/acme\/widgets\/pulls\?state=open/.test(url)) {
        return new Response(JSON.stringify([makeSampleRestPR(1)]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.endsWith("/repos/acme/widgets/pulls/1")) {
        return new Response(JSON.stringify({ ...makeSampleRestPR(1), body: "body" }), {
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
      if (/\/repos\/acme\/widgets\/issues\/1\/comments/.test(url)) {
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
            {
              status: 200,
              headers: { "Content-Type": "application/json" },
            },
          );
        }

        reviewStatusCalls++;
        const mergeable = reviewStatusCalls === 1 ? "CONFLICTING" : "MERGEABLE";
        return new Response(
          JSON.stringify(makeGraphQLResponse([{ ...SAMPLE_GQL_META, number: 1, mergeable }])),
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

    const app = createTestLegit({ httpFetch: fetch });
    const { renderOnce, captureCharFrame, mockInput } = await testRenderTracked(
      () => <App app={app} />,
      { width: 180, height: 20 },
    );

    await new Promise((resolve) => setTimeout(resolve, 300));
    await renderOnce();

    let frame = captureCharFrame();
    expect(frame).toContain("! conflict");

    mockInput.pressKey("r");
    await new Promise((resolve) => setTimeout(resolve, 350));
    await renderOnce();

    frame = captureCharFrame();
    expect(frame).toContain("✓ mergeable");
    expect(frame).not.toContain("! conflict");
  });

  test("single-PR refresh swaps checks to the refreshed head commit", async () => {
    let oldCheckCalls = 0;
    let newCheckCalls = 0;
    let reviewStatusCalls = 0;
    const fetch = async (url: string, init?: RequestInit) => {
      if (/\/repos\/acme\/widgets\/pulls\?state=open/.test(url)) {
        return new Response(JSON.stringify([makeSampleRestPR(1)]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.endsWith("/repos/acme/widgets/pulls/1")) {
        const headSha = "newsha";
        return new Response(
          JSON.stringify({
            ...makeSampleRestPR(1),
            body: "body",
            head: { ref: "feature", sha: headSha },
            base: { ref: "main" },
          }),
          {
            status: 200,
            headers: { "Content-Type": "application/json" },
          },
        );
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
      if (/\/repos\/acme\/widgets\/issues\/1\/comments/.test(url)) {
        return new Response(JSON.stringify([]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.includes("/repos/acme/widgets/commits/oldsha/check-runs")) {
        oldCheckCalls++;
        return new Response(
          JSON.stringify({
            check_runs: [{ name: "build", status: "completed", conclusion: "failure" }],
          }),
          {
            status: 200,
            headers: { "Content-Type": "application/json" },
          },
        );
      }
      if (url.includes("/repos/acme/widgets/commits/newsha/check-runs")) {
        newCheckCalls++;
        return new Response(
          JSON.stringify({
            check_runs: [{ name: "build", status: "completed", conclusion: "success" }],
          }),
          {
            status: 200,
            headers: { "Content-Type": "application/json" },
          },
        );
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

        reviewStatusCalls++;
        const oid = reviewStatusCalls === 1 ? "oldsha" : "newsha";
        return new Response(
          JSON.stringify(
            makeGraphQLResponse([
              {
                ...SAMPLE_GQL_META,
                number: 1,
                reviewDecision: "REVIEW_REQUIRED",
                commits: {
                  nodes: [{ commit: { committedDate: "2026-03-14T00:00:00Z", oid } }],
                },
              },
            ]),
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

    const app = createTestLegit({ httpFetch: fetch });
    const { renderOnce, captureCharFrame, mockInput } = await testRenderTracked(
      () => <App app={app} />,
      { width: 180, height: 20 },
    );

    await new Promise((resolve) => setTimeout(resolve, 300));
    await renderOnce();

    expect(oldCheckCalls).toBeGreaterThan(0);
    let frame = captureCharFrame();
    expect(frame).toContain("Waiting on author");
    expect(frame).not.toContain("Needs review");

    mockInput.pressKey("j");
    await renderOnce();
    mockInput.pressKey("r");
    await new Promise((resolve) => setTimeout(resolve, 500));
    await renderOnce();

    expect(newCheckCalls).toBeGreaterThan(0);
    frame = captureCharFrame();
    expect(frame).toContain("Needs review");
    expect(frame).not.toContain("Waiting on author");
  });

  test("list row reflects draft after refreshing from detail view", async () => {
    const totalPrs = 60;
    const targetPr = 30;
    let pullDetailCalls = 0;
    const fetch = async (url: string, init?: RequestInit) => {
      if (/\/repos\/acme\/widgets\/pulls\?state=open/.test(url)) {
        const list = Array.from({ length: totalPrs }, (_, i) => ({
          ...makeSampleRestPR(i + 1),
          title: `PR #${i + 1}`,
          draft: false,
        }));
        return new Response(JSON.stringify(list), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      const pullMatch = url.match(/\/repos\/acme\/widgets\/pulls\/(\d+)$/);
      if (pullMatch) {
        const n = Number(pullMatch[1]);
        if (n === targetPr) pullDetailCalls++;
        const draft = n === targetPr && pullDetailCalls > 0;
        return new Response(
          JSON.stringify({ ...makeSampleRestPR(n), title: `PR #${n}`, body: "body", draft }),
          { status: 200, headers: { "Content-Type": "application/json" } },
        );
      }
      if (/\/repos\/acme\/widgets\/pulls\/\d+\/(reviews|files)/.test(url)) {
        return new Response(JSON.stringify([]), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (/\/repos\/acme\/widgets\/issues\/\d+\/comments/.test(url)) {
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
        const numbers = Array.from(query.matchAll(/pullRequest\(number: (\d+)\)/g), (m) =>
          Number(m[1]),
        );
        return new Response(
          JSON.stringify(
            makeGraphQLResponse(
              numbers.map((n) => ({
                ...SAMPLE_GQL_META,
                number: n,
                reviewDecision: "REVIEW_REQUIRED",
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
    const { renderOnce, captureCharFrame, mockInput } = await testRenderTracked(
      () => <App app={app} />,
      { width: 200, height: 80 },
    );

    await new Promise((resolve) => setTimeout(resolve, 400));
    await renderOnce();

    let frame = captureCharFrame();
    expect(frame).toContain(`PR #${targetPr}`);
    expect(frame).toContain("Needs review");

    // Navigate down to the target PR
    for (let i = 0; i < targetPr - 1; i++) {
      mockInput.pressKey("j");
    }
    await renderOnce();

    // Enter detail view
    mockInput.pressEnter();
    await new Promise((resolve) => setTimeout(resolve, 300));
    await renderOnce();

    // Refresh from detail view
    mockInput.pressKey("r");
    await new Promise((resolve) => setTimeout(resolve, 400));
    await renderOnce();

    // Exit detail view (escape)
    mockInput.pressEscape();
    await renderOnce();
    await new Promise((resolve) => setTimeout(resolve, 100));
    await renderOnce();

    frame = captureCharFrame();
    expect(pullDetailCalls).toBeGreaterThan(0);
    // List should now show target PR in Waiting on author group with draft suffix
    expect(frame).toContain("Waiting on author");
    const targetTitle = `PR #${targetPr}`;
    expect(frame.match(new RegExp(`Waiting on author[\\s\\S]*${targetTitle}\\b`))).toBeTruthy();
    expect(frame.match(new RegExp(`${targetTitle}\\b[^\\n]*draft`))).toBeTruthy();
  });
});
