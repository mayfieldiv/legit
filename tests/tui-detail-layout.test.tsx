import { describe, test, expect } from "bun:test";
import { testRender } from "@opentui/solid";
import { AppCtx } from "../src/app-context";
import { DetailView } from "../src/components/DetailView";
import type { FullReviewThread, PRDetail } from "../src/lib/types";
import { makeAppContextValue } from "./helpers";

function findLineIndex(lines: string[], text: string): number {
  return lines.findIndex((line) => line.includes(text));
}

const pr: PRDetail = {
  number: 42,
  repoSlug: "acme/widgets",
  title: "Detail layout regression",
  author: "alice",
  createdAt: "2026-04-14T00:00:00Z",
  updatedAt: "2026-04-14T00:00:00Z",
  additions: 10,
  deletions: 2,
  isDraft: false,
  labels: [],
  requestedReviewers: [],
  assignees: [],
  reviewDecision: "",
  mergeable: "MERGEABLE",
  lastCommitDate: null,
  headCommitSha: "deadbeef",
  headRef: "feature/detail-layout",
  baseRef: "main",
  headRepositoryOwner: "acme",
  state: "OPEN",
  body: "",
};

const threads: FullReviewThread[] = [
  {
    id: "RT_1",
    isResolved: false,
    path: "src/foo.ts",
    line: 10,
    comments: [
      {
        id: "RC_1",
        author: "bob",
        body: "First thread",
        createdAt: "2026-04-14T00:00:00Z",
        url: "https://github.com/acme/widgets/pull/42#discussion_r1",
        isBot: false,
      },
    ],
  },
  {
    id: "RT_2",
    isResolved: false,
    path: "src/bar.ts",
    line: 20,
    comments: [
      {
        id: "RC_2",
        author: "carol",
        body: "Second thread",
        createdAt: "2026-04-14T00:00:00Z",
        url: "https://github.com/acme/widgets/pull/42#discussion_r2",
        isBot: false,
      },
    ],
  },
];

function DetailViewWithContext() {
  const context = makeAppContextValue({
    detail: {
      view: () => ({ view: "detail", pr }),
      pr: () => pr,
      threads: () => threads,
      comments: () => [],
      loading: () => false,
      showResolved: () => false,
      showBotComments: () => true,
    },
  });

  return (
    <AppCtx value={context}>
      <DetailView />
    </AppCtx>
  );
}

describe("DetailView layout", () => {
  test("moving selection between thread cards does not shift their rows", async () => {
    const { renderOnce, captureCharFrame, mockInput } = await testRender(
      () => <DetailViewWithContext />,
      { width: 120, height: 24 },
    );

    await renderOnce();

    mockInput.pressKey("j");
    await renderOnce();
    const firstFrameLines = captureCharFrame().split("\n");
    const fooIndexWithFirstSelection = findLineIndex(firstFrameLines, "src/foo.ts:10");
    const barIndexWithFirstSelection = findLineIndex(firstFrameLines, "src/bar.ts:20");

    mockInput.pressKey("j");
    await renderOnce();
    const secondFrameLines = captureCharFrame().split("\n");
    const fooIndexWithSecondSelection = findLineIndex(secondFrameLines, "src/foo.ts:10");
    const barIndexWithSecondSelection = findLineIndex(secondFrameLines, "src/bar.ts:20");

    expect(fooIndexWithFirstSelection).toBeGreaterThan(-1);
    expect(barIndexWithFirstSelection).toBeGreaterThan(-1);
    expect(fooIndexWithSecondSelection).toBe(fooIndexWithFirstSelection);
    expect(barIndexWithSecondSelection).toBe(barIndexWithFirstSelection);
  });
});
