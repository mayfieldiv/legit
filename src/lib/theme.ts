/**
 * Centralized color tokens for the entire UI.
 *
 * Every color in the app should reference a token from this object
 * rather than using a hard-coded color string. This makes it easy
 * to adjust the palette in one place and keeps the visual language
 * consistent across components.
 *
 * Values are hex strings (parsed by @opentui's `parseColor`).
 * Background: #282C34 (One Dark).
 */

export const theme = {
	/** Branding, headings, branch refs, section headers, filter labels, separators */
	accent: "#61AFEF",
	/** Labels, separators, help text, empty states, timestamps, code fences */
	muted: "#5C6370",
	/** Errors, failed checks, changes requested, conflicts */
	error: "#E06C75",
	/** Loading states, draft badges, pending checks */
	warning: "#E5C07B",
	/** Author names, approved reviews, passed checks, mergeable */
	success: "#98C379",
	/** Repo slugs, URLs */
	info: "#56B6C2",
	/** "you" as blocker */
	selfHighlight: "#C678DD",
	/** Markdown code block content and inline code */
	code: "#7EC8D3",

	/** All text in a selected/highlighted row */
	selectedFg: "#FFFFFF",
	/** Background of selected rows and options */
	selectedBg: "#3E4451",
	/** Borders (e.g. detail view focus card) */
	border: "#61AFEF",
	/** Non-semantic text: spacers, skipped/cancelled/unknown states */
	neutral: "#ABB2BF",

	/** Diff: added lines (future) */
	diffAdded: "#98C379",
	/** Diff: removed lines (future) */
	diffRemoved: "#E06C75",
	/** Diff: unchanged context lines (future) */
	diffContext: "#ABB2BF",
} as const;
