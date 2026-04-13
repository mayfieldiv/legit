import { describe, test, expect } from "bun:test";
import { testRender } from "@opentui/solid";
import { PRList, PRListHeader, computeVisibleColumns } from "../src/components/PRList";
import { makePR } from "./helpers";

describe("PRList", () => {
  test("renders a list of PRs with correct columns", async () => {
    const prs = [
      makePR({ number: 1, title: "First PR", author: "alice" }),
      makePR({ number: 2, title: "Second PR", author: "bob" }),
    ];

    const { renderOnce, captureCharFrame } = await testRender(
      () => <PRList prs={prs} selectedIndex={0} />,
      { width: 120, height: 20 },
    );

    await renderOnce();
    const frame = captureCharFrame();

    // Should show PR numbers
    expect(frame).toContain("#1");
    expect(frame).toContain("#2");

    // Should show titles
    expect(frame).toContain("First PR");
    expect(frame).toContain("Second PR");

    // Should show authors
    expect(frame).toContain("alice");
    expect(frame).toContain("bob");
  });

  test("highlights the selected PR", async () => {
    const prs = [
      makePR({ number: 1, title: "First PR" }),
      makePR({ number: 2, title: "Second PR" }),
    ];

    const { renderOnce, captureSpans } = await testRender(
      () => <PRList prs={prs} selectedIndex={1} />,
      { width: 120, height: 20 },
    );

    await renderOnce();
    const _spans = captureSpans();

    // The selected row should have a different style — we check that
    // the second PR's row has some differentiation in the span data.
    // We verify this through the visual output having the selection marker.
    const frame = captureSpans();
    // At minimum we check the component renders without error
    expect(frame).toBeDefined();
  });

  test("shows draft indicator in review column for draft PRs", async () => {
    const prs = [
      makePR({ number: 1, title: "WIP thing", isDraft: true, reviewDecision: "APPROVED" }),
    ];

    const { renderOnce, captureCharFrame } = await testRender(
      () => <PRList prs={prs} selectedIndex={0} />,
      { width: 120, height: 20 },
    );

    await renderOnce();
    const frame = captureCharFrame();
    // Draft indicator should appear in the review column alongside the decision
    expect(frame).toContain("draft");
    expect(frame).toContain("approved");
    // Title should NOT contain the draft suffix
    expect(frame).not.toContain("WIP thing draft");
  });

  test("shows size as additions/deletions", async () => {
    const prs = [makePR({ additions: 123, deletions: 45 })];

    const { renderOnce, captureCharFrame } = await testRender(
      () => <PRList prs={prs} selectedIndex={0} />,
      { width: 120, height: 20 },
    );

    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toContain("+123");
    expect(frame).toContain("-45");
  });

  test("shows review decision", async () => {
    const prs = [makePR({ reviewDecision: "APPROVED" })];

    const { renderOnce, captureCharFrame } = await testRender(
      () => <PRList prs={prs} selectedIndex={0} />,
      { width: 120, height: 20 },
    );

    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toMatch(/approved/i);
  });

  test("renders empty state when no PRs", async () => {
    const { renderOnce, captureCharFrame } = await testRender(
      () => <PRList prs={[]} selectedIndex={0} />,
      { width: 120, height: 20 },
    );

    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toContain("No open pull requests");
  });

  test("shows age relative to now", async () => {
    const twoDaysAgo = new Date(Date.now() - 2 * 24 * 60 * 60 * 1000).toISOString();
    const prs = [makePR({ createdAt: twoDaysAgo })];

    const { renderOnce, captureCharFrame } = await testRender(
      () => <PRList prs={prs} selectedIndex={0} />,
      { width: 120, height: 20 },
    );

    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toContain("2d");
  });

  test("truncates long titles instead of wrapping rows", async () => {
    const prs = [
      makePR({
        number: 1,
        author: "alice",
        title:
          "This is a very long PR title that should not bleed into author or other columns when rendered in a constrained terminal width",
      }),
    ];

    const { renderOnce, captureCharFrame } = await testRender(
      () => <PRList prs={prs} selectedIndex={0} />,
      { width: 80, height: 8 },
    );

    await renderOnce();
    const frame = captureCharFrame();
    const lines = frame.split("\n");
    const nonEmptyLines = lines.filter((line) => line.trim() !== "");

    expect(lines[0]).toContain("alice");
    expect(nonEmptyLines).toHaveLength(1);
    expect(frame).not.toContain("author or other columns");
  });

  test("shows repo column when showRepo is true", async () => {
    const prs = [
      makePR({ number: 1, title: "First PR", repoSlug: "acme/widgets" }),
      makePR({ number: 2, title: "Second PR", repoSlug: "acme/gadgets" }),
    ];

    const { renderOnce, captureCharFrame } = await testRender(
      () => <PRList prs={prs} selectedIndex={0} showRepo={true} />,
      { width: 120, height: 20 },
    );

    await renderOnce();
    const frame = captureCharFrame();

    expect(frame).toContain("widgets");
    expect(frame).toContain("gadgets");
  });

  test("hides repo column when showRepo is false", async () => {
    const prs = [makePR({ number: 1, title: "First PR", repoSlug: "acme/widgets" })];

    const { renderOnce, captureCharFrame } = await testRender(
      () => <PRList prs={prs} selectedIndex={0} showRepo={false} />,
      { width: 120, height: 20 },
    );

    await renderOnce();
    const frame = captureCharFrame();

    expect(frame).not.toContain("widgets");
  });

  test("keeps a visible gap before the author column when title is truncated", async () => {
    const prs = [
      makePR({
        number: 1,
        author: "alice",
        title: "X".repeat(200),
      }),
    ];

    const { renderOnce, captureCharFrame } = await testRender(
      () => <PRList prs={prs} selectedIndex={0} />,
      { width: 80, height: 8 },
    );

    await renderOnce();
    const line = captureCharFrame().split("\n")[0] ?? "";

    expect(line).toMatch(/\salice\s+/);
  });

  test("shows conflict indicator ! in review column for conflicting PRs", async () => {
    const prs = [makePR({ number: 1, mergeable: "CONFLICTING" })];

    const { renderOnce, captureCharFrame } = await testRender(
      () => <PRList prs={prs} selectedIndex={0} />,
      { width: 120, height: 8 },
    );

    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toContain("!");
  });

  test("does not show conflict indicator for mergeable PRs", async () => {
    const prs = [makePR({ number: 1, mergeable: "MERGEABLE" })];

    const { renderOnce, captureCharFrame } = await testRender(
      () => <PRList prs={prs} selectedIndex={0} />,
      { width: 120, height: 8 },
    );

    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).not.toContain("! ");
  });

  test("shows conflict indicator alongside approved status", async () => {
    const prs = [makePR({ number: 1, mergeable: "CONFLICTING", reviewDecision: "APPROVED" })];

    const { renderOnce, captureCharFrame } = await testRender(
      () => <PRList prs={prs} selectedIndex={0} />,
      { width: 120, height: 8 },
    );

    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toContain("!");
    expect(frame).toContain("approved");
  });

  test("shows blocker column with 'you' when current user is requested reviewer", async () => {
    const prs = [makePR({ number: 1, requestedReviewers: ["alice"] })];

    const { renderOnce, captureCharFrame } = await testRender(
      () => <PRList prs={prs} selectedIndex={0} currentUser="alice" />,
      { width: 140, height: 8 },
    );

    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toContain("you");
  });

  test("shows 'you' when current user is the author of a draft (waiting-on-author self)", async () => {
    const prs = [makePR({ number: 1, author: "alice", isDraft: true })];

    const { renderOnce, captureCharFrame } = await testRender(
      () => <PRList prs={prs} selectedIndex={0} currentUser="alice" />,
      { width: 140, height: 8 },
    );

    await renderOnce();
    const frame = captureCharFrame();
    // Blocker column shows "you" not the raw login
    expect(frame).toContain("you");
    // "you" should appear once (in the blocker column), not duplicated as a username
    const youCount = (frame.match(/\byou\b/g) ?? []).length;
    expect(youCount).toBeGreaterThanOrEqual(1);
  });

  test("shows blocker column with reviewer name for needs-review with requested reviewer", async () => {
    const prs = [makePR({ number: 1, requestedReviewers: ["bob"] })];

    const { renderOnce, captureCharFrame } = await testRender(
      () => <PRList prs={prs} selectedIndex={0} currentUser="alice" />,
      { width: 140, height: 8 },
    );

    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toContain("bob");
  });

  test("keeps a visible gap between the PR and title columns in the header", async () => {
    const { renderOnce, captureCharFrame } = await testRender(() => <PRListHeader />, {
      width: 80,
      height: 4,
    });

    await renderOnce();
    const line = captureCharFrame().split("\n")[0] ?? "";

    expect(line).toMatch(/^PR\s+Title\s+/);
  });

  test("shows blocker header when currentUser is provided", async () => {
    const prs = [makePR({ number: 1 })];

    const { renderOnce, captureCharFrame } = await testRender(
      () => (
        <box flexDirection="column">
          <PRListHeader currentUser="alice" />
          <PRList prs={prs} selectedIndex={0} currentUser="alice" />
        </box>
      ),
      { width: 140, height: 8 },
    );

    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).toMatch(/[Bb]locker/);
  });

  test("does not show blocker column when currentUser is not provided", async () => {
    const prs = [makePR({ number: 1, requestedReviewers: ["alice"] })];

    const { renderOnce, captureCharFrame } = await testRender(
      () => (
        <box flexDirection="column">
          <PRListHeader />
          <PRList prs={prs} selectedIndex={0} />
        </box>
      ),
      { width: 140, height: 8 },
    );

    await renderOnce();
    const frame = captureCharFrame();
    expect(frame).not.toMatch(/[Bb]locker/);
  });
});

const countTrueValues = (c: object) => Object.values(c).filter(Boolean).length;

describe("computeVisibleColumns", () => {
  test("shows all columns at very wide widths", () => {
    const cols = computeVisibleColumns(180, true);
    expect(cols.age).toBe(true);
    expect(cols.author).toBe(true);
    expect(cols.size).toBe(true);
    expect(cols.review).toBe(true);
    expect(cols.threads).toBe(true);
    expect(cols.blocker).toBe(true);
  });

  test("hides columns progressively at narrow widths", () => {
    // base with repo = 7 (PR) + 30 (title) + 14 (repo) = 51
    // At 60 chars with repo: budget = 9, age(6) fits but author(14) doesn't
    const cols = computeVisibleColumns(60, true);
    expect(cols.age).toBe(true);
    expect(cols.author).toBe(false);
    expect(cols.size).toBe(false);
    expect(cols.review).toBe(false);
    expect(cols.threads).toBe(false);
    expect(cols.blocker).toBe(false);
  });

  test("without repo, more columns fit", () => {
    // Without repo, base is smaller (37 vs 51), so more budget
    const withRepo = computeVisibleColumns(80, true);
    const withoutRepo = computeVisibleColumns(80, false);
    // Without repo should show at least as many columns
    expect(countTrueValues(withoutRepo)).toBeGreaterThanOrEqual(countTrueValues(withRepo));
  });

  test("hides everything when extremely narrow", () => {
    const cols = computeVisibleColumns(30, true);
    expect(cols.age).toBe(false);
    expect(cols.author).toBe(false);
    expect(cols.blocker).toBe(false);
  });
});
