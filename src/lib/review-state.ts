import type { PR, Review, ReviewState } from "./types";

/**
 * Return the latest review state for the current user when they are acting as a
 * reviewer rather than the PR author.
 */
export function currentUserReviewState(
  pr: Pick<PR, "author">,
  currentUser: string | undefined,
  reviews: Review[] | undefined,
): ReviewState | undefined {
  if (!currentUser || !reviews || pr.author === currentUser) return undefined;
  return reviews.find((review) => review.user === currentUser)?.state;
}

/**
 * Best-effort aggregate review state when GitHub's top-level reviewDecision is
 * empty or lagging behind the individual reviews we have already loaded.
 */
export function aggregateReviewState(
  pr: Pick<PR, "reviewDecision">,
  reviews: Review[] | undefined,
): ReviewState | undefined {
  if (pr.reviewDecision === "CHANGES_REQUESTED" || pr.reviewDecision === "APPROVED") {
    return pr.reviewDecision;
  }
  if (!reviews || reviews.length === 0) return undefined;
  if (reviews.some((review) => review.state === "CHANGES_REQUESTED")) {
    return "CHANGES_REQUESTED";
  }
  if (reviews.some((review) => review.state === "APPROVED")) {
    return "APPROVED";
  }
  return undefined;
}
