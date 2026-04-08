import { describe, test, expect } from "bun:test";
import { testRender } from "@opentui/solid";
import { DetailView } from "../src/components/DetailView";
import type { PRDetail, CheckRun, FullReviewThread, IssueComment } from "../src/lib/types";
import { makePR } from "./helpers";

function makeDetail(overrides: Partial<PRDetail> = {}): PRDetail {
  return {
    ...makePR(),
    body: "## Summary\n\nThis fixes the **bug**.",
    ...overrides,
  };
}

async function renderDetail(
  props: Partial<Parameters<typeof DetailView>[0]> = {},
  width = 80,
  height = 40,
) {
  const defaults = {
    pr: makeDetail(),
    threads: [],
    comments: [],
    loading: false,
    showResolved: false,
    showBotComments: true,
  };
  const { renderOnce, captureCharFrame } = await testRender(
    () => <DetailView {...defaults} {...props} />,
    { width, height },
  );
  await renderOnce();
  return captureCharFrame();
}

describe("DetailView", () => {
  test("shows loading state when no PR loaded", async () => {
    const { renderOnce, captureCharFrame } = await testRender(
      () => (
        <DetailView
          pr={undefined}
          threads={[]}
          comments={[]}
          loading={true}
          showResolved={false}
          showBotComments={true}
        />
      ),
      { width: 80, height: 10 },
    );
    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toMatch(/loading/i);
  });

  test("renders PR header with number, title, and author", async () => {
    const frame = await renderDetail({
      pr: makeDetail({ number: 99, title: "Big refactor", author: "bob" }),
    });
    expect(frame).toContain("#99");
    expect(frame).toContain("Big refactor");
    expect(frame).toContain("bob");
  });

  test("renders PR description as markdown", async () => {
    const frame = await renderDetail({
      pr: makeDetail({ body: "## Heading\n\nSome text with `code`." }),
    });
    expect(frame).toContain("## Heading");
    expect(frame).toContain("Some text with");
    expect(frame).toContain("code");
  });

  test("shows 'No description' for empty body", async () => {
    const frame = await renderDetail({ pr: makeDetail({ body: "" }) });
    expect(frame).toContain("No description");
  });

  test("renders CI checks with pass/fail counts", async () => {
    const checks: CheckRun[] = [
      { name: "build", status: "completed", conclusion: "success" },
      { name: "lint", status: "completed", conclusion: "failure" },
      { name: "deploy", status: "in_progress", conclusion: null },
    ];
    const pr = { ...makeDetail(), checks } as any;
    const frame = await renderDetail({ pr });
    expect(frame).toContain("CI Checks");
    expect(frame).toContain("1/3 passed");
    expect(frame).toContain("1 failed");
    expect(frame).toContain("1 pending");
    expect(frame).toContain("build");
    expect(frame).toContain("lint");
    expect(frame).toContain("deploy");
  });

  test("does not render checks section when no checks", async () => {
    const frame = await renderDetail({ pr: makeDetail() });
    expect(frame).not.toContain("CI Checks");
  });

  test("renders draft badge", async () => {
    const frame = await renderDetail({ pr: makeDetail({ isDraft: true }) });
    expect(frame).toContain("draft");
  });

  test("renders size info", async () => {
    const frame = await renderDetail({
      pr: makeDetail({ additions: 100, deletions: 20 }),
    });
    expect(frame).toContain("+100");
    expect(frame).toContain("-20");
  });

  test("shows merge conflict indicator on branch line", async () => {
    const frame = await renderDetail({
      pr: makeDetail({ mergeable: "CONFLICTING", headRef: "feat", baseRef: "main" }),
    });
    expect(frame).toContain("! conflict");
  });

  test("shows mergeable status on branch line", async () => {
    const frame = await renderDetail({
      pr: makeDetail({ mergeable: "MERGEABLE", headRef: "feat", baseRef: "main" }),
    });
    expect(frame).toContain("mergeable");
  });

  test("shows unknown merge status on branch line", async () => {
    const frame = await renderDetail({
      pr: makeDetail({ mergeable: "UNKNOWN", headRef: "feat", baseRef: "main" }),
    });
    expect(frame).toContain("merge unknown");
  });

  // ── Review Threads ─────────────────────────────────────────────────────

  test("renders review threads with three-state labels (unreplied/awaiting/resolved)", async () => {
    const threads: FullReviewThread[] = [
      {
        id: "RT_1",
        isResolved: false,
        path: "src/foo.ts",
        line: 42,
        comments: [
          {
            id: "RC_1",
            author: "bob",
            body: "Needs a null check here",
            createdAt: "2026-03-10T00:00:00Z",
            url: "https://github.com/acme/widgets/pull/42#discussion_r1",
            isBot: false,
          },
          {
            id: "RC_2",
            author: "alice",
            body: "Good catch, fixing",
            createdAt: "2026-03-11T00:00:00Z",
            url: "https://github.com/acme/widgets/pull/42#discussion_r2",
            isBot: false,
          },
        ],
      },
    ];
    const frame = await renderDetail({ threads });
    expect(frame).toContain("Review Threads");
    expect(frame).toContain("1 shown");
    expect(frame).toContain("src/foo.ts:42");
    expect(frame).toContain("awaiting reviewer");
    expect(frame).toContain("bob");
    expect(frame).toContain("Needs a null check here");
    expect(frame).toContain("alice");
    expect(frame).toContain("Good catch, fixing");
  });

  test("hides resolved threads by default", async () => {
    const threads: FullReviewThread[] = [
      {
        id: "RT_1",
        isResolved: true,
        path: "src/bar.ts",
        line: 10,
        comments: [
          {
            id: "RC_1",
            author: "bob",
            body: "This was fixed",
            createdAt: "2026-03-10T00:00:00Z",
            url: "https://example.com",
            isBot: false,
          },
        ],
      },
    ];
    const frame = await renderDetail({ threads, showResolved: false });
    expect(frame).toContain("Review Threads");
    expect(frame).toContain("1 hidden");
    expect(frame).toContain("All threads resolved or hidden");
    expect(frame).not.toContain("This was fixed");
  });

  test("shows resolved threads when showResolved is true", async () => {
    const threads: FullReviewThread[] = [
      {
        id: "RT_1",
        isResolved: true,
        path: "src/bar.ts",
        line: null,
        comments: [
          {
            id: "RC_1",
            author: "bob",
            body: "This was fixed",
            createdAt: "2026-03-10T00:00:00Z",
            url: "https://example.com",
            isBot: false,
          },
        ],
      },
    ];
    const frame = await renderDetail({ threads, showResolved: true });
    expect(frame).toContain("resolved");
    expect(frame).toContain("This was fixed");
    expect(frame).toContain("src/bar.ts");
  });

  test("hides bot-only threads when showBotComments is false", async () => {
    const threads: FullReviewThread[] = [
      {
        id: "RT_1",
        isResolved: false,
        path: "src/bot.ts",
        line: 1,
        comments: [
          {
            id: "RC_1",
            author: "copilot[bot]",
            body: "Suggestion: refactor",
            createdAt: "2026-03-10T00:00:00Z",
            url: "https://example.com",
            isBot: true,
          },
        ],
      },
    ];
    const frame = await renderDetail({ threads, showBotComments: false });
    expect(frame).toContain("1 hidden");
    expect(frame).not.toContain("Suggestion: refactor");
  });

  test("does not render threads section when no threads exist", async () => {
    const frame = await renderDetail({ threads: [] });
    expect(frame).not.toContain("Review Threads");
  });

  test("shows file path without line when line is null", async () => {
    const threads: FullReviewThread[] = [
      {
        id: "RT_1",
        isResolved: false,
        path: "README.md",
        line: null,
        comments: [
          {
            id: "RC_1",
            author: "bob",
            body: "Update readme",
            createdAt: "2026-03-10T00:00:00Z",
            url: "https://example.com",
            isBot: false,
          },
        ],
      },
    ];
    const frame = await renderDetail({ threads });
    expect(frame).toContain("README.md");
    expect(frame).not.toContain("README.md:");
  });

  // ── Conversation ────────────────────────────────────────────────────────

  test("renders issue comments in conversation section", async () => {
    const comments: IssueComment[] = [
      {
        id: 100,
        author: "alice",
        body: "Looks good overall",
        createdAt: "2026-03-10T00:00:00Z",
        url: "https://github.com/acme/widgets/pull/42#issuecomment-100",
        isBot: false,
      },
      {
        id: 101,
        author: "bob",
        body: "Thanks for the review",
        createdAt: "2026-03-11T00:00:00Z",
        url: "https://github.com/acme/widgets/pull/42#issuecomment-101",
        isBot: false,
      },
    ];
    const frame = await renderDetail({ comments });
    expect(frame).toContain("Conversation");
    expect(frame).toContain("2 comments");
    expect(frame).toContain("alice");
    expect(frame).toContain("Looks good overall");
    expect(frame).toContain("bob");
    expect(frame).toContain("Thanks for the review");
  });

  test("hides bot comments when showBotComments is false", async () => {
    const comments: IssueComment[] = [
      {
        id: 100,
        author: "alice",
        body: "Human comment",
        createdAt: "2026-03-10T00:00:00Z",
        url: "https://example.com",
        isBot: false,
      },
      {
        id: 101,
        author: "github-actions[bot]",
        body: "Bot comment",
        createdAt: "2026-03-10T00:00:00Z",
        url: "https://example.com",
        isBot: true,
      },
    ];
    const frame = await renderDetail({ comments, showBotComments: false });
    expect(frame).toContain("1 comment");
    expect(frame).toContain("Human comment");
    expect(frame).not.toContain("Bot comment");
  });

  test("shows bot badge on bot comments", async () => {
    const comments: IssueComment[] = [
      {
        id: 100,
        author: "devin-ai[bot]",
        body: "Auto summary",
        createdAt: "2026-03-10T00:00:00Z",
        url: "https://example.com",
        isBot: true,
      },
    ];
    const frame = await renderDetail({ comments, showBotComments: true });
    expect(frame).toContain("[bot]");
    expect(frame).toContain("devin-ai[bot]");
  });

  test("does not render conversation section when no comments exist", async () => {
    const frame = await renderDetail({ comments: [] });
    expect(frame).not.toContain("Conversation");
  });

  test("singular 'comment' label for 1 comment", async () => {
    const comments: IssueComment[] = [
      {
        id: 100,
        author: "alice",
        body: "Single comment",
        createdAt: "2026-03-10T00:00:00Z",
        url: "https://example.com",
        isBot: false,
      },
    ];
    const frame = await renderDetail({ comments });
    expect(frame).toContain("1 comment");
    expect(frame).not.toContain("1 comments");
  });

  // ── Keybindings ────────────────────────────────────────────────────────

  test("Escape calls onExit", async () => {
    let exited = false;
    const { renderOnce, mockInput } = await testRender(
      () => (
        <DetailView
          pr={makeDetail()}
          threads={[]}
          comments={[]}
          loading={false}
          showResolved={false}
          showBotComments={true}
          onExit={() => {
            exited = true;
          }}
        />
      ),
      { width: 80, height: 20 },
    );
    await renderOnce();
    mockInput.pressEscape();
    await new Promise((r) => setTimeout(r, 20));
    await renderOnce();
    expect(exited).toBe(true);
  });

  test("t calls onToggleResolved", async () => {
    let toggled = false;
    const { renderOnce, mockInput } = await testRender(
      () => (
        <DetailView
          pr={makeDetail()}
          threads={[]}
          comments={[]}
          loading={false}
          showResolved={false}
          showBotComments={true}
          onToggleResolved={() => {
            toggled = true;
          }}
        />
      ),
      { width: 80, height: 20 },
    );
    await renderOnce();
    mockInput.pressKey("t");
    await renderOnce();
    expect(toggled).toBe(true);
  });

  test("b calls onToggleBotComments", async () => {
    let toggled = false;
    const { renderOnce, mockInput } = await testRender(
      () => (
        <DetailView
          pr={makeDetail()}
          threads={[]}
          comments={[]}
          loading={false}
          showResolved={false}
          showBotComments={true}
          onToggleBotComments={() => {
            toggled = true;
          }}
        />
      ),
      { width: 80, height: 20 },
    );
    await renderOnce();
    mockInput.pressKey("b");
    await renderOnce();
    expect(toggled).toBe(true);
  });

  test("o calls onOpenInBrowser", async () => {
    let opened = false;
    const { renderOnce, mockInput } = await testRender(
      () => (
        <DetailView
          pr={makeDetail()}
          threads={[]}
          comments={[]}
          loading={false}
          showResolved={false}
          showBotComments={true}
          onOpenInBrowser={() => {
            opened = true;
          }}
        />
      ),
      { width: 80, height: 20 },
    );
    await renderOnce();
    mockInput.pressKey("o");
    await renderOnce();
    expect(opened).toBe(true);
  });

  test("r calls onRefresh", async () => {
    let refreshed = false;
    const { renderOnce, mockInput } = await testRender(
      () => (
        <DetailView
          pr={makeDetail()}
          threads={[]}
          comments={[]}
          loading={false}
          showResolved={false}
          showBotComments={true}
          onRefresh={() => {
            refreshed = true;
          }}
        />
      ),
      { width: 80, height: 20 },
    );
    await renderOnce();
    mockInput.pressKey("r");
    await renderOnce();
    expect(refreshed).toBe(true);
  });

  test("shows status bar with keybinding hints", async () => {
    const frame = await renderDetail();
    expect(frame).toContain("Esc");
    expect(frame).toContain("o GitHub");
    expect(frame).toContain("r refresh");
    expect(frame).toContain("show resolved");
  });

  test("status bar shows 'hide resolved' when showResolved is true", async () => {
    const frame = await renderDetail({ showResolved: true });
    expect(frame).toContain("hide resolved");
  });

  test("status bar shows 'hide bots' when showBotComments is true", async () => {
    const frame = await renderDetail({ showBotComments: true }, 100);
    expect(frame).toContain("hide bots");
  });

  test("status bar shows 'show bots' when showBotComments is false", async () => {
    const frame = await renderDetail({ showBotComments: false }, 100);
    expect(frame).toContain("show bots");
  });

  test("status bar shows j/k navigate hint", async () => {
    const frame = await renderDetail();
    expect(frame).toContain("j/k nav");
  });

  // ── Focus selection ─────────────────────────────────────────────────────

  const sampleThreads: FullReviewThread[] = [
    {
      id: "RT_1",
      isResolved: false,
      path: "src/foo.ts",
      line: 42,
      comments: [
        {
          id: "RC_1",
          author: "bob",
          body: "Thread one comment",
          createdAt: "2026-03-10T00:00:00Z",
          url: "https://github.com/acme/widgets/pull/42#discussion_r1",
          isBot: false,
        },
      ],
    },
    {
      id: "RT_2",
      isResolved: false,
      path: "src/bar.ts",
      line: 10,
      comments: [
        {
          id: "RC_2",
          author: "alice",
          body: "Thread two comment",
          createdAt: "2026-03-10T00:00:00Z",
          url: "https://github.com/acme/widgets/pull/42#discussion_r2",
          isBot: false,
        },
      ],
    },
  ];

  const sampleComments: IssueComment[] = [
    {
      id: 100,
      author: "alice",
      body: "Issue comment one",
      createdAt: "2026-03-10T00:00:00Z",
      url: "https://github.com/acme/widgets/pull/42#issuecomment-100",
      isBot: false,
    },
  ];

  test("j moves focus to first thread and shows border", async () => {
    const { renderOnce, captureCharFrame, mockInput } = await testRender(
      () => (
        <DetailView
          pr={makeDetail()}
          threads={sampleThreads}
          comments={[]}
          loading={false}
          showResolved={false}
          showBotComments={true}
        />
      ),
      { width: 80, height: 60 },
    );
    await renderOnce();

    // No focus initially — no border visible
    let frame = captureCharFrame();
    expect(frame).not.toContain("╭");

    // Press j to focus first item
    mockInput.pressKey("j");
    await renderOnce();
    frame = captureCharFrame();
    // Rounded border should appear
    expect(frame).toContain("╭");
    expect(frame).toContain("╰");
  });

  test("k from first item unfocuses (index -1)", async () => {
    const { renderOnce, captureCharFrame, mockInput } = await testRender(
      () => (
        <DetailView
          pr={makeDetail()}
          threads={sampleThreads}
          comments={[]}
          loading={false}
          showResolved={false}
          showBotComments={true}
        />
      ),
      { width: 80, height: 60 },
    );
    await renderOnce();

    // Focus first, then move back up
    mockInput.pressKey("j");
    await renderOnce();
    mockInput.pressKey("k");
    await renderOnce();

    const frame = captureCharFrame();
    expect(frame).not.toContain("╭");
  });

  test("j navigates through threads then to comments", async () => {
    const { renderOnce, captureCharFrame, mockInput } = await testRender(
      () => (
        <DetailView
          pr={makeDetail()}
          threads={sampleThreads}
          comments={sampleComments}
          loading={false}
          showResolved={false}
          showBotComments={true}
        />
      ),
      { width: 80, height: 80 },
    );
    await renderOnce();

    // j → focus thread 0 (src/foo.ts)
    mockInput.pressKey("j");
    await renderOnce();
    let frame = captureCharFrame();
    // The focused card contains src/foo.ts — we can't easily tell which card
    // has the border, but the border should be present
    expect(frame).toContain("╭");

    // j → focus thread 1 (src/bar.ts)
    mockInput.pressKey("j");
    await renderOnce();
    frame = captureCharFrame();
    // Still has a border (now on thread 2)
    expect(frame).toContain("╭");

    // j → focus comment 0 ("Issue comment one")
    mockInput.pressKey("j");
    await renderOnce();
    frame = captureCharFrame();
    expect(frame).toContain("╭");
    expect(frame).toContain("Issue comment one");
  });

  test("j stops at last item", async () => {
    const { renderOnce, captureCharFrame, mockInput } = await testRender(
      () => (
        <DetailView
          pr={makeDetail()}
          threads={[sampleThreads[0]!]}
          comments={[]}
          loading={false}
          showResolved={false}
          showBotComments={true}
        />
      ),
      { width: 80, height: 60 },
    );
    await renderOnce();

    // Focus the only item, then try to go further
    mockInput.pressKey("j");
    await renderOnce();
    mockInput.pressKey("j");
    await renderOnce();
    mockInput.pressKey("j");
    await renderOnce();

    // Should still show border (stuck on last item)
    const frame = captureCharFrame();
    expect(frame).toContain("╭");
  });

  test("down arrow also navigates focus", async () => {
    const { renderOnce, captureCharFrame, mockInput } = await testRender(
      () => (
        <DetailView
          pr={makeDetail()}
          threads={sampleThreads}
          comments={[]}
          loading={false}
          showResolved={false}
          showBotComments={true}
        />
      ),
      { width: 80, height: 60 },
    );
    await renderOnce();
    mockInput.pressArrow("down");
    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toContain("╭");
  });

  test("up arrow navigates focus backward", async () => {
    let openedUrl = "";
    const { renderOnce, mockInput } = await testRender(
      () => (
        <DetailView
          pr={makeDetail()}
          threads={sampleThreads}
          comments={[]}
          loading={false}
          showResolved={false}
          showBotComments={true}
          onOpenUrl={(url: string) => {
            openedUrl = url;
          }}
        />
      ),
      { width: 80, height: 60 },
    );
    await renderOnce();

    // Navigate down two, then up one → back to first thread
    mockInput.pressKey("j");
    await renderOnce();
    mockInput.pressKey("j");
    await renderOnce();
    mockInput.pressArrow("up");
    await renderOnce();

    // Press o to verify we’re on the first thread (RT_1)
    mockInput.pressKey("o");
    await renderOnce();
    expect(openedUrl).toBe("https://github.com/acme/widgets/pull/42#discussion_r1");
  });

  test("o opens focused item URL via onOpenUrl", async () => {
    let openedUrl = "";
    let prOpened = false;
    const { renderOnce, mockInput } = await testRender(
      () => (
        <DetailView
          pr={makeDetail()}
          threads={sampleThreads}
          comments={[]}
          loading={false}
          showResolved={false}
          showBotComments={true}
          onOpenInBrowser={() => {
            prOpened = true;
          }}
          onOpenUrl={(url: string) => {
            openedUrl = url;
          }}
        />
      ),
      { width: 80, height: 60 },
    );
    await renderOnce();

    // Focus first thread, then press o
    mockInput.pressKey("j");
    await renderOnce();
    mockInput.pressKey("o");
    await renderOnce();

    expect(openedUrl).toBe("https://github.com/acme/widgets/pull/42#discussion_r1");
    expect(prOpened).toBe(false);
  });

  test("o opens PR in browser when nothing is focused", async () => {
    let prOpened = false;
    let openedUrl = "";
    const { renderOnce, mockInput } = await testRender(
      () => (
        <DetailView
          pr={makeDetail()}
          threads={sampleThreads}
          comments={[]}
          loading={false}
          showResolved={false}
          showBotComments={true}
          onOpenInBrowser={() => {
            prOpened = true;
          }}
          onOpenUrl={(url: string) => {
            openedUrl = url;
          }}
        />
      ),
      { width: 80, height: 60 },
    );
    await renderOnce();

    // No focus navigation — press o directly
    mockInput.pressKey("o");
    await renderOnce();

    expect(prOpened).toBe(true);
    expect(openedUrl).toBe("");
  });

  test("o on second thread opens its URL", async () => {
    let openedUrl = "";
    const { renderOnce, mockInput } = await testRender(
      () => (
        <DetailView
          pr={makeDetail()}
          threads={sampleThreads}
          comments={sampleComments}
          loading={false}
          showResolved={false}
          showBotComments={true}
          onOpenUrl={(url: string) => {
            openedUrl = url;
          }}
        />
      ),
      { width: 80, height: 80 },
    );
    await renderOnce();

    // j → thread 0, j → thread 1, o
    mockInput.pressKey("j");
    await renderOnce();
    mockInput.pressKey("j");
    await renderOnce();
    mockInput.pressKey("o");
    await renderOnce();

    expect(openedUrl).toBe("https://github.com/acme/widgets/pull/42#discussion_r2");
  });

  test("o on issue comment opens its URL", async () => {
    let openedUrl = "";
    const { renderOnce, mockInput } = await testRender(
      () => (
        <DetailView
          pr={makeDetail()}
          threads={sampleThreads}
          comments={sampleComments}
          loading={false}
          showResolved={false}
          showBotComments={true}
          onOpenUrl={(url: string) => {
            openedUrl = url;
          }}
        />
      ),
      { width: 80, height: 80 },
    );
    await renderOnce();

    // j → thread 0, j → thread 1, j → comment 0, o
    mockInput.pressKey("j");
    await renderOnce();
    mockInput.pressKey("j");
    await renderOnce();
    mockInput.pressKey("j");
    await renderOnce();
    mockInput.pressKey("o");
    await renderOnce();

    expect(openedUrl).toBe("https://github.com/acme/widgets/pull/42#issuecomment-100");
  });

  test("mouse click focuses a comment card", async () => {
    const { renderOnce, captureCharFrame, mockMouse } = await testRender(
      () => (
        <DetailView
          pr={makeDetail({ body: "" })}
          threads={[]}
          comments={sampleComments}
          loading={false}
          showResolved={false}
          showBotComments={true}
        />
      ),
      { width: 80, height: 30 },
    );
    await renderOnce();

    // Comment card content is at rows 8–9 (inside scrollbox).
    // Click inside the card area.
    await mockMouse.click(10, 8);
    await renderOnce();

    const frame = captureCharFrame();
    // After clicking, the border should appear
    expect(frame).toContain("╭");
  });

  test("only one item has a visible border at a time", async () => {
    const { renderOnce, captureCharFrame, mockInput } = await testRender(
      () => (
        <DetailView
          pr={makeDetail()}
          threads={sampleThreads}
          comments={[]}
          loading={false}
          showResolved={false}
          showBotComments={true}
        />
      ),
      { width: 80, height: 60 },
    );
    await renderOnce();

    // Focus first thread
    mockInput.pressKey("j");
    await renderOnce();

    const frame = captureCharFrame();
    // Count border corners — should have exactly one top-left and one bottom-left
    const topLeftCount = (frame.match(/╭/g) || []).length;
    const bottomLeftCount = (frame.match(/╰/g) || []).length;
    expect(topLeftCount).toBe(1);
    expect(bottomLeftCount).toBe(1);
  });

  test("Enter toggles all details in focused card", async () => {
    const commentWithDetails: IssueComment[] = [
      {
        id: 1,
        author: "alice",
        body: [
          "<details>",
          "<summary>First</summary>",
          "",
          "Content A.",
          "",
          "</details>",
          "",
          "<details>",
          "<summary>Second</summary>",
          "",
          "Content B.",
          "",
          "</details>",
        ].join("\n"),
        createdAt: new Date().toISOString(),
        url: "https://github.com/test#issuecomment-1",
        isBot: false,
      },
    ];

    const { renderOnce, captureCharFrame, mockInput } = await testRender(
      () => (
        <DetailView
          pr={makeDetail({ body: "" })}
          threads={[]}
          comments={commentWithDetails}
          loading={false}
          showResolved={false}
          showBotComments={true}
        />
      ),
      { width: 80, height: 40 },
    );
    await renderOnce();

    // Both details collapsed initially
    let frame = captureCharFrame();
    expect(frame).toContain("\u25b6"); // collapsed arrow
    expect(frame).not.toContain("Content A.");
    expect(frame).not.toContain("Content B.");

    // Focus the comment card
    mockInput.pressKey("j");
    await renderOnce();

    // Press Enter to expand all details
    mockInput.pressEnter();
    await renderOnce();
    frame = captureCharFrame();
    expect(frame).toContain("Content A.");
    expect(frame).toContain("Content B.");
    expect(frame).toContain("\u25bc"); // expanded arrow

    // Press Enter again to collapse all
    mockInput.pressEnter();
    await renderOnce();
    frame = captureCharFrame();
    expect(frame).not.toContain("Content A.");
    expect(frame).not.toContain("Content B.");
    expect(frame).toContain("\u25b6"); // collapsed again
  });
});
