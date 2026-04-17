import {
  computeBlocker,
  tierLabel,
  type BlockerOptions,
  type BlockerResult,
  type Tier,
} from "./blocker-engine";
import { formatReviewDecision, formatReviewState } from "./format";
import { aggregateReviewState, currentUserReviewState } from "./review-state";
import { computeCommentCounts, type CommentCounts, type PR, type ReviewState } from "./types";

export type BlockerDisplayTone = "self" | "warning" | "muted";

export interface BlockerDisplayState {
  text: string;
  tone: BlockerDisplayTone;
}

export interface SmartStatusState {
  key: Tier;
  label: string;
}

export interface PRDerivedState {
  loading: boolean;
  reviewText: string;
  currentUserReview: ReviewState | undefined;
  commentCounts: CommentCounts | undefined;
  blockerResult: BlockerResult | undefined;
  blockerDisplay: BlockerDisplayState | null;
  smartStatus: SmartStatusState | undefined;
}

export interface PRDerivedOptions extends BlockerOptions {
  currentUser?: string;
  loading?: boolean;
}

function blockerDisplay(
  blocker: BlockerResult | undefined,
  currentUser: string | undefined,
): BlockerDisplayState | null {
  if (!blocker || !currentUser) return null;

  const isMe = blocker.blocker === currentUser;
  switch (blocker.tier) {
    case "me-blocking":
      return { text: "you", tone: "self" };
    case "waiting-on-author":
      return {
        text: isMe ? "you" : blocker.blocker || "author",
        tone: isMe ? "self" : "warning",
      };
    case "needs-review":
      return blocker.blocker ? { text: blocker.blocker, tone: "muted" } : null;
  }
}

export function derivePRState(pr: PR, options: PRDerivedOptions = {}): PRDerivedState {
  const { currentUser, loading = false } = options;
  const reviews = options.reviews;
  const threads = options.threads;
  const checks = options.checks ?? [];

  const currentUserReview = currentUserReviewState(pr, currentUser, reviews);
  const aggregateReview = aggregateReviewState(pr, reviews);
  const reviewText = aggregateReview
    ? formatReviewState(aggregateReview)
    : currentUserReview
      ? formatReviewState(currentUserReview)
      : formatReviewDecision(pr.reviewDecision);

  const commentCounts = threads ? computeCommentCounts(threads) : undefined;

  if (loading) {
    return {
      loading,
      reviewText,
      currentUserReview,
      commentCounts,
      blockerResult: undefined,
      blockerDisplay: null,
      smartStatus: undefined,
    };
  }

  const blockerResult = computeBlocker(pr, currentUser ?? "", {
    checks,
    reviews: reviews ?? [],
    threads: threads ?? [],
  });

  return {
    loading: false,
    reviewText,
    currentUserReview,
    commentCounts,
    blockerResult,
    blockerDisplay: blockerDisplay(blockerResult, currentUser),
    smartStatus: {
      key: blockerResult.tier,
      label: tierLabel(blockerResult.tier),
    },
  };
}
