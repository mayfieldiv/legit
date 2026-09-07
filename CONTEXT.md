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

**Main Worktree**:
A repo's main working copy — git's main worktree, the one its linked worktrees hang off. Configured per-repo; legit creates **Worktree**s from it and starts local wayfinder Effort discovery there, fanning out across its linked worktrees. Without one, worktree features and local Effort discovery are unavailable for that repo.
_Avoid_: source clone (the term this replaces), path.

**Tracked Repo**:
A repo present in `~/.legit/config.json` plus the repo detected from the CWD — the set of repos legit tracks. One with a GitHub slug is PR-capable; a slug-less local repo (a **Main Worktree** alone) participates only in the ticket surface and gets no Repo Tab.

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
One Focus Sequence item's framed block in the detail body: byline plus markdown body (thread roots also carry their `path:line` and **Thread Classification** badge). The focused card draws a rounded border; unfocused cards reserve the same rows so focus changes never shift the layout. Enter toggles the focused card's **Details Group**s (keyed by the comment's URL so it survives filter toggles). A card body longer than ~100 rendered rows is additionally capped with a "+N more lines (truncated)" marker — an unconditional backstop for pathological bodies (bot dumps, pasted logs), independent of the Enter toggle and never lifted by it.

**Details Group**:
A collapsible `<details>`/`<summary>` region inside a markdown body, rendered as a `▶ summary` line collapsed or `▼ summary` plus its inner markdown (indented) expanded. Each **Card** folds and unfolds all of its groups together: Enter is a per-card toggle-all, keyed by the comment's URL (the PR body, which has no URL, uses a URL-less sentinel). A card with no group has nothing for Enter to toggle, so the keypress is a no-op there.

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
A git worktree on disk under the user's `worktreeRoot` (defaulting to `~/.legit/worktrees/<owner>/<repo>/<number>-<branch>`), checked out to a PR's head branch by `gh pr checkout`. legit can create one for any PR whose repo has a configured **Main Worktree**, and detects whether one already exists for any PR shown in the list/detail views.

**Expected Branch**:
The local branch name `gh pr checkout` would produce for a PR. Same-repo PRs keep `headRef` verbatim; fork PRs get prefixed with `<forkOwner>-` to avoid collisions across forks of the same branch name.

### Refresh

**Fetch Unit**:
The batch of data one read fetches, stamps, and refreshes together — a **PR**'s enrichment, or an **Effort**'s ticket data (one GitHub map read or one local probe). **Fetch Age**, the refresh indicator, and `r`/`R` are keyed by Fetch Unit, never by a larger container: `r` refreshes the unit backing the selected row, `R` every unit backing the current view scope. A future ticket source brings its own Fetch Unit and inherits these semantics unchanged.
_Avoid_: fetch scope (collides with view scope).

**Refresh**:
Re-fetching one PR (`r` — the selected or open PR) or every visible PR (`R`). A refresh updates the PR list entry plus all enrichment queries (threads, checks, reviews, files). Each PR's refresh is one `Cmd::RefreshPr` dispatched straight through the [[Priority Queue]]: the focused PR is promoted, and `R` dispatches in **Smart-status** tier order so `me-blocking` PRs refresh first (ADR 0004). An in-flight refresh shows a per-row indicator; re-pressing while it is in flight is a no-op. In list view, refresh also reconciles local **Worktrees**: `r` lists only the selected PR's **Main Worktree**, while `R` lists every configured **Main Worktree**. Both keys also trigger a [[Re-list]] for discovery: `R` always re-lists the in-scope repos, and `r` re-lists when the active Repo Tab has no PRs (nothing to refresh, so "check GitHub for new PRs" instead). On the ticket surface, `r` re-reads the **Fetch Unit** backing the selected **Ticket** (the detail view's `r` also re-fetches the open Ticket's body and comments) and `R` re-reads every unit in the current view scope; Tickets have no separate **Re-list** — one map read reconciles membership and enrichment together.

**Re-list**:
Re-fetching a repo's [[Open PR List]] to reconcile _membership_ — surfacing PRs opened since the last listing and pruning ones closed or merged since — as distinct from a **Refresh**, which re-fetches the enrichment of PRs already pooled. A re-list re-streams the listing on top of the pooled PRs rather than clearing them first: each arrival is deduped by key, so a surviving PR keeps the enrichment fetched for it and a newly-opened PR is appended, and once the listing settles any pooled PR whose number didn't reappear is dropped. `R` re-lists every in-scope repo (the active Repo Tab's repo, or all Tracked Repos on the All tab); `r` re-lists only an empty Repo Tab.

**Priority Queue**:
The shared network limiter's queue of pending fetches. Requests are granted highest-priority-first: interactive-effective ones (see [[Fetch Priority]]) ahead of background ones, FIFO within a lane. Smart-status tier does not influence the limiter's internal order; instead a tier-ordered **Refresh** (`R`) dispatches its requests in tier order so the background FIFO lane drains `me-blocking` first (ADR 0004), rather than the limiter sorting by tier (ADR 0003).

**Fetch Priority**:
Which lane a network request takes through the shared concurrency limiter — fully derived from focus, never declared at dispatch. A request carries the PR or **Ticket** it serves (or none, for unit-wide work like the open-PR listing or an **Effort**'s map read) and is:

- `Interactive` while that PR or Ticket is the **Focused PR** — so the fetches the user is actively waiting on (the detail body on drill-in or `r`, the selected PR's files and enrichment) take precedence over the list-wide backlog.
- `Background` otherwise: speculative, list-wide work (the open-PR listing, the enrichment fan-out, a map read, `R` refresh-all) and any fetch for a PR or Ticket the user has moved away from.

Because priority is derived, it shifts while a request is still queued (see [[Focus Promotion]]).

**Affinity**:
The PR or **Ticket** a network request serves, carried by the request so its [[Fetch Priority]] can be derived from the **Focused PR**. Unit-wide work (the open-PR listing, an **Effort**'s map read) carries none and is never promoted.

**Focused PR**:
The single PR whose pending work the limiter prioritises: the open **PR Detail**, or — in the list view — the selected PR. Changing the selection or drilling in/out moves the focus. The limiter tracks one focused entity across surfaces: on the ticket surface it is the open or selected **Ticket**, and toggling surfaces moves the focus there, demoting the other surface's pending fetches.

**Focus Promotion**:
Re-ranking the **Priority Queue** when the **Focused PR** changes, so the focused PR's still-pending fetches (its threads, reviews, checks, files, detail body) jump ahead of the rest of the fan-out — and the previously-focused PR's pending fetches demote back to `Background`. Only pending requests re-rank; one already in flight keeps running.

### Wayfinder tickets

**Effort**:
A unit of wayfinding work: one **Map** plus its **Tickets**. Belongs to exactly one **Tracked Repo**; whether its data comes from the repo's GitHub tracker or a local wayfinder directory is an attribute of the Effort, not a different kind of container.
_Avoid_: project, initiative.

**Map**:
The index artifact anchoring an Effort — a GitHub issue labelled `wayfinder:map` or a local `map.md`. Carries the **Destination** and the record of decisions; open Tickets are its children, never listed in its body.

**Destination**:
What reaching the end of an Effort looks like, stated on its Map. The Map closes when nothing is left to decide.

**Ticket**:
A single decision or investigation belonging to an Effort — a GitHub sub-issue of the Map, or a local ticket file. Open or Closed; Closed covers both resolved and closed-as-out-of-scope, which no dialect encodes structurally.
_Avoid_: issue (the GitHub artifact that may host a Ticket), task (a **Type**).

**Type**:
A Ticket's kind: `research`, `prototype`, `grilling`, or `task` — the `wayfinder:<type>` label or the dialect's type field. Unknown Types are shown verbatim, never hidden.

**Mode**:
Which kind of session can take a Ticket, derived from **Type**, never stored: `AFK` (agent alone — research), `HITL` (human in the loop — prototype, grilling), or `Either` (task, and unknown Types). The AFK/HITL filter tests Mode; `Either` matches both views, visibly tagged.

**Claim**:
The in-progress marker on an open Ticket — the GitHub assignee, or the dialect's claim field. A claimed Ticket is off the **Frontier**. Claims are taken and released by wayfinder sessions; legit only renders them.

**Dependency**:
A directed edge between Tickets: this Ticket waits on that one. A Ticket with any open Dependency is off the **Frontier**. "Blocked by" is ordinary prose for it. A person who must act before a Ticket is takeable is modelled as a Dependency on a `task` Ticket they've claimed — never as a new state.

**External Dependency**:
A Dependency whose target lives in another Effort.

**Unknown Dependency**:
A Dependency whose target legit cannot find or read. A Ticket with one is never on the **Frontier**, regardless of everything else legit can see.
_Avoid_: unresolved dependency (unresolved is a review-thread word).

**Blocks**:
The reverse read of **Dependency** — the open Tickets whose Dependencies include this one. Shown in the summary/detail views.
_Avoid_: blocking (as the field name; fine as prose).

**Frontier**:
The Tickets a session can take right now: open, unclaimed, every Dependency target closed, and no **Unknown Dependency**. The Ticket analog of **Smart-status** — Tickets never carry Smart-status, **Next Action**, or a PR-sense **Blocker**.
_Avoid_: takeable (in UI labels and counts too — say "Frontier" / "on the Frontier").

**Wayfinder Root**:
A directory probed for local Efforts — a root either is a single Effort itself or holds Effort subdirectories. Default roots are probed in a repo's **Main Worktree** and every worktree linked to it, and at each level from the cwd up to its git toplevel; a repo's configured roots replace the defaults for it.

### File categorisation

**File Category**:
One of `code`, `test`, `generated`, `docs`, `config`. Assigned per file by pattern rules from `fileRules` in config. Drives the summary panel's per-category size breakdown.

### Presentation

**Selected Row**:
The **Open PR List** row under the cursor, highlighted by a subtle full-width background fill with its title brightened — not inverted video. Every other cell keeps its semantic colour, so the row's colour-coding survives the highlight.
_Avoid_: highlighted row, active row, reverse-video row.

**Label Chip**:
A PR label rendered as a filled badge — the label's own GitHub colour as background, a contrast-flipped foreground — shown in the summary panel and detail header (not the list). Pure presentation: chips assign no domain meaning and drive no sort, filter, or **Smart-status**; they are a cosmetic rendering of the same contextual metadata the labels are otherwise.
_Avoid_: tag, badge (badge is the generic shape; chip is the legit term).

**Repo Color**:
A stable accent hue derived per repo by hashing its short name into a curated truecolor ramp. Applied wherever a repo's name appears as an identifier — the **Repo Tab** bar, the app-header scope, the All-tab repo cell, the repo group header, and the detail header — so a repo reads the same colour everywhere. The one exception is the PR's GitHub URL, where the slug stays on the link colour rather than recolouring a substring. Distinct from **Smart-status** tier colours, which never share a render site with a repo name.
_Avoid_: repo accent, repo tint.

**Check Duration**:
A check run's wall-clock time (`completed_at − started_at`), read from the same check-runs fetch that yields name and conclusion — best-effort, so only completed runs have one (queued/in-progress runs, and older commit statuses outside the check-runs endpoint, do not). Surfaced beside each completed check row and used as the secondary sort key after outcome.

**Fetch Age**:
How long ago legit last received a given **Fetch Unit**'s data — a PR's initial enrichment or **Refresh** settling, or an **Effort**'s map read or probe settling — shown as a relative age ("fetched 2m ago") per PR in the summary panel and detail header, and per Effort-backed unit on the effort card and ticket detail header. A per-unit staleness signal, deliberately not global: Fetch Units are fetched and refreshed independently (see [[Fetch Priority]], [[Refresh]]), so there is no single moment "the data" was loaded. Distinct from the PR's GitHub `updated_at` (its last activity, shown as "updated Y") and from the live network indicator's in-flight/waiting counts.
_Avoid_: last updated, updated at (reserved for GitHub's activity time).

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
- CI check summaries count all checks; individual check rows show up to eight checks of any outcome, ordered by outcome (failed/action-required, then pending, then passed) and within that by **Check Duration** descending so the slowest surfaces first. The detail view lays these rows out in two columns; the narrower summary panel uses one.
- A check with GitHub's `action_required` conclusion is a **Next Action** after hard CI failures but before draft, conflict, and review rules.
- The selected PR summary is action-first: identity, **Next Action**, mergeability, threads, reviews/requested reviewers, checks, files, contextual metadata, worktree, then URL.
- Assignees are contextual metadata unless they make the current user the **Effective Author**; labels are contextual metadata until legit gives specific labels domain meaning.
- The PR list keeps review state, unresolved thread counts, and **Next Action** as separate scanning signals when width allows.
- A **Worktree** belongs to one **PR** and one **Main Worktree**.
- The **Blocker** of a `waiting-on-author` PR is the **Effective Author**; the **Blocker** of a `me-blocking` PR is the current user.
- A **Refresh** sends each of its fetches through the **Priority Queue**, each with a **Fetch Priority**.
- An **Effort** belongs to exactly one **Tracked Repo**, has exactly one **Map**, and has many **Tickets**.
- A **Ticket**'s **Mode** is derived from its **Type**; unknown **Types** get Mode `Either`.
- A **Ticket** is on the **Frontier** iff it is open, unclaimed, and has no open or **Unknown Dependency**.
- **Efforts** and **Tickets** are read-only to legit: claiming and resolving happen in wayfinder sessions.
- Where an **Effort**'s data is found (discovery — a **Wayfinder Root** or a GitHub tracker) and which **Tracked Repo** it belongs to (attribution) are distinct steps; today attribution follows discovery, as a default rather than a definition.

## Example dialogue

> **Dev:** "If the **PR** is approved but CI is failing, who's the **Blocker**?"
> **Domain expert:** "The **Effective Author** — CI rule fires before the approval rule. They have to fix CI before anyone needs to re-review."

> **Dev:** "When I am requested as a reviewer, should the row say my login or what I need to do?"
> **Domain expert:** "The **Blocker** is you, but the **Next Action** is the useful label: 'Review requested from you'."

> **Dev:** "What if there are five unresolved **Review Threads**, all `awaiting-reviewer`, and one of them is awaiting me?"
> **Domain expert:** "Pick the reviewer with the most awaiting threads as the **Blocker**. Ties go to the longest-waiting one. If that's me, the PR's **Smart-status** is `me-blocking`."

> **Dev:** "A `task` **Ticket** shows in both the AFK and HITL views — is its **Mode** both?"
> **Domain expert:** "`Either` — takeable by either kind of session, not needing both."

> **Dev:** "Ticket #12 can't start until someone provisions the sandbox — where does that live?"
> **Domain expert:** "A `task` **Ticket** they claim; #12 takes a **Dependency** on it."

## Flagged ambiguities

- "updated" was overloaded — GitHub's PR activity time (`updated_at`, rendered "updated Y") versus how long ago legit last fetched the PR's data. Resolved: the local staleness signal is **Fetch Age** ("fetched Nm ago"); "updated" refers only to GitHub's activity time.
