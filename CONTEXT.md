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

**Next Action**:
A short user-facing reason for why a PR is in its **Smart-status**, chosen by the blocker engine's first matching rule.
_Avoid_: blocker reason, status text.

**Review Requested**:
A **Next Action** meaning a specific reviewer has been asked to review and has not yet responded; word as "Review requested from you" for the current user and "Review requested from <login>" for someone else.
_Avoid_: You are a requested reviewer.

**Waiting on Reviewer Threads**:
A **Next Action** meaning every unresolved review thread has an author reply and the reviewer must resolve or reply; word as "<N> threads waiting on you" for the current user and "<N> threads waiting on <login>" for someone else.
_Avoid_: awaiting reviewer.

**Author Reply Needed**:
A **Next Action** meaning unresolved review threads are waiting for the **Effective Author** to respond; word as "<N> threads need your reply" for the current user and "<N> threads need author reply" otherwise.
_Avoid_: unreplied threads.

**Ready to Merge**:
A **Next Action** meaning reviews approve the PR and the **Effective Author** should merge; word as "Ready for you to merge" when the current user is the **Blocker**, otherwise "Ready to merge".

**Draft Not Ready**:
A **Next Action** meaning the PR is a draft and should not be reviewed yet; word as "Draft - not ready for review".

**Merge Conflict**:
A **Next Action** meaning the PR cannot merge until conflicts are resolved; word as "Resolve merge conflict".

**Requested Changes Response**:
A **Next Action** meaning a reviewer requested changes and the **Effective Author** must respond before further review is needed; word as "Respond to requested changes".

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

### Detail view

**Focus Sequence**:
The detail view's flat ordering of focusable items: the PR body first, then each visible **Review Thread**'s root comment followed by its replies, then each visible **Issue Comment**. `j`/`k` move the focus through it. Derived from enrichment plus the [[Detail Filters]]. The focus is identity-keyed by comment URL (the body is the special URL-less first item) and re-anchored against the fresh sequence after every update — arriving data or a filter toggle moves the focused card's _index_, never _which card_ is focused. Only when the focused item vanishes (filtered out, gone on refresh) does the focus fall back to its last position.

**Card**:
One Focus Sequence item's framed block in the detail body: byline plus markdown body (thread roots also carry their `path:line` and **Thread Classification** badge). The focused card draws a rounded border; unfocused cards reserve the same rows so focus changes never shift the layout. A card body longer than a few rows collapses with a "+N more lines" marker; Enter toggles the focused card's expansion, keyed by the comment's URL so it survives filter toggles.

**Detail Filters**:
The detail view's two visibility toggles: show resolved threads (`t`, default off) and show bot comments (`b`, default on). App-level preferences — they survive closing and reopening detail views. Hiding bot comments also hides a thread left with no visible comments.

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
Re-fetching one PR (`r` — the selected or open PR) or every visible PR (`R`). A refresh updates the PR list entry plus all enrichment queries (threads, checks, reviews, files). Each PR's refresh is one `Cmd::RefreshPr` dispatched straight through the [[Priority Queue]]: the focused PR is promoted, and `R` dispatches in **Smart-status** tier order so `me-blocking` PRs refresh first (ADR 0004). An in-flight refresh shows a per-row indicator; re-pressing while it is in flight is a no-op.

**Priority Queue**:
The shared network limiter's queue of pending fetches. Requests are granted highest-priority-first: interactive-effective ones (see [[Fetch Priority]]) ahead of background ones, FIFO within a lane. Smart-status tier does not influence the limiter's internal order; instead a tier-ordered **Refresh** (`R`) dispatches its requests in tier order so the background FIFO lane drains `me-blocking` first (ADR 0004), rather than the limiter sorting by tier (ADR 0003).

**Fetch Priority**:
Which lane a network request takes through the shared concurrency limiter — fully derived from focus, never declared at dispatch. A request carries the PR it serves (or none, for repo-wide work like the open-PR listing) and is:

- `Interactive` while that PR is the **Focused PR** — so the fetches the user is actively waiting on (the detail body on drill-in or `r`, the selected PR's files and enrichment) take precedence over the list-wide backlog.
- `Background` otherwise: speculative, list-wide work (the open-PR listing, the enrichment fan-out, `R` refresh-all) and any fetch for a PR the user has moved away from.

Because priority is derived, it shifts while a request is still queued (see [[Focus Promotion]]).

**Focused PR**:
The single PR whose pending work the limiter prioritises: the open **PR Detail**, or — in the list view — the selected PR. Changing the selection or drilling in/out moves the focus.

**Focus Promotion**:
Re-ranking the **Priority Queue** when the **Focused PR** changes, so the focused PR's still-pending fetches (its threads, reviews, checks, files, detail body) jump ahead of the rest of the fan-out — and the previously-focused PR's pending fetches demote back to `Background`. Only pending requests re-rank; one already in flight keeps running.

### File categorisation

**File Category**:
One of `code`, `test`, `generated`, `docs`, `config`. Assigned per file by pattern rules from `fileRules` in config. Drives the summary panel's per-category size breakdown.

## Relationships

- A **PR** belongs to exactly one **Tracked Repo** and has many **Review Threads** and **Issue Comments**.
- Every **PR** has exactly one **Smart-status** computed from its current state.
- Every **PR** with a computed **Smart-status** has exactly one **Next Action**.
- A **Next Action** explains why the **Blocker** must act; it can still be meaningful when **Blocker** is empty.
- **Review Requested** is the requested-reviewer form of **Next Action**; when the requested reviewer is the current user, word it as a request "from you".
- **Waiting on Reviewer Threads** uses the reviewer selected by the blocker engine as the **Blocker**.
- **Author Reply Needed** uses the **Effective Author** as the **Blocker**.
- **Ready to Merge** belongs to the `waiting-on-author` **Smart-status** unless the current user is the **Blocker**, in which case it is elevated to `me-blocking`.
- **Draft Not Ready** and **Merge Conflict** use the **Effective Author** as the **Blocker**.
- **Requested Changes Response** uses the **Effective Author** as the **Blocker** and takes precedence over pending review requests.
- **Smart-status** and **Next Action** are authoritative only after the enrichment they depend on has arrived; raw PR facts such as draft, mergeability, and review decision may still be shown before then.
- CI check summaries count all checks, but individual check rows are reserved for checks that are failed, pending, or action-required.
- A check with GitHub's `action_required` conclusion is a **Next Action** after hard CI failures but before draft, conflict, and review rules.
- The selected PR summary is action-first: identity, **Next Action**, mergeability, threads, reviews/requested reviewers, checks, files, contextual metadata, worktree, then URL.
- Assignees are contextual metadata unless they make the current user the **Effective Author**; labels are contextual metadata until legit gives specific labels domain meaning.
- The PR list keeps review state, unresolved thread counts, and **Next Action** as separate scanning signals when width allows.
- A **Worktree** belongs to one **PR** and one **Source Clone**.
- The **Blocker** of a `waiting-on-author` PR is the **Effective Author**; the **Blocker** of a `me-blocking` PR is the current user.
- A **Refresh** sends each of its fetches through the **Priority Queue**, each with a **Fetch Priority**.

## Example dialogue

> **Dev:** "If the **PR** is approved but CI is failing, who's the **Blocker**?"
> **Domain expert:** "The **Effective Author** — CI rule fires before the approval rule. They have to fix CI before anyone needs to re-review."

> **Dev:** "When I am requested as a reviewer, should the row say my login or what I need to do?"
> **Domain expert:** "The **Blocker** is you, but the **Next Action** is the useful label: 'Review requested from you'."

> **Dev:** "What if there are five unresolved **Review Threads**, all `awaiting-reviewer`, and one of them is awaiting me?"
> **Domain expert:** "Pick the reviewer with the most awaiting threads as the **Blocker**. Ties go to the longest-waiting one. If that's me, the PR's **Smart-status** is `me-blocking`."
