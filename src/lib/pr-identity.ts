import type { PR } from "./types";

export type PRIdentity = Pick<PR, "number" | "repoSlug">;

export function prKey(pr: PRIdentity): PRIdentity {
  return { number: pr.number, repoSlug: pr.repoSlug };
}

export function samePr(a: PRIdentity | undefined, b: PRIdentity | undefined): boolean {
  if (a === b) return true;
  if (!a || !b) return a === b;
  return a.number === b.number && a.repoSlug === b.repoSlug;
}

export function samePrKey(pr: PRIdentity, key: PRIdentity | null | undefined): boolean {
  return key !== null && key !== undefined && samePr(pr, key);
}

export function findPrIndex(prs: PR[], target: PRIdentity): number {
  return prs.findIndex((pr) => samePr(pr, target));
}
