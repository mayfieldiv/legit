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

    await new Promise((resolve) => setTimeout(resolve, 120));
    await renderOnce();

    let frame = captureCharFrame();
    expect(frame).toContain("! conflict");

    mockInput.pressKey("r");
    await new Promise((resolve) => setTimeout(resolve, 250));
    await renderOnce();

    frame = captureCharFrame();
    expect(frame).toContain("✓ mergeable");
    expect(frame).not.toContain("! conflict");
  });
});
