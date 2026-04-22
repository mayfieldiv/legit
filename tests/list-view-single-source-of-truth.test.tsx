import { afterEach, describe, expect, test } from "bun:test";
import { testRender } from "@opentui/solid";
import type { CliRenderer } from "@opentui/core";
import { createSignal } from "solid-js";
import { ListView } from "../src/components/ListView";
import { makePR } from "./helpers";

let activeRenderer: CliRenderer | undefined;

async function testRenderTracked(
  ...args: Parameters<typeof testRender>
): ReturnType<typeof testRender> {
  activeRenderer?.destroy();
  const result = await testRender(...args);
  activeRenderer = result.renderer;
  return result;
}

describe("ListView single source of truth", () => {
  afterEach(async () => {
    activeRenderer?.destroy();
    activeRenderer = undefined;
    await new Promise((resolve) => setTimeout(resolve, 10));
  });

  test("review column updates when the live PR object changes without moving groups", async () => {
    const pr = makePR({
      number: 1,
      mergeable: "CONFLICTING",
      reviewDecision: "APPROVED",
      requestedReviewers: ["alice"],
    });

    const [prs, setPrs] = createSignal([pr]);

    const { renderOnce, captureCharFrame } = await testRenderTracked(
      () => (
        <ListView
          prs={prs()}
          currentUser="bob"
          onRefreshSelected={() => {}}
          onRefreshAll={() => {}}
          onEnterDetail={() => {}}
        />
      ),
      { width: 160, height: 12 },
    );

    await renderOnce();
    let frame = captureCharFrame();
    expect(frame).toContain("! approved");

    pr.mergeable = "MERGEABLE";
    setPrs([pr]);

    await renderOnce();
    frame = captureCharFrame();
    expect(frame).toContain("approved");
    expect(frame).not.toContain("! approved");
  });
});
