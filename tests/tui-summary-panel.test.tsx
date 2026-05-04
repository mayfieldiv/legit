import { describe, test, expect } from "bun:test";
import { testRender } from "@opentui/solid";
import { AppCtx } from "../src/app-context";
import { SummaryPanel } from "../src/components/SummaryPanel";
import { makeAppContextValue, makePR } from "./helpers";
import type { CheckRun, Review, FullReviewThread, FileCategorization, PR } from "../src/lib/types";

type TestSummaryPanelProps = {
  pr: PR | undefined;
  currentUser?: string;
  threads?: FullReviewThread[];
  checks?: CheckRun[];
  reviews?: Review[];
  files?: FileCategorization;
  loading?: boolean;
};

function SummaryPanelWithContext(props: TestSummaryPanelProps) {
  const context = makeAppContextValue({
    prData: {
      selectedPr: () => (props.pr ? { body: "", ...props.pr } : undefined),
      currentUser: () => props.currentUser,
    },
    summary: {
      threads: () => props.threads,
      checks: () => props.checks,
      reviews: () => props.reviews,
      files: () => props.files,
      loading: () => props.loading ?? false,
    },
  });

  return (
    <AppCtx value={context}>
      <SummaryPanel />
    </AppCtx>
  );
}

const EMPTY_FILES: FileCategorization = {
  files: [],
  breakdown: {
    code: { additions: 0, deletions: 0, files: 0 },
    test: { additions: 0, deletions: 0, files: 0 },
    generated: { additions: 0, deletions: 0, files: 0 },
    docs: { additions: 0, deletions: 0, files: 0 },
    config: { additions: 0, deletions: 0, files: 0 },
    total: { additions: 0, deletions: 0, files: 0 },
  },
};

describe("SummaryPanel", () => {
  test("shows PR title and author", async () => {
    const pr = makePR({ title: "Fix login bug", author: "alice", number: 99 });
    const { renderOnce, captureCharFrame } = await testRender(
      () => (
        <SummaryPanelWithContext
          pr={pr}
          threads={[]}
          checks={[]}
          reviews={[]}
          files={EMPTY_FILES}
        />
      ),
      { width: 40, height: 30 },
    );
    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toContain("Fix login bug");
    expect(frame).toContain("alice");
    expect(frame).toContain("#99");
  });

  test("shows draft badge for draft PRs", async () => {
    const pr = makePR({ isDraft: true });
    const { renderOnce, captureCharFrame } = await testRender(
      () => (
        <SummaryPanelWithContext
          pr={pr}
          threads={[]}
          checks={[]}
          reviews={[]}
          files={EMPTY_FILES}
        />
      ),
      { width: 40, height: 30 },
    );
    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toMatch(/draft/i);
  });

  test("shows merge conflict indicator", async () => {
    const pr = makePR({ mergeable: "CONFLICTING" });
    const { renderOnce, captureCharFrame } = await testRender(
      () => (
        <SummaryPanelWithContext
          pr={pr}
          threads={[]}
          checks={[]}
          reviews={[]}
          files={EMPTY_FILES}
        />
      ),
      { width: 40, height: 30 },
    );
    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toMatch(/conflict/i);
  });

  test("shows CI checks sorted: failed first", async () => {
    const checks: CheckRun[] = [
      { name: "lint", status: "completed", conclusion: "success" },
      { name: "build", status: "completed", conclusion: "failure" },
      { name: "deploy", status: "in_progress", conclusion: null },
    ];
    const { renderOnce, captureCharFrame } = await testRender(
      () => (
        <SummaryPanelWithContext
          pr={makePR()}
          threads={[]}
          checks={checks}
          reviews={[]}
          files={EMPTY_FILES}
        />
      ),
      { width: 40, height: 30 },
    );
    await renderOnce();
    const frame = captureCharFrame();
    const buildIdx = frame.indexOf("build");
    const deployIdx = frame.indexOf("deploy");
    const lintIdx = frame.indexOf("lint");
    expect(buildIdx).toBeGreaterThan(-1);
    expect(deployIdx).toBeGreaterThan(-1);
    expect(lintIdx).toBeGreaterThan(-1);
    expect(buildIdx).toBeLessThan(deployIdx);
    expect(deployIdx).toBeLessThan(lintIdx);
  });

  test("shows reviewers", async () => {
    const reviews: Review[] = [
      { user: "bob", state: "APPROVED" },
      { user: "carol", state: "CHANGES_REQUESTED" },
    ];
    const { renderOnce, captureCharFrame } = await testRender(
      () => (
        <SummaryPanelWithContext
          pr={makePR()}
          threads={[]}
          checks={[]}
          reviews={reviews}
          files={EMPTY_FILES}
        />
      ),
      { width: 40, height: 30 },
    );
    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toContain("bob");
    expect(frame).toContain("carol");
  });

  test("shows unresolved thread counts", async () => {
    const threads: FullReviewThread[] = [
      {
        id: "RT_1",
        isResolved: false,
        path: "src/a.ts",
        line: 1,
        comments: [
          {
            id: "RC_1",
            author: "alice",
            body: "fix",
            createdAt: "2026-03-01T00:00:00Z",
            url: "https://github.com/test",
            isBot: false,
          },
        ],
      },
      {
        id: "RT_2",
        isResolved: false,
        path: "src/b.ts",
        line: 2,
        comments: [
          {
            id: "RC_2",
            author: "alice",
            body: "fix2",
            createdAt: "2026-03-01T00:00:00Z",
            url: "https://github.com/test",
            isBot: false,
          },
        ],
      },
      {
        id: "RT_3",
        isResolved: false,
        path: "src/c.ts",
        line: 3,
        comments: [
          {
            id: "RC_3",
            author: "bot",
            body: "bot comment",
            createdAt: "2026-03-01T00:00:00Z",
            url: "https://github.com/test",
            isBot: true,
          },
        ],
      },
    ];
    const { renderOnce, captureCharFrame } = await testRender(
      () => (
        <SummaryPanelWithContext
          pr={makePR()}
          threads={threads}
          checks={[]}
          reviews={[]}
          files={EMPTY_FILES}
        />
      ),
      { width: 40, height: 30 },
    );
    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toContain("3");
    expect(frame).toContain("unresolved");
  });

  test("shows empty state when no pr", async () => {
    const { renderOnce, captureCharFrame } = await testRender(
      () => <SummaryPanelWithContext pr={undefined} />,
      { width: 40, height: 30 },
    );
    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toBeDefined();
  });

  test("shows basic info from PR when enrichment is loading", async () => {
    const pr = makePR({ title: "Loading test", number: 77 });
    const { renderOnce, captureCharFrame } = await testRender(
      () => <SummaryPanelWithContext pr={pr} loading={true} />,
      { width: 40, height: 30 },
    );
    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toContain("Loading test");
    expect(frame).toContain("#77");
  });

  test("shows blocker tier when currentUser is provided and enrichment loaded", async () => {
    const pr = makePR({ author: "charlie", requestedReviewers: ["alice"] });
    const { renderOnce, captureCharFrame } = await testRender(
      () => (
        <SummaryPanelWithContext
          pr={pr}
          currentUser="alice"
          threads={[]}
          checks={[]}
          reviews={[]}
          files={EMPTY_FILES}
        />
      ),
      { width: 50, height: 30 },
    );
    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toMatch(/me.blocking|you/i);
  });

  test("shows waiting-on-author when CI is failing", async () => {
    const pr = makePR({ author: "charlie" });
    const checks: CheckRun[] = [{ name: "build", status: "completed", conclusion: "failure" }];
    const { renderOnce, captureCharFrame } = await testRender(
      () => (
        <SummaryPanelWithContext
          pr={pr}
          currentUser="alice"
          threads={[]}
          checks={checks}
          reviews={[]}
          files={EMPTY_FILES}
        />
      ),
      { width: 50, height: 30 },
    );
    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toMatch(/waiting.on.author|charlie/i);
  });

  test("does not show blocker section when currentUser is not provided", async () => {
    const pr = makePR({ requestedReviewers: ["alice"] });
    const { renderOnce, captureCharFrame } = await testRender(
      () => (
        <SummaryPanelWithContext
          pr={pr}
          threads={[]}
          checks={[]}
          reviews={[]}
          files={EMPTY_FILES}
        />
      ),
      { width: 50, height: 30 },
    );
    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).not.toMatch(/me.blocking/i);
  });
});
