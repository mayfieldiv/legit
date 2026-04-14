import { describe, test, expect } from "bun:test";
import { testRender } from "@opentui/solid";
import { createSignal } from "solid-js";
import { AppShell } from "../src/components/AppShell";
import { makePR } from "./helpers";

describe("AppShell", () => {
  test("shows loading state when loading is true", async () => {
    const { renderOnce, captureCharFrame } = await testRender(
      () => (
        <AppShell
          view={{ view: "list" }}
          onEnterDetail={() => {}}
          prs={[]}
          loading={true}
          repoSlug="acme/widgets"
          onRefreshSelected={() => {}}
          onRefreshAllActive={() => {}}
        />
      ),
      { width: 130, height: 20 },
    );

    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toMatch(/loading/i);
  });

  test("shows PR list when loaded", async () => {
    const prs = [
      makePR({ number: 1, title: "First PR" }),
      makePR({ number: 2, title: "Second PR" }),
    ];

    const { renderOnce, captureCharFrame } = await testRender(
      () => (
        <AppShell
          view={{ view: "list" }}
          onEnterDetail={() => {}}
          prs={prs}
          loading={false}
          repoSlug="acme/widgets"
          onRefreshSelected={() => {}}
          onRefreshAllActive={() => {}}
        />
      ),
      // Wide enough to accommodate the Threads column without squeezing the title.
      { width: 160, height: 20 },
    );

    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toContain("First PR");
    expect(frame).toContain("Second PR");
  });

  test("shows repo name in header", async () => {
    const { renderOnce, captureCharFrame } = await testRender(
      () => (
        <AppShell
          view={{ view: "list" }}
          onEnterDetail={() => {}}
          prs={[]}
          loading={false}
          repoSlug="acme/widgets"
          onRefreshSelected={() => {}}
          onRefreshAllActive={() => {}}
        />
      ),
      { width: 130, height: 20 },
    );

    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toContain("acme/widgets");
  });

  test("shows error message when error is set", async () => {
    const { renderOnce, captureCharFrame } = await testRender(
      () => (
        <AppShell
          view={{ view: "list" }}
          onEnterDetail={() => {}}
          prs={[]}
          loading={false}
          repoSlug="acme/widgets"
          error="Network timeout"
          onRefreshSelected={() => {}}
          onRefreshAllActive={() => {}}
        />
      ),
      { width: 130, height: 20 },
    );

    await renderOnce();
    const frame = captureCharFrame();
    // captureCharFrame may have minor overlap artifacts from layout,
    // but the error text components should be present
    expect(frame).toMatch(/Error/);
    expect(frame).toMatch(/Network/);
    expect(frame).toMatch(/timeout/);
  });

  test("shows PR count in header", async () => {
    const prs = [makePR({ number: 1 }), makePR({ number: 2 })];

    const { renderOnce, captureCharFrame } = await testRender(
      () => (
        <AppShell
          view={{ view: "list" }}
          onEnterDetail={() => {}}
          prs={prs}
          loading={false}
          repoSlug="acme/widgets"
          onRefreshSelected={() => {}}
          onRefreshAllActive={() => {}}
        />
      ),
      { width: 130, height: 20 },
    );

    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toContain("2 open PRs");
  });

  test("updates the summary panel without mixing title and metadata", async () => {
    const prs = [
      makePR({
        number: 597,
        title: "Add authorization middleware with secure fallback policy by default",
        author: "cmbankester",
        repoSlug: "immense/immybot-manager",
      }),
      makePR({
        number: 598,
        title: "FallbackPolicy — require auth",
        author: "cmbankester",
        repoSlug: "immense/immybot-manager",
      }),
    ];
    const [selectedPr, setSelectedPr] = createSignal(prs[0]);
    const { renderOnce, captureCharFrame } = await testRender(
      () => (
        <AppShell
          view={{ view: "list" }}
          onEnterDetail={() => {}}
          prs={prs}
          loading={false}
          repoSlug="All repos"
          selectedPr={selectedPr()}
          summaryThreads={[]}
          summaryChecks={[]}
          summaryReviews={[]}
          onRefreshSelected={() => {}}
          onRefreshAllActive={() => {}}
        />
      ),
      { width: 156, height: 17 },
    );

    await renderOnce();
    setSelectedPr(prs[1]!);
    await renderOnce();

    const lines = captureCharFrame().split("\n");
    expect(lines[1]).toContain("FallbackPolicy");
    expect(lines[1]).not.toContain("cmbankester");
    expect(lines[2]).toContain("cmbankester");
    expect(lines[2]).toContain("#598");
    expect(lines[2]).not.toContain("middleware");
  });

  test("renders tab bar with All and repo tabs", async () => {
    const { renderOnce, captureCharFrame } = await testRender(
      () => (
        <AppShell
          view={{ view: "list" }}
          onEnterDetail={() => {}}
          prs={[]}
          loading={false}
          repoSlug="acme/widgets"
          tabs={["All", "acme/widgets", "acme/gadgets"]}
          activeTab={0}
          onTabChange={() => {}}
          onRefreshSelected={() => {}}
          onRefreshAllActive={() => {}}
        />
      ),
      { width: 130, height: 20 },
    );

    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toContain("All");
    expect(frame).toContain("acme/widgets");
    expect(frame).toContain("acme/gadgets");
  });

  test("tab keybindings switch tabs", async () => {
    const calls: number[] = [];
    const { renderOnce, mockInput } = await testRender(
      () => (
        <AppShell
          view={{ view: "list" }}
          onEnterDetail={() => {}}
          prs={[]}
          loading={false}
          repoSlug="acme/widgets"
          tabs={["All", "acme/widgets", "acme/gadgets"]}
          activeTab={1}
          onTabChange={(index) => calls.push(index)}
          onRefreshSelected={() => {}}
          onRefreshAllActive={() => {}}
        />
      ),
      { width: 130, height: 20 },
    );

    await renderOnce();
    mockInput.pressKey("l");
    mockInput.pressKey("h");
    mockInput.pressKey("right");
    mockInput.pressKey("left");
    mockInput.pressKey("3");
    mockInput.pressKey("0");
    mockInput.pressKey("[");
    mockInput.pressKey("]");
    await renderOnce();

    expect(calls).toContain(2);
    expect(calls).toContain(0);
  });

  test("number keys map 0 to All and 1 to first repo", async () => {
    const calls: number[] = [];
    const { renderOnce, mockInput } = await testRender(
      () => (
        <AppShell
          view={{ view: "list" }}
          onEnterDetail={() => {}}
          prs={[]}
          loading={false}
          repoSlug="acme/widgets"
          tabs={["All", "acme/widgets", "acme/gadgets"]}
          activeTab={2}
          onTabChange={(index) => calls.push(index)}
          onRefreshSelected={() => {}}
          onRefreshAllActive={() => {}}
        />
      ),
      { width: 130, height: 20 },
    );

    await renderOnce();
    mockInput.pressKey("0");
    mockInput.pressKey("1");
    await renderOnce();

    expect(calls).toEqual([0, 1]);
  });
});
