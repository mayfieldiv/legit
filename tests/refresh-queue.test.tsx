import { describe, test, expect, afterAll, afterEach } from "bun:test";
import { testRender } from "@opentui/solid";
import type { CliRenderer } from "@opentui/core";
import { App } from "../src/App";
import { GITHUB_HTTP_MAX_CONCURRENT_REQUESTS } from "../src/lib/concurrency";
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

afterEach(async () => {
  activeRenderer?.destroy();
  activeRenderer = undefined;
  await new Promise((resolve) => setTimeout(resolve, 10));
});

async function testRenderTracked(
  ...args: Parameters<typeof testRender>
): ReturnType<typeof testRender> {
  activeRenderer?.destroy();
  const result = await testRender(...args);
  activeRenderer = result.renderer;
  return result;
}

async function waitForCondition(
  condition: () => boolean | Promise<boolean>,
  timeoutMs = 2_000,
): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    if (await condition()) return;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error(`Timed out after ${timeoutMs}ms`);
}

function json(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

describe("refresh queue", () => {
  test("bulk refresh can saturate the HTTP concurrency limit", async () => {
    const prs = Array.from({ length: 12 }, (_, index) => makeSampleRestPR(index + 1));
    let slowMode = false;
    let activeRequests = 0;
    let maxActiveRequests = 0;

    const fetch = async (url: string, init?: RequestInit) => {
      if (slowMode) {
        activeRequests++;
        maxActiveRequests = Math.max(maxActiveRequests, activeRequests);
        await new Promise((resolve) => setTimeout(resolve, 40));
      }

      try {
        if (/\/pulls\?/.test(url)) {
          return json(prs);
        }

        const pullMatch = url.match(/\/pulls\/(\d+)$/);
        if (pullMatch) {
          return json(makeSampleRestPR(Number(pullMatch[1])));
        }

        if (/\/reviews\?per_page=100&page=1$/.test(url) || /\/reviews$/.test(url)) {
          return json([]);
        }

        if (/\/check-runs\?per_page=100&page=1$/.test(url)) {
          return json({ total_count: 0, check_runs: [] });
        }

        if (url.endsWith("/graphql")) {
          const body = JSON.parse(String(init?.body ?? "{}")) as {
            query?: string;
            variables?: { number?: number };
          };
          const query = body.query ?? "";

          if (query.includes("reviewThreads")) {
            return json({
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
            });
          }

          const numbers = Array.from(query.matchAll(/pullRequest\(number: (\d+)\)/g), (match) =>
            Number(match[1]),
          );
          return json(
            makeGraphQLResponse(
              numbers.map((number) => ({
                ...SAMPLE_GQL_META,
                number,
              })),
            ),
          );
        }

        return new Response(JSON.stringify({ message: `Unexpected URL: ${url}` }), {
          status: 404,
          headers: { "Content-Type": "application/json" },
        });
      } finally {
        if (slowMode) activeRequests--;
      }
    };

    const app = createTestLegit({ httpFetch: fetch });
    const { renderOnce, captureCharFrame, mockInput } = await testRenderTracked(
      () => <App app={app} />,
      {
        width: 180,
        height: 20,
      },
    );

    await waitForCondition(async () => {
      await renderOnce();
      const frame = captureCharFrame();
      const stats = app.githubNetworkStats;
      return frame.includes("PR #1") && stats.inFlight === 0 && stats.waiting === 0;
    }, 3_000);

    slowMode = true;
    activeRequests = 0;
    maxActiveRequests = 0;

    mockInput.pressKey("r", { shift: true });

    await waitForCondition(() => maxActiveRequests >= GITHUB_HTTP_MAX_CONCURRENT_REQUESTS, 3_000);

    expect(maxActiveRequests).toBe(GITHUB_HTTP_MAX_CONCURRENT_REQUESTS);

    await waitForCondition(() => app.githubNetworkStats.inFlight === 0, 3_000);
  });
});
