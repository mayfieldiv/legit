import { describe, test, expect } from "bun:test";
import { parseWorktreeList, expectedBranchForPR, matchWorktree } from "../src/lib/worktree";
import { sanitizeBranchForPath } from "../src/lib/legit";
import { makePR } from "./helpers";

describe("parseWorktreeList", () => {
  test("parses a single attached worktree on a branch", () => {
    const stdout = [
      "worktree /Users/me/src/widgets",
      "HEAD abc123def4567890abc123def4567890abc123de",
      "branch refs/heads/main",
      "",
    ].join("\n");
    expect(parseWorktreeList(stdout)).toEqual([
      {
        path: "/Users/me/src/widgets",
        head: "abc123def4567890abc123def4567890abc123de",
        branchRef: "refs/heads/main",
        branchName: "main",
        detached: false,
        bare: false,
      },
    ]);
  });

  test("parses multiple worktrees including detached, bare, locked, prunable", () => {
    const stdout = [
      "worktree /Users/me/src/widgets",
      "bare",
      "",
      "worktree /Users/me/.legit/worktrees/acme/widgets/1-foo",
      "HEAD deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
      "branch refs/heads/foo",
      "",
      "worktree /tmp/detached-head",
      "HEAD aabbccddaabbccddaabbccddaabbccddaabbccdd",
      "detached",
      "locked because I said so",
      "",
      "worktree /tmp/orphan",
      "HEAD 0000000000000000000000000000000000000000",
      "detached",
      "prunable gitdir file points to non-existent location",
      "",
    ].join("\n");
    const entries = parseWorktreeList(stdout);
    expect(entries).toHaveLength(4);
    expect(entries[0]!.bare).toBe(true);
    expect(entries[1]!.branchName).toBe("foo");
    expect(entries[2]!.detached).toBe(true);
    expect(entries[2]!.locked).toBe("because I said so");
    expect(entries[3]!.prunable).toBe("gitdir file points to non-existent location");
  });

  test("parses fork-named branch (gh pr checkout convention)", () => {
    const stdout = [
      "worktree /Users/me/.legit/worktrees/acme/widgets/42-patch-1",
      "HEAD abc123def4567890abc123def4567890abc123de",
      "branch refs/heads/contributor-patch-1",
      "",
    ].join("\n");
    const [entry] = parseWorktreeList(stdout);
    expect(entry!.branchName).toBe("contributor-patch-1");
  });

  test("ignores trailing newlines and empty records", () => {
    const stdout =
      "\n\nworktree /a\nHEAD 1111111111111111111111111111111111111111\nbranch refs/heads/x\n\n\n\n";
    expect(parseWorktreeList(stdout)).toHaveLength(1);
  });

  test("tolerates a bare `locked` line without a message", () => {
    const stdout = [
      "worktree /a",
      "HEAD " + "1".repeat(40),
      "branch refs/heads/x",
      "locked",
      "",
    ].join("\n");
    expect(parseWorktreeList(stdout)[0]!.locked).toBe("");
  });
});

describe("sanitizeBranchForPath", () => {
  test("replaces slashes with dashes", () => {
    expect(sanitizeBranchForPath("feature/login")).toBe("feature-login");
  });

  test("drops invalid characters", () => {
    expect(sanitizeBranchForPath("feat: cool!")).toBe("feat-cool");
  });

  test("collapses runs of dashes", () => {
    expect(sanitizeBranchForPath("a//b")).toBe("a-b");
  });

  test("preserves dots and underscores", () => {
    expect(sanitizeBranchForPath("release.v1_beta")).toBe("release.v1_beta");
  });

  test("caps at 80 chars", () => {
    const long = "a".repeat(200);
    expect(sanitizeBranchForPath(long).length).toBe(80);
  });

  test("strips leading and trailing dashes", () => {
    expect(sanitizeBranchForPath("/foo/")).toBe("foo");
  });
});

describe("expectedBranchForPR", () => {
  test("same-repo PR uses headRef", () => {
    const pr = makePR({ headRef: "feature/foo", headRepositoryOwner: "acme" });
    expect(expectedBranchForPR(pr, "acme")).toBe("feature/foo");
  });

  test("fork PR prefixes with fork owner", () => {
    const pr = makePR({ headRef: "patch-1", headRepositoryOwner: "contributor" });
    expect(expectedBranchForPR(pr, "acme")).toBe("contributor-patch-1");
  });

  test("deleted fork (empty headRepositoryOwner) falls back to headRef", () => {
    const pr = makePR({ headRef: "patch-1", headRepositoryOwner: "" });
    expect(expectedBranchForPR(pr, "acme")).toBe("patch-1");
  });
});

describe("matchWorktree", () => {
  const entries = [
    {
      path: "/some/other/place",
      head: "a".repeat(40),
      branchRef: "refs/heads/unrelated",
      branchName: "unrelated",
      detached: false,
      bare: false,
    },
    {
      path: "/Users/me/.legit/worktrees/acme/widgets/1-foo",
      head: "b".repeat(40),
      branchRef: "refs/heads/foo",
      branchName: "foo",
      detached: false,
      bare: false,
    },
  ];

  test("matches by branch name first", () => {
    const found = matchWorktree(entries, "foo", "/nonmatching/path");
    expect(found?.path).toBe("/Users/me/.legit/worktrees/acme/widgets/1-foo");
  });

  test("falls back to path when branch does not match", () => {
    const found = matchWorktree(
      entries,
      "ghost-branch",
      "/Users/me/.legit/worktrees/acme/widgets/1-foo",
    );
    expect(found?.branchName).toBe("foo");
  });

  test("returns undefined when neither matches", () => {
    expect(matchWorktree(entries, "ghost", "/nowhere")).toBeUndefined();
  });
});
