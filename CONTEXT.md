# legit

A terminal UI for working through your GitHub pull request queue across one or more repos. The primary value: telling you which PR you most need to act on next, surfacing worktree state, and getting you into review/respond/merge actions with single keypresses.

## Language

### PR & repos

**PR**:
A GitHub pull request as legit cares about it — number, title, author, branch refs, labels, requested reviewers, assignees, review decision, mergeability, lifecycle state.
_Avoid_: change request, MR.

**PR Detail**:
A PR plus its body (markdown), used in the detail view. Fetched lazily when the user drills in.

**Open PR List**:
The list of open PRs for the current Tracked Repo, plus the user's selection cursor and the scroll viewport that keeps the cursor on-screen as the list grows. Populated by REST streaming during a fetch; rendered as one row per PR in the list view.

**Repo Tab**:
A UI tab showing PRs from a single configured repo (or `All` showing every tracked repo combined).

**Source Clone**:
A local git clone of a repo, configured per-repo, from which legit creates worktrees. Without one, worktree features are unavailable for that repo.

**Tracked Repo**:
A repo present in `~/.legit/config.json` plus the repo detected from the CWD. The set of repos legit fetches PRs from.

### Blocker engine

**Smart-status** (alias: **Tier**):
The categorisation that drives sort order and grouping: `me-blocking`, `needs-review`, or `waiting-on-author`. Computed by the blocker engine from the PR, its reviews, its threads, and its CI checks.
_Avoid_: priority, severity, status (overloaded with GitHub's PR `state`).

**Blocker**:
The login of the person who must act next on a PR, or empty for `needs-review` with no specific reviewer. Distinct from PR author — for a `waiting-on-author` PR the blocker is the author; for a `me-blocking` PR the blocker is the current user.

**Effective Author**:
The current user when they are an assignee on a PR they did not author. The blocker engine treats them as "the author" for all `waiting-on-author` rules, modelling takeover of an in-flight PR.

### Reviews & comments

**Review Thread**:
An inline review comment thread on a specific file path (and usually a line). Has a `path`, `line`, `isResolved` flag, and an ordered list of comments. Threads are GitHub's first-class review unit; one review may produce many threads.

**Thread Classification** (for unresolved threads): one of

- `unreplied` — last non-bot comment is the thread starter's; author must respond.
- `awaiting-reviewer` — someone other than the thread starter spoke last; the reviewer must resolve or reply.
- `resolved` — closed; not blocking anyone.

**Issue Comment**:
A top-level comment on the PR conversation (not tied to a file/line). Distinct from review thread comments.

**Bot**:
A GitHub Bot account or any login matching `botLogins` in config or ending with `[bot]`. Bot comments are visually de-emphasised and filtered separately from human comments.

### State flags

**Mergeable**:
GitHub's flag for whether the PR can be merged: `MERGEABLE`, `CONFLICTING`, or `UNKNOWN`. `UNKNOWN` is GitHub computing lazily; legit retries once after a delay for open PRs.

**Review Decision**:
GitHub's aggregate: `APPROVED`, `CHANGES_REQUESTED`, `REVIEW_REQUIRED`, or empty.

**Lifecycle State** (the GitHub `state` field): `OPEN`, `MERGED`, `CLOSED`. The list endpoint only returns `OPEN`; the other two appear after a per-PR refresh detects a state change since the list was fetched.

### Worktree

**Worktree**:
A git worktree on disk under the user's `worktreeRoot` (defaulting to `~/.legit/worktrees/<owner>/<repo>/<number>-<branch>`), checked out to a PR's head branch by `gh pr checkout`. legit can create one for any PR whose repo has a configured `sourceClone`, and detects whether one already exists for any PR shown in the list/detail views.

**Expected Branch**:
The local branch name `gh pr checkout` would produce for a PR. Same-repo PRs keep `headRef` verbatim; fork PRs get prefixed with `<forkOwner>-` to avoid collisions across forks of the same branch name.

### Refresh

**Refresh**:
Re-fetching one PR (`r`) or every visible PR (`R`). A refresh updates the PR list entry plus all enrichment queries (threads, checks, reviews, files).

**Priority Queue**:
The queue background refreshes flow through. Items are dequeued highest-priority-first; smart-status tier determines priority so `me-blocking` PRs refresh ahead of `waiting-on-author` ones.

**Fetch Priority**:
Which lane a network request takes through the shared concurrency limiter. Orthogonal to the Priority Queue's smart-status ordering. Two values:

- `Interactive` — a fetch the user is actively waiting on: the detail body on drill-in or `r`, and the selected PR's files. Takes precedence so it doesn't queue behind a list-wide enrichment backlog.
- `Background` — speculative, list-wide work: the open-PR listing, the enrichment fan-out, and `R` (refresh-all, same volume as the initial load).

### File categorisation

**File Category**:
One of `code`, `test`, `generated`, `docs`, `config`. Assigned per file by pattern rules from `fileRules` in config. Drives the summary panel's per-category size breakdown.

## Relationships

- A **PR** belongs to exactly one **Tracked Repo** and has many **Review Threads** and **Issue Comments**.
- Every **PR** has exactly one **Smart-status** computed from its current state.
- A **Worktree** belongs to one **PR** and one **Source Clone**.
- The **Blocker** of a `waiting-on-author` PR is the **Effective Author**; the **Blocker** of a `me-blocking` PR is the current user.
- A **Refresh** enqueues one or more **PR**s into the **Priority Queue** for re-fetching.

## Example dialogue

> **Dev:** "If the **PR** is approved but CI is failing, who's the **Blocker**?"
> **Domain expert:** "The **Effective Author** — CI rule fires before the approval rule. They have to fix CI before anyone needs to re-review."

> **Dev:** "What if there are five unresolved **Review Threads**, all `awaiting-reviewer`, and one of them is awaiting me?"
> **Domain expert:** "Pick the reviewer with the most awaiting threads as the **Blocker**. Ties go to the longest-waiting one. If that's me, the PR's **Smart-status** is `me-blocking`."
