import { describe, test, expect } from "bun:test";
import { processPRList } from "../src/lib/group-filter-engine";
import { derivePRState } from "../src/lib/pr-state";
import { makePR } from "./helpers";

// ── helpers ──────────────────────────────────────────────────────────────────

function labels(result: ReturnType<typeof processPRList>): string[] {
  return result.groups.map((g) => g.label);
}

// ── groupBy: none (flat list) ────────────────────────────────────────────────

describe("processPRList — groupBy: none", () => {
  test("empty input returns empty groups", () => {
    const result = processPRList([], { groupBy: "none" });
    expect(result.groups).toHaveLength(0);
    expect(result.totalMatched).toBe(0);
  });

  test("returns a single group with all PRs", () => {
    const prs = [makePR({ number: 1 }), makePR({ number: 2 }), makePR({ number: 3 })];
    const result = processPRList(prs, { groupBy: "none" });
    expect(result.groups).toHaveLength(1);
    expect(result.groups[0]!.prs).toHaveLength(3);
    expect(result.totalMatched).toBe(3);
  });

  test("single group has empty key and empty label", () => {
    const result = processPRList([makePR()], { groupBy: "none" });
    expect(result.groups[0]!.key).toBe("");
    expect(result.groups[0]!.label).toBe("");
  });

  test("default groupBy (undefined) behaves like none", () => {
    const prs = [makePR({ number: 1 }), makePR({ number: 2 })];
    const result = processPRList(prs);
    expect(result.groups).toHaveLength(1);
    expect(result.groups[0]!.prs).toHaveLength(2);
  });
});

// ── groupBy: author ───────────────────────────────────────────────────────────

describe("processPRList — groupBy: author", () => {
  test("groups PRs by author login", () => {
    const prs = [
      makePR({ number: 1, author: "alice" }),
      makePR({ number: 2, author: "bob" }),
      makePR({ number: 3, author: "alice" }),
    ];
    const result = processPRList(prs, { groupBy: "author" });
    // Should produce one group per author
    const byAuthor = Object.fromEntries(result.groups.map((g) => [g.key, g.prs.length]));
    expect(byAuthor["alice"]).toBe(2);
    expect(byAuthor["bob"]).toBe(1);
  });

  test("group key and label match author login", () => {
    const result = processPRList([makePR({ author: "alice" })], { groupBy: "author" });
    expect(result.groups[0]!.key).toBe("alice");
    expect(result.groups[0]!.label).toBe("alice");
  });

  test("groups are sorted alphabetically by author", () => {
    const prs = [
      makePR({ number: 1, author: "zed" }),
      makePR({ number: 2, author: "alice" }),
      makePR({ number: 3, author: "bob" }),
    ];
    const result = processPRList(prs, { groupBy: "author" });
    expect(labels(result)).toEqual(["alice", "bob", "zed"]);
  });

  test("single author produces one group", () => {
    const prs = [makePR({ number: 1, author: "alice" }), makePR({ number: 2, author: "alice" })];
    const result = processPRList(prs, { groupBy: "author" });
    expect(result.groups).toHaveLength(1);
    expect(result.groups[0]!.prs).toHaveLength(2);
  });
});

// ── groupBy: repo ────────────────────────────────────────────────────────────

describe("processPRList — groupBy: repo", () => {
  test("groups PRs by repoSlug", () => {
    const prs = [
      makePR({ number: 1, repoSlug: "acme/web" }),
      makePR({ number: 2, repoSlug: "acme/api" }),
      makePR({ number: 3, repoSlug: "acme/web" }),
    ];
    const result = processPRList(prs, { groupBy: "repo" });
    const byRepo = Object.fromEntries(result.groups.map((g) => [g.key, g.prs.length]));
    expect(byRepo["acme/web"]).toBe(2);
    expect(byRepo["acme/api"]).toBe(1);
  });

  test("PRs with no repoSlug go to 'unknown' group", () => {
    const prs = [makePR({ number: 1, repoSlug: undefined })];
    const result = processPRList(prs, { groupBy: "repo" });
    expect(result.groups[0]!.key).toBe("unknown");
  });

  test("groups sorted alphabetically by slug", () => {
    const prs = [
      makePR({ number: 1, repoSlug: "z/repo" }),
      makePR({ number: 2, repoSlug: "a/repo" }),
    ];
    const result = processPRList(prs, { groupBy: "repo" });
    expect(labels(result)[0]).toBe("a/repo");
    expect(labels(result)[1]).toBe("z/repo");
  });
});

// ── groupBy: size-category ───────────────────────────────────────────────────

describe("processPRList — groupBy: size-category", () => {
  test("small: additions+deletions < 100", () => {
    const pr = makePR({ number: 1, additions: 40, deletions: 30 }); // 70
    const result = processPRList([pr], { groupBy: "size-category" });
    expect(result.groups[0]!.key).toBe("small");
  });

  test("medium: 100 to 500 inclusive", () => {
    const pr100 = makePR({ number: 1, additions: 60, deletions: 40 }); // 100
    const pr500 = makePR({ number: 2, additions: 300, deletions: 200 }); // 500
    const result = processPRList([pr100, pr500], { groupBy: "size-category" });
    const keys = result.groups.map((g) => g.key);
    expect(keys).toContain("medium");
    expect(keys).not.toContain("small");
    expect(keys).not.toContain("large");
  });

  test("large: additions+deletions > 500", () => {
    const pr = makePR({ number: 1, additions: 400, deletions: 200 }); // 600
    const result = processPRList([pr], { groupBy: "size-category" });
    expect(result.groups[0]!.key).toBe("large");
  });

  test("groups ordered small → medium → large", () => {
    const prs = [
      makePR({ number: 1, additions: 400, deletions: 200 }), // large
      makePR({ number: 2, additions: 50, deletions: 20 }), // small
      makePR({ number: 3, additions: 200, deletions: 100 }), // medium
    ];
    const result = processPRList(prs, { groupBy: "size-category" });
    expect(labels(result)).toEqual(["small", "medium", "large"]);
  });

  test("boundary: exactly 99 is small", () => {
    const pr = makePR({ additions: 60, deletions: 39 }); // 99
    const result = processPRList([pr], { groupBy: "size-category" });
    expect(result.groups[0]!.key).toBe("small");
  });

  test("boundary: exactly 100 is medium", () => {
    const pr = makePR({ additions: 60, deletions: 40 }); // 100
    const result = processPRList([pr], { groupBy: "size-category" });
    expect(result.groups[0]!.key).toBe("medium");
  });

  test("boundary: exactly 501 is large", () => {
    const pr = makePR({ additions: 301, deletions: 200 }); // 501
    const result = processPRList([pr], { groupBy: "size-category" });
    expect(result.groups[0]!.key).toBe("large");
  });
});

// ── groupBy: label ───────────────────────────────────────────────────────────

describe("processPRList — groupBy: label", () => {
  test("groups by first label", () => {
    const prs = [
      makePR({ number: 1, labels: ["bug"] }),
      makePR({ number: 2, labels: ["feature"] }),
      makePR({ number: 3, labels: ["bug", "urgent"] }),
    ];
    const result = processPRList(prs, { groupBy: "label" });
    const byLabel = Object.fromEntries(result.groups.map((g) => [g.key, g.prs.length]));
    expect(byLabel["bug"]).toBe(2);
    expect(byLabel["feature"]).toBe(1);
  });

  test("PRs with no labels go to Unlabeled group", () => {
    const pr = makePR({ number: 1, labels: [] });
    const result = processPRList([pr], { groupBy: "label" });
    expect(result.groups[0]!.key).toBe("unlabeled");
    expect(result.groups[0]!.label).toBe("Unlabeled");
  });

  test("groups sorted alphabetically, Unlabeled last", () => {
    const prs = [
      makePR({ number: 1, labels: [] }),
      makePR({ number: 2, labels: ["z-label"] }),
      makePR({ number: 3, labels: ["a-label"] }),
    ];
    const result = processPRList(prs, { groupBy: "label" });
    const ls = labels(result);
    expect(ls[0]).toBe("a-label");
    expect(ls[1]).toBe("z-label");
    expect(ls[2]).toBe("Unlabeled");
  });
});

// ── groupBy: smart-status ────────────────────────────────────────────────────

describe("processPRList — groupBy: smart-status", () => {
  test("groups by tier in priority order", () => {
    const prs = [
      // waiting-on-author: draft
      makePR({ number: 1, isDraft: true, author: "alice" }),
      // needs-review (no specific reviewer)
      makePR({ number: 2 }),
      // me-blocking: I'm requested reviewer
      makePR({ number: 3, requestedReviewers: ["me"] }),
      // needs-review (specific reviewer requested, but still needs-review tier)
      makePR({ number: 4, requestedReviewers: ["bob"] }),
    ];
    const result = processPRList(prs, { groupBy: "smart-status", currentUser: "me" });
    const ls = labels(result);
    expect(ls[0]).toBe("Me blocking");
    expect(ls[1]).toBe("Needs review");
    expect(ls[2]).toBe("Waiting on author");
    // PRs 2 and 4 both go to Needs review
    expect(result.groups[1]!.prs).toHaveLength(2);
  });

  test("empty tiers are omitted", () => {
    // Only needs-review PRs
    const prs = [makePR({ number: 1 }), makePR({ number: 2 })];
    const result = processPRList(prs, { groupBy: "smart-status", currentUser: "me" });
    expect(result.groups).toHaveLength(1);
    expect(result.groups[0]!.label).toBe("Needs review");
  });

  test("works without currentUser (treats all as needs-review)", () => {
    const prs = [makePR({ number: 1, requestedReviewers: ["me"] }), makePR({ number: 2 })];
    // No currentUser — can't compute me-blocking
    const result = processPRList(prs, { groupBy: "smart-status" });
    expect(result.groups.some((g) => g.label === "Me blocking")).toBe(false);
  });

  test("me-blocking PRs contain only those where currentUser is requested reviewer", () => {
    const prs = [
      makePR({ number: 1, requestedReviewers: ["me"] }),
      makePR({ number: 2, requestedReviewers: ["bob"] }),
    ];
    const result = processPRList(prs, { groupBy: "smart-status", currentUser: "me" });
    const meBlocking = result.groups.find((g) => g.label === "Me blocking");
    expect(meBlocking?.prs).toHaveLength(1);
    expect(meBlocking?.prs[0]!.number).toBe(1);
  });

  test("current user's APPROVED review moves PR out of needs-review", () => {
    const approvedByMe = makePR({ number: 1, author: "alice", requestedReviewers: ["bob"] });
    const untouched = makePR({ number: 2, author: "alice", requestedReviewers: ["carol"] });

    const result = processPRList([approvedByMe, untouched], {
      groupBy: "smart-status",
      currentUser: "me",
      getPRState: (pr) =>
        derivePRState(pr, {
          currentUser: "me",
          checks: [],
          threads: [],
          reviews: pr.number === 1 ? [{ user: "me", state: "APPROVED" }] : [],
        }),
    });

    expect(
      result.groups.find((g) => g.label === "Needs review")?.prs.map((pr) => pr.number),
    ).toEqual([2]);
    expect(
      result.groups.find((g) => g.label === "Waiting on author")?.prs.map((pr) => pr.number),
    ).toEqual([1]);
  });

  test("another reviewer's APPROVED review also moves PR out of needs-review", () => {
    const approved = makePR({ number: 1, author: "alice", requestedReviewers: ["bob"] });
    const untouched = makePR({ number: 2, author: "alice", requestedReviewers: ["carol"] });

    const result = processPRList([approved, untouched], {
      groupBy: "smart-status",
      currentUser: "me",
      getPRState: (pr) =>
        derivePRState(pr, {
          currentUser: "me",
          checks: [],
          threads: [],
          reviews: pr.number === 1 ? [{ user: "someone-else", state: "APPROVED" }] : [],
        }),
    });

    expect(
      result.groups.find((g) => g.label === "Needs review")?.prs.map((pr) => pr.number),
    ).toEqual([2]);
    expect(
      result.groups.find((g) => g.label === "Waiting on author")?.prs.map((pr) => pr.number),
    ).toEqual([1]);
  });
});

// ── filtering ────────────────────────────────────────────────────────────────

describe("processPRList — filtering", () => {
  test("empty filterText returns all PRs", () => {
    const prs = [makePR({ number: 1 }), makePR({ number: 2 })];
    const result = processPRList(prs, { filterText: "" });
    expect(result.totalMatched).toBe(2);
  });

  test("filters by title (case-insensitive)", () => {
    const prs = [
      makePR({ number: 1, title: "Fix memory leak" }),
      makePR({ number: 2, title: "Add new feature" }),
    ];
    const result = processPRList(prs, { filterText: "memory" });
    expect(result.totalMatched).toBe(1);
    expect(result.groups[0]!.prs[0]!.number).toBe(1);
  });

  test("filters by author login", () => {
    const prs = [makePR({ number: 1, author: "alice" }), makePR({ number: 2, author: "bob" })];
    const result = processPRList(prs, { filterText: "alice" });
    expect(result.totalMatched).toBe(1);
    expect(result.groups[0]!.prs[0]!.number).toBe(1);
  });

  test("filters by label (case-insensitive)", () => {
    const prs = [
      makePR({ number: 1, labels: ["bug", "urgent"] }),
      makePR({ number: 2, labels: ["feature"] }),
    ];
    const result = processPRList(prs, { filterText: "urgent" });
    expect(result.totalMatched).toBe(1);
    expect(result.groups[0]!.prs[0]!.number).toBe(1);
  });

  test("filters by requested reviewer", () => {
    const prs = [
      makePR({ number: 1, author: "user1", requestedReviewers: ["alice", "bob"] }),
      makePR({ number: 2, author: "user2", requestedReviewers: ["charlie"] }),
    ];
    const result = processPRList(prs, { filterText: "alice" });
    expect(result.totalMatched).toBe(1);
    expect(result.groups[0]!.prs[0]!.number).toBe(1);
  });

  test("filters by PR number as string", () => {
    const prs = [makePR({ number: 42 }), makePR({ number: 99 })];
    const result = processPRList(prs, { filterText: "42" });
    expect(result.totalMatched).toBe(1);
    expect(result.groups[0]!.prs[0]!.number).toBe(42);
  });

  test("filters by PR number with # prefix", () => {
    const prs = [makePR({ number: 42 }), makePR({ number: 99 })];
    const result = processPRList(prs, { filterText: "#42" });
    expect(result.totalMatched).toBe(1);
    expect(result.groups[0]!.prs[0]!.number).toBe(42);
  });

  test("#42 does not match title containing '42'", () => {
    const prs = [
      makePR({ number: 99, title: "Fix issue 42 bug" }),
      makePR({ number: 42, title: "Some other PR" }),
    ];
    // #42 should only match by number, not title
    const result = processPRList(prs, { filterText: "#42" });
    expect(result.totalMatched).toBe(1);
    expect(result.groups[0]!.prs[0]!.number).toBe(42);
  });

  test("no match returns empty groups and totalMatched=0", () => {
    const prs = [makePR({ number: 1, title: "Fix bug" })];
    const result = processPRList(prs, { filterText: "zzznomatch" });
    expect(result.totalMatched).toBe(0);
    expect(result.groups).toHaveLength(0);
  });

  test("filters by head branch name", () => {
    const prs = [
      makePR({ number: 1, headRef: "feature/login-page" }),
      makePR({ number: 2, headRef: "bugfix/typo" }),
    ];
    const result = processPRList(prs, { filterText: "login" });
    expect(result.totalMatched).toBe(1);
    expect(result.groups[0]!.prs[0]!.number).toBe(1);
  });

  test("filters by base branch name", () => {
    const prs = [makePR({ number: 1, baseRef: "main" }), makePR({ number: 2, baseRef: "develop" })];
    const result = processPRList(prs, { filterText: "develop" });
    expect(result.totalMatched).toBe(1);
    expect(result.groups[0]!.prs[0]!.number).toBe(2);
  });

  test("filters by repo slug", () => {
    const prs = [
      makePR({ number: 1, repoSlug: "acme/web" }),
      makePR({ number: 2, repoSlug: "acme/api" }),
    ];
    const result = processPRList(prs, { filterText: "web" });
    expect(result.totalMatched).toBe(1);
    expect(result.groups[0]!.prs[0]!.number).toBe(1);
  });

  test("filters by draft status", () => {
    const prs = [makePR({ number: 1, isDraft: true }), makePR({ number: 2, isDraft: false })];
    const result = processPRList(prs, { filterText: "draft" });
    expect(result.totalMatched).toBe(1);
    expect(result.groups[0]!.prs[0]!.number).toBe(1);
  });

  test("filters by conflict status", () => {
    const prs = [
      makePR({ number: 1, mergeable: "CONFLICTING" }),
      makePR({ number: 2, mergeable: "MERGEABLE" }),
    ];
    const result = processPRList(prs, { filterText: "conflict" });
    expect(result.totalMatched).toBe(1);
    expect(result.groups[0]!.prs[0]!.number).toBe(1);
  });

  test("filters by review decision", () => {
    const prs = [
      makePR({ number: 1, reviewDecision: "APPROVED" }),
      makePR({ number: 2, reviewDecision: "CHANGES_REQUESTED" }),
    ];
    const result = processPRList(prs, { filterText: "approved" });
    expect(result.totalMatched).toBe(1);
    expect(result.groups[0]!.prs[0]!.number).toBe(1);
  });

  test("filters by assignee", () => {
    const prs = [
      makePR({ number: 1, assignees: ["alice"] }),
      makePR({ number: 2, assignees: ["bob"] }),
    ];
    const result = processPRList(prs, { filterText: "bob" });
    expect(result.totalMatched).toBe(1);
    expect(result.groups[0]!.prs[0]!.number).toBe(2);
  });

  test("filter is trimmed", () => {
    const prs = [makePR({ number: 1, title: "Fix memory leak" })];
    const result = processPRList(prs, { filterText: "  memory  " });
    expect(result.totalMatched).toBe(1);
  });

  test("combined: filter + groupBy", () => {
    const prs = [
      makePR({ number: 1, author: "alice", title: "Fix bug" }),
      makePR({ number: 2, author: "bob", title: "Fix bug" }),
      makePR({ number: 3, author: "alice", title: "Add feature" }),
    ];
    const result = processPRList(prs, { filterText: "fix", groupBy: "author" });
    // Only PRs 1 and 2 match; grouped by author
    expect(result.totalMatched).toBe(2);
    const byAuthor = Object.fromEntries(result.groups.map((g) => [g.key, g.prs.length]));
    expect(byAuthor["alice"]).toBe(1);
    expect(byAuthor["bob"]).toBe(1);
  });
});

// ── sorting ───────────────────────────────────────────────────────────────────

describe("processPRList — sorting", () => {
  test("sortBy: size desc (largest first)", () => {
    const prs = [
      makePR({ number: 1, additions: 10, deletions: 5 }), // 15
      makePR({ number: 2, additions: 100, deletions: 50 }), // 150
      makePR({ number: 3, additions: 30, deletions: 20 }), // 50
    ];
    const result = processPRList(prs, { sortBy: "size", sortDir: "desc" });
    const numbers = result.groups[0]!.prs.map((p) => p.number);
    expect(numbers).toEqual([2, 3, 1]);
  });

  test("sortBy: size asc (smallest first)", () => {
    const prs = [
      makePR({ number: 1, additions: 100, deletions: 50 }), // 150
      makePR({ number: 2, additions: 10, deletions: 5 }), // 15
    ];
    const result = processPRList(prs, { sortBy: "size", sortDir: "asc" });
    const numbers = result.groups[0]!.prs.map((p) => p.number);
    expect(numbers).toEqual([2, 1]);
  });

  test("sortBy: age desc (newest first)", () => {
    const prs = [
      makePR({ number: 1, createdAt: "2026-01-01T00:00:00Z" }),
      makePR({ number: 2, createdAt: "2026-03-01T00:00:00Z" }),
      makePR({ number: 3, createdAt: "2026-02-01T00:00:00Z" }),
    ];
    const result = processPRList(prs, { sortBy: "age", sortDir: "desc" });
    const numbers = result.groups[0]!.prs.map((p) => p.number);
    expect(numbers).toEqual([2, 3, 1]);
  });

  test("sortBy: age asc (oldest first)", () => {
    const prs = [
      makePR({ number: 1, createdAt: "2026-03-01T00:00:00Z" }),
      makePR({ number: 2, createdAt: "2026-01-01T00:00:00Z" }),
    ];
    const result = processPRList(prs, { sortBy: "age", sortDir: "asc" });
    const numbers = result.groups[0]!.prs.map((p) => p.number);
    expect(numbers).toEqual([2, 1]);
  });

  test("sortBy: updated desc (most recently updated first)", () => {
    const prs = [
      makePR({ number: 1, updatedAt: "2026-01-01T00:00:00Z" }),
      makePR({ number: 2, updatedAt: "2026-03-01T00:00:00Z" }),
    ];
    const result = processPRList(prs, { sortBy: "updated", sortDir: "desc" });
    const numbers = result.groups[0]!.prs.map((p) => p.number);
    expect(numbers).toEqual([2, 1]);
  });

  test("sortBy: updated asc (least recently updated first)", () => {
    const prs = [
      makePR({ number: 1, updatedAt: "2026-03-01T00:00:00Z" }),
      makePR({ number: 2, updatedAt: "2026-01-01T00:00:00Z" }),
    ];
    const result = processPRList(prs, { sortBy: "updated", sortDir: "asc" });
    const numbers = result.groups[0]!.prs.map((p) => p.number);
    expect(numbers).toEqual([2, 1]);
  });

  test("sorting is applied within each group", () => {
    const prs = [
      makePR({ number: 1, author: "alice", additions: 10, deletions: 5 }), // small
      makePR({ number: 2, author: "alice", additions: 100, deletions: 50 }), // large
      makePR({ number: 3, author: "bob", additions: 30, deletions: 10 }), // medium
      makePR({ number: 4, author: "bob", additions: 5, deletions: 2 }), // tiny
    ];
    const result = processPRList(prs, {
      groupBy: "author",
      sortBy: "size",
      sortDir: "desc",
    });
    const aliceGroup = result.groups.find((g) => g.key === "alice")!;
    const bobGroup = result.groups.find((g) => g.key === "bob")!;
    expect(aliceGroup.prs.map((p) => p.number)).toEqual([2, 1]);
    expect(bobGroup.prs.map((p) => p.number)).toEqual([3, 4]);
  });

  test("no sortBy preserves original input order", () => {
    const prs = [makePR({ number: 3 }), makePR({ number: 1 }), makePR({ number: 2 })];
    const result = processPRList(prs);
    const numbers = result.groups[0]!.prs.map((p) => p.number);
    expect(numbers).toEqual([3, 1, 2]);
  });

  test("default sortDir is desc", () => {
    const prs = [
      makePR({ number: 1, additions: 10, deletions: 5 }),
      makePR({ number: 2, additions: 100, deletions: 50 }),
    ];
    // sortDir not specified — should default to desc (largest first)
    const result = processPRList(prs, { sortBy: "size" });
    const numbers = result.groups[0]!.prs.map((p) => p.number);
    expect(numbers).toEqual([2, 1]);
  });
});

// ── combined operations ──────────────────────────────────────────────────────

describe("processPRList — combined operations", () => {
  test("filter + sort + groupBy together", () => {
    const prs = [
      makePR({ number: 1, author: "alice", title: "Fix bug", additions: 100, deletions: 0 }),
      makePR({ number: 2, author: "bob", title: "Fix bug", additions: 10, deletions: 0 }),
      makePR({
        number: 3,
        author: "alice",
        title: "Add feature",
        additions: 50,
        deletions: 0,
      }),
    ];
    const result = processPRList(prs, {
      filterText: "fix",
      groupBy: "author",
      sortBy: "size",
      sortDir: "asc",
    });
    expect(result.totalMatched).toBe(2);
    // Groups should only contain filtered PRs
    const aliceGroup = result.groups.find((g) => g.key === "alice")!;
    expect(aliceGroup.prs).toHaveLength(1);
    expect(aliceGroup.prs[0]!.number).toBe(1);
  });

  test("totalMatched reflects filtered count, not grouped count", () => {
    const prs = [
      makePR({ number: 1, title: "Fix A" }),
      makePR({ number: 2, title: "Fix B" }),
      makePR({ number: 3, title: "Add C" }),
    ];
    const result = processPRList(prs, { filterText: "fix" });
    expect(result.totalMatched).toBe(2);
  });

  test("empty PRs with groupBy returns empty groups", () => {
    const result = processPRList([], { groupBy: "smart-status", currentUser: "me" });
    expect(result.groups).toHaveLength(0);
    expect(result.totalMatched).toBe(0);
  });
});

// ── totalMatched ─────────────────────────────────────────────────────────────

describe("processPRList — totalMatched", () => {
  test("totalMatched counts all PRs in all groups", () => {
    const prs = [
      makePR({ number: 1, author: "alice" }),
      makePR({ number: 2, author: "bob" }),
      makePR({ number: 3, author: "alice" }),
    ];
    const result = processPRList(prs, { groupBy: "author" });
    expect(result.totalMatched).toBe(3);
  });

  test("after filtering, totalMatched counts only matched PRs", () => {
    const prs = [
      makePR({ number: 1, title: "Fix bug" }),
      makePR({ number: 2, title: "Fix another" }),
      makePR({ number: 3, title: "New feature" }),
    ];
    const result = processPRList(prs, { filterText: "fix", groupBy: "author" });
    expect(result.totalMatched).toBe(2);
  });
});
