import type { PR } from "../src/lib/types";

/**
 * Create a PR with sensible defaults. Override any field.
 */
export function makePR(overrides: Partial<PR> = {}): PR {
	return {
		number: 42,
		title: "Fix the thing",
		author: "alice",
		createdAt: "2026-03-01T00:00:00Z",
		updatedAt: "2026-03-15T00:00:00Z",
		additions: 50,
		deletions: 10,
		isDraft: false,
		labels: [],
		requestedReviewers: [],
		assignees: [],
		reviewDecision: "",
		mergeable: "MERGEABLE",
		lastCommitDate: "2026-03-14T00:00:00Z",
		...overrides,
	};
}
