/**
 * I/O glue between the high-level primitives and the TanStack query cache.
 *
 * `runPrRefresh` is the body of the refresh-queue's `runRefresh` callback —
 * it fetches the PR + threads + reviews (and optionally checks + files),
 * commits everything to the cache, and prunes the pr-index when the PR
 * has been merged or closed.
 *
 * `fetchDetail` / `commitDetailFetch` are the callbacks for the detail-view
 * primitive — they handle the parallel detail fetch and persist its result
 * into the per-PR and threads caches.
 *
 * Keeping these side-effecting helpers in their own module lets `AppInner`
 * read as composition: instantiate primitives, wire callbacks, render.
 */

import type { QueryClient } from "@tanstack/solid-query";
import type { Legit } from "./legit";
import type { PRIdentity } from "./pr-identity";
import type { PRDetail } from "./types";
import type { DetailFetchResult } from "./detail-state";
import type { QueueItem } from "./refresh-queue";
import type { PRQueriesActions } from "./pr-queries";

export interface PrCacheDeps {
  app: Legit;
  queryClient: QueryClient;
  prActions: PRQueriesActions;
}

/** Body of the refresh-queue's runRefresh callback. */
export async function runPrRefresh(deps: PrCacheDeps, item: QueueItem): Promise<void> {
  const { app, queryClient, prActions } = deps;
  const { pr, includeFiles } = item;
  const repo = pr.repoSlug ?? app.repoSlug;
  prActions.notePrRefreshed(repo, pr.number);

  const [nextPr, threads, reviews] = await Promise.all([
    app.fetchPR(repo, pr.number),
    app.fetchFullReviewThreads(repo, pr.number),
    app.fetchReviews(repo, pr.number),
  ]);

  queryClient.setQueryData<PRDetail>(["pr", repo, pr.number], (prev) => ({
    ...(prev ?? {}),
    ...nextPr,
    repoSlug: repo,
  }));
  queryClient.setQueryData(["threads", repo, pr.number], threads);
  queryClient.setQueryData(["reviews", repo, pr.number], reviews);
  prActions.prunePrIndexIfClosed(repo, nextPr);

  if (nextPr.headCommitSha) {
    const checks = await app.fetchCheckRuns(repo, nextPr.headCommitSha);
    queryClient.setQueryData(["checks", repo, nextPr.headCommitSha], checks);
  }

  if (includeFiles) {
    const files = await app.fetchCategorizedFiles(repo, pr.number);
    queryClient.setQueryData(["files", repo, pr.number], files);
  }

  const sourceClone = app.resolveSourceClone(repo);
  if (sourceClone) {
    void queryClient.invalidateQueries({ queryKey: ["worktrees", sourceClone] });
  }
}

/** Detail-view fetch: PR + threads + comments in parallel, with the
 *  freshly-fetched PR carrying its repoSlug so cache reads stay correct. */
export async function fetchDetail(
  app: Legit,
  pr: PRIdentity,
  signal: AbortSignal,
): Promise<DetailFetchResult> {
  const repo = pr.repoSlug ?? app.repoSlug;
  const [nextPr, threads, comments] = await Promise.all([
    app.fetchPR(repo, pr.number, signal),
    app.fetchFullReviewThreads(repo, pr.number, signal),
    app.fetchIssueComments(repo, pr.number, signal),
  ]);
  return { pr: { ...nextPr, repoSlug: repo }, threads, comments };
}

/** Detail-view onFetched callback: commit the freshly-fetched PR + threads
 *  to the cache and prune the pr-index when the PR is closed. */
export function commitDetailFetch(
  deps: PrCacheDeps,
  pr: PRIdentity,
  result: DetailFetchResult,
): void {
  const { queryClient, prActions, app } = deps;
  const repo = pr.repoSlug ?? app.repoSlug;
  queryClient.setQueryData<PRDetail>(["pr", repo, pr.number], (prev) => ({
    ...(prev ?? {}),
    ...result.pr,
    repoSlug: repo,
  }));
  queryClient.setQueryData(["threads", repo, pr.number], result.threads);
  prActions.prunePrIndexIfClosed(repo, result.pr);
}
