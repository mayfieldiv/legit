/**
 * GitHub Client — pure parsing + orchestration over a GitHubTransport.
 * Parsing functions are exported for direct testing.
 */

import type {
  RawRestPR,
  RawPRReviewStatus,
  RawFileChange,
  GitHubTransport,
} from "./github-transport";
import type {
  PR,
  PRDetail,
  PRState,
  FileChange,
  CheckRun,
  Review,
  FullReviewThread,
  ReviewComment,
  IssueComment,
} from "./types";

// Re-export transport types that callers may need
export type {
  HttpFetch,
  GitHubTransport,
  RawRestPR,
  RawPRReviewStatus,
  RawFileChange,
  RawCheckRun,
} from "./github-transport";

// Re-export domain types for backward compatibility
export type {
  PR,
  PRDetail,
  FileChange,
  CheckRun,
  Review,
  FullReviewThread,
  ReviewComment,
  IssueComment,
} from "./types";

// ── Intermediate parsed types ───────────────────────────────────────────────

export interface RestPR {
  number: number;
  title: string;
  author: string;
  createdAt: string;
  updatedAt: string;
  additions: number;
  deletions: number;
  isDraft: boolean;
  labels: string[];
  requestedReviewers: string[];
  assignees: string[];
  headRef: string;
  baseRef: string;
  headRepositoryOwner: string;
  state: PRState;
}

export interface ReviewStatus {
  additions: number;
  deletions: number;
  reviewDecision: string;
  mergeable: string;
  lastCommitDate: string | null;
  headCommitSha: string | null;
}

// ── Pure parsing functions ──────────────────────────────────────────────────

export function parseRestPR(raw: RawRestPR): RestPR {
  // GitHub reports merged PRs as state: "closed" with merged_at set —
  // split them into a distinct MERGED state so the UI can distinguish them
  // from PRs that were closed without being merged. The list endpoint omits
  // `state` entirely, so default to OPEN when absent.
  const state: PRState = raw.state === "closed" ? (raw.merged_at ? "MERGED" : "CLOSED") : "OPEN";
  return {
    number: raw.number,
    title: raw.title,
    author: raw.user?.login ?? "ghost",
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
    additions: raw.additions ?? 0,
    deletions: raw.deletions ?? 0,
    isDraft: raw.draft ?? false,
    labels: (raw.labels ?? []).map((l) => l.name),
    requestedReviewers: (raw.requested_reviewers ?? []).map((r) => r.login),
    assignees: (raw.assignees ?? []).map((a) => a.login),
    headRef: raw.head?.ref ?? "",
    baseRef: raw.base?.ref ?? "",
    headRepositoryOwner: raw.head?.repo?.owner?.login ?? "",
    state,
  };
}

export function parseReviewStatus(raw: RawPRReviewStatus): ReviewStatus {
  const commitNode = raw.commits.nodes[0]?.commit;
  return {
    additions: raw.additions ?? 0,
    deletions: raw.deletions ?? 0,
    reviewDecision: raw.reviewDecision ?? "",
    mergeable: raw.mergeable ?? "UNKNOWN",
    lastCommitDate: commitNode?.committedDate ?? null,
    headCommitSha: commitNode?.oid ?? null,
  };
}

export function parseFileChange(raw: RawFileChange): FileChange {
  return {
    path: raw.filename,
    additions: raw.additions ?? 0,
    deletions: raw.deletions ?? 0,
  };
}

export function mergePR(rest: RestPR, status?: ReviewStatus): PR {
  return {
    ...rest,
    additions: status?.additions ?? rest.additions,
    deletions: status?.deletions ?? rest.deletions,
    reviewDecision: status?.reviewDecision ?? "",
    mergeable: status?.mergeable ?? "UNKNOWN",
    lastCommitDate: status?.lastCommitDate ?? null,
    headCommitSha: status?.headCommitSha ?? null,
  };
}

// ── Client interface ────────────────────────────────────────────────────────

export interface GitHubClient {
  fetchOpenPRs(repo: string, signal?: AbortSignal): AsyncIterable<PR[]>;
  fetchPR(repo: string, prNumber: number, signal?: AbortSignal): Promise<PRDetail>;
  fetchFiles(repo: string, prNumber: number, signal?: AbortSignal): AsyncIterable<FileChange[]>;
  fetchCheckRuns(repo: string, commitSha: string, signal?: AbortSignal): Promise<CheckRun[]>;
  fetchReviews(repo: string, prNumber: number, signal?: AbortSignal): Promise<Review[]>;
  fetchFullReviewThreads(
    repo: string,
    prNumber: number,
    botLogins: string[],
    signal?: AbortSignal,
  ): Promise<FullReviewThread[]>;
  fetchIssueComments(
    repo: string,
    prNumber: number,
    botLogins: string[],
    signal?: AbortSignal,
  ): Promise<IssueComment[]>;
}

export interface GitHubClientOptions {
  /**
   * Delay in ms before retrying PRs with UNKNOWN mergeable status.
   * GitHub computes mergeability lazily; this delay lets the computation finish.
   * Default: 3000.
   */
  mergeableRetryDelayMs?: number;
  /**
   * Max number of newly discovered PRs to accumulate before yielding a fresh
   * open-PR snapshot after the initial eager snapshot.
   * Default: 20.
   */
  openPrRestSnapshotBatchSize?: number;
  /**
   * Max number of enrichment updates to accumulate before yielding a fresh
   * open-PR snapshot after the initial eager snapshot.
   * Default: 25.
   */
  openPrStatusSnapshotBatchSize?: number;
}

function parseOwnerRepo(repo: string): [string, string] {
  const parts = repo.split("/");
  if (parts.length !== 2 || !parts[0] || !parts[1]) throw new Error(`Invalid repo format: ${repo}`);
  return [parts[0], parts[1]];
}

export function createGitHubClient(
  transport: GitHubTransport,
  options?: GitHubClientOptions,
): GitHubClient {
  const MERGEABLE_RETRY_DELAY_MS = options?.mergeableRetryDelayMs ?? 3_000;
  const OPEN_PR_REST_SNAPSHOT_BATCH_SIZE = options?.openPrRestSnapshotBatchSize ?? 20;
  const OPEN_PR_STATUS_SNAPSHOT_BATCH_SIZE = options?.openPrStatusSnapshotBatchSize ?? 25;

  const shouldFlushSnapshotBatch = (
    totalItems: number,
    pendingItems: number,
    batchSize: number,
  ): boolean => {
    return totalItems === 1 || pendingItems >= batchSize;
  };

  return {
    async *fetchOpenPRs(repo: string, signal?: AbortSignal) {
      const [owner, repoName] = parseOwnerRepo(repo);

      // Phase 1: yield PRs as they stream in from REST (no review status yet).
      // Yield the first item eagerly so the UI can render immediately, then
      // batch later additions to avoid hundreds of whole-list re-renders.
      const restPRs: RestPR[] = [];
      const prs: PR[] = [];
      let pendingRestUpdates = 0;

      for await (const raw of transport.listOpenPRs(owner, repoName, signal)) {
        const rest = parseRestPR(raw);
        restPRs.push(rest);
        prs.push(mergePR(rest));
        pendingRestUpdates++;

        if (
          shouldFlushSnapshotBatch(prs.length, pendingRestUpdates, OPEN_PR_REST_SNAPSHOT_BATCH_SIZE)
        ) {
          yield [...prs];
          pendingRestUpdates = 0;
        }
      }

      if (pendingRestUpdates > 0) {
        yield [...prs];
      }

      if (prs.length === 0) return;

      // Phase 2: enrich with review status as it streams in. Apply the same
      // eager-first-then-batched strategy so review/check metadata doesn't
      // thrash the list view with one full snapshot per PR.
      let pendingStatusUpdates = 0;
      let totalStatusUpdates = 0;
      for await (const rawStatus of transport.fetchReviewStatus(
        owner,
        repoName,
        restPRs.map((r) => r.number),
        signal,
      )) {
        const status = parseReviewStatus(rawStatus);
        const idx = restPRs.findIndex((r) => r.number === rawStatus.prNumber);
        if (idx === -1) continue;

        prs[idx] = mergePR(restPRs[idx]!, status);
        pendingStatusUpdates++;
        totalStatusUpdates++;

        if (
          shouldFlushSnapshotBatch(
            totalStatusUpdates,
            pendingStatusUpdates,
            OPEN_PR_STATUS_SNAPSHOT_BATCH_SIZE,
          )
        ) {
          yield [...prs];
          pendingStatusUpdates = 0;
        }
      }

      if (pendingStatusUpdates > 0) {
        yield [...prs];
      }
    },

    async fetchPR(repo: string, prNumber: number, signal?: AbortSignal): Promise<PRDetail> {
      const [owner, repoName] = parseOwnerRepo(repo);

      const raw = await transport.getPR(owner, repoName, prNumber, signal);
      const rest = parseRestPR(raw);

      let status: ReviewStatus | undefined;
      for await (const rawStatus of transport.fetchReviewStatus(
        owner,
        repoName,
        [prNumber],
        signal,
      )) {
        status = parseReviewStatus(rawStatus);
      }

      // Retry once if mergeable is UNKNOWN (GitHub lazy computation). Skip
      // the retry for merged/closed PRs — they always report UNKNOWN and
      // the retry just burns 3s with no hope of a better answer.
      if (rest.state === "OPEN" && (!status || status.mergeable === "UNKNOWN")) {
        await new Promise((r) => setTimeout(r, MERGEABLE_RETRY_DELAY_MS));
        signal?.throwIfAborted();
        for await (const rawStatus of transport.fetchReviewStatus(
          owner,
          repoName,
          [prNumber],
          signal,
        )) {
          status = parseReviewStatus(rawStatus);
        }
      }

      return {
        ...mergePR(rest, status),
        body: raw.body ?? "",
      };
    },

    async *fetchFiles(repo: string, prNumber: number, signal?: AbortSignal) {
      const [owner, repoName] = parseOwnerRepo(repo);
      const files: FileChange[] = [];
      for await (const raw of transport.listPRFiles(owner, repoName, prNumber, signal)) {
        files.push(parseFileChange(raw));
        yield [...files];
      }
    },

    async fetchReviews(repo: string, prNumber: number, signal?: AbortSignal): Promise<Review[]> {
      const [owner, repoName] = parseOwnerRepo(repo);
      const rawReviews: Array<{ user: string; state: string; submitted_at: string }> = [];
      for await (const raw of transport.listReviews(owner, repoName, prNumber, signal)) {
        if (raw.state === "PENDING") continue;
        const login = raw.user?.login;
        if (!login) continue;
        rawReviews.push({
          user: login,
          state: raw.state,
          submitted_at: raw.submitted_at,
        });
      }
      const byUser = new Map<string, { state: string; submitted_at: string }>();
      for (const r of rawReviews) {
        const existing = byUser.get(r.user);
        if (!existing || r.submitted_at > existing.submitted_at) {
          byUser.set(r.user, r);
        }
      }
      return Array.from(byUser.entries()).map(([user, r]) => ({
        user,
        state: r.state as Review["state"],
      }));
    },

    async fetchFullReviewThreads(
      repo: string,
      prNumber: number,
      botLogins: string[],
      signal?: AbortSignal,
    ): Promise<FullReviewThread[]> {
      const [owner, repoName] = parseOwnerRepo(repo);
      const botSet = new Set(botLogins);
      const threads: FullReviewThread[] = [];

      for await (const raw of transport.fetchFullReviewThreads(owner, repoName, prNumber, signal)) {
        const comments: ReviewComment[] = raw.comments.nodes.map((c) => {
          const login = c.author?.login ?? "ghost";
          const isBot =
            c.author != null &&
            (c.author.__typename === "Bot" || login.endsWith("[bot]") || botSet.has(login));
          return {
            id: c.id,
            author: login,
            body: c.body,
            createdAt: c.createdAt,
            url: c.url,
            isBot,
          };
        });
        threads.push({
          id: raw.id,
          isResolved: raw.isResolved,
          path: raw.path,
          line: raw.line,
          comments,
        });
      }

      return threads;
    },

    async fetchIssueComments(
      repo: string,
      prNumber: number,
      botLogins: string[],
      signal?: AbortSignal,
    ): Promise<IssueComment[]> {
      const [owner, repoName] = parseOwnerRepo(repo);
      const botSet = new Set(botLogins);
      const comments: IssueComment[] = [];

      for await (const raw of transport.listIssueComments(owner, repoName, prNumber, signal)) {
        const login = raw.user?.login ?? "ghost";
        const isBot =
          raw.user != null &&
          (raw.user.type === "Bot" || login.endsWith("[bot]") || botSet.has(login));
        comments.push({
          id: raw.id,
          author: login,
          body: raw.body,
          createdAt: raw.created_at,
          url: raw.html_url,
          isBot,
        });
      }

      return comments;
    },

    async fetchCheckRuns(
      repo: string,
      commitSha: string,
      signal?: AbortSignal,
    ): Promise<CheckRun[]> {
      const [owner, repoName] = parseOwnerRepo(repo);
      const checks: CheckRun[] = [];
      for await (const raw of transport.listCheckRuns(owner, repoName, commitSha, signal)) {
        checks.push({
          name: raw.name,
          status: raw.status as CheckRun["status"],
          conclusion: raw.conclusion as CheckRun["conclusion"],
        });
      }
      return checks;
    },
  };
}
