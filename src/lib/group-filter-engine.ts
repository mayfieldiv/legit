/**
 * Grouping, Sorting, and Filtering Engine
 *
 * Pure function that takes a flat list of PRs and produces a grouped,
 * sorted, filtered result based on user preferences.
 *
 * No side effects — all inputs are passed explicitly.
 */

import type { PR } from "./types";
import { computeBlocker, compareTiers, tierLabel, type Tier } from "./blocker-engine";

// ── Public types ──────────────────────────────────────────────────────────────

export type GroupByKey = "smart-status" | "author" | "repo" | "size-category" | "label" | "none";

export type SortByKey = "size" | "age" | "updated";
export type SortDir = "asc" | "desc";

export interface ProcessOptions {
	/** How to group PRs. Default: "none" (flat list). */
	groupBy?: GroupByKey;
	/** Field to sort by within each group. Default: undefined (preserve input order). */
	sortBy?: SortByKey;
	/** Sort direction. Default: "desc". */
	sortDir?: SortDir;
	/** Full-text filter string. Default: "" (no filter). */
	filterText?: string;
	/** Current user login — required for smart-status grouping. */
	currentUser?: string;
}

export interface PRGroup {
	/** Stable key for this group (e.g. author login, tier name, label name). */
	key: string;
	/** Display label for this group. */
	label: string;
	/** PRs in this group, sorted. */
	prs: PR[];
}

export interface GroupedResult {
	/** Groups in priority/alphabetical order. Empty groups are omitted. */
	groups: PRGroup[];
	/** Total number of PRs that matched the filter (sum of all group sizes). */
	totalMatched: number;
}

// ── Size category ─────────────────────────────────────────────────────────────

const SIZE_ORDER = ["small", "medium", "large"] as const;
type SizeCategory = (typeof SIZE_ORDER)[number];

function sizeCategory(pr: PR): SizeCategory {
	const total = pr.additions + pr.deletions;
	if (total < 100) return "small";
	if (total <= 500) return "medium";
	return "large";
}

// ── Filtering ─────────────────────────────────────────────────────────────────

function matchesPR(pr: PR, filterText: string): boolean {
	const trimmed = filterText.trim();
	if (!trimmed) return true;

	// "#N" syntax: exact PR number match only — does not search title/author
	if (trimmed.startsWith("#")) {
		const numStr = trimmed.slice(1);
		return /^\d+$/.test(numStr) && pr.number === Number(numStr);
	}

	const lower = trimmed.toLowerCase();

	// PR number as plain string
	if (String(pr.number) === trimmed) return true;

	// Text fields (case-insensitive)
	if (pr.title.toLowerCase().includes(lower)) return true;
	if (pr.author.toLowerCase().includes(lower)) return true;
	if (pr.labels.some((l) => l.toLowerCase().includes(lower))) return true;
	if (pr.requestedReviewers.some((r) => r.toLowerCase().includes(lower))) return true;
	if (pr.assignees.some((a) => a.toLowerCase().includes(lower))) return true;

	return false;
}

// ── Sorting ───────────────────────────────────────────────────────────────────

function sortPRs(prs: PR[], sortBy: SortByKey | undefined, sortDir: SortDir): PR[] {
	if (!sortBy) return prs; // preserve input order

	return [...prs].toSorted((a, b) => {
		let diff: number;
		switch (sortBy) {
			case "size":
				diff = a.additions + a.deletions - (b.additions + b.deletions);
				break;
			case "age":
				diff = new Date(a.createdAt).getTime() - new Date(b.createdAt).getTime();
				break;
			case "updated":
				diff = new Date(a.updatedAt).getTime() - new Date(b.updatedAt).getTime();
				break;
		}
		return sortDir === "asc" ? diff : -diff;
	});
}

// ── Grouping ─────────────────────────────────────────────────────────────────

function groupByKey(prs: PR[], key: GroupByKey, currentUser?: string): PRGroup[] {
	switch (key) {
		case "none": {
			if (prs.length === 0) return [];
			return [{ key: "", label: "", prs }];
		}

		case "author": {
			const map = new Map<string, PR[]>();
			for (const pr of prs) {
				const k = pr.author;
				if (!map.has(k)) map.set(k, []);
				map.get(k)!.push(pr);
			}
			return [...map.entries()]
				.toSorted(([a], [b]) => a.localeCompare(b))
				.map(([k, list]) => ({ key: k, label: k, prs: list }));
		}

		case "repo": {
			const map = new Map<string, PR[]>();
			for (const pr of prs) {
				const k = pr.repoSlug ?? "unknown";
				if (!map.has(k)) map.set(k, []);
				map.get(k)!.push(pr);
			}
			return [...map.entries()]
				.toSorted(([a], [b]) => a.localeCompare(b))
				.map(([k, list]) => ({ key: k, label: k, prs: list }));
		}

		case "size-category": {
			const map = new Map<SizeCategory, PR[]>();
			for (const pr of prs) {
				const cat = sizeCategory(pr);
				if (!map.has(cat)) map.set(cat, []);
				map.get(cat)!.push(pr);
			}
			return SIZE_ORDER.filter((cat) => map.has(cat)).map((cat) => ({
				key: cat,
				label: cat,
				prs: map.get(cat)!,
			}));
		}

		case "label": {
			const labeled = new Map<string, PR[]>();
			const unlabeled: PR[] = [];
			for (const pr of prs) {
				const lbl = pr.labels[0];
				if (!lbl) {
					unlabeled.push(pr);
				} else {
					if (!labeled.has(lbl)) labeled.set(lbl, []);
					labeled.get(lbl)!.push(pr);
				}
			}
			const groups: PRGroup[] = [...labeled.entries()]
				.toSorted(([a], [b]) => a.localeCompare(b))
				.map(([k, list]) => ({ key: k, label: k, prs: list }));
			if (unlabeled.length > 0) {
				groups.push({ key: "unlabeled", label: "Unlabeled", prs: unlabeled });
			}
			return groups;
		}

		case "smart-status": {
			const user = currentUser ?? "";
			const tierMap = new Map<Tier, PR[]>();
			for (const pr of prs) {
				const { tier } = computeBlocker(pr, user);
				if (!tierMap.has(tier)) tierMap.set(tier, []);
				tierMap.get(tier)!.push(pr);
			}
			return [...tierMap.entries()]
				.toSorted(([a], [b]) => compareTiers(a, b))
				.map(([tier, list]) => ({
					key: tier,
					label: tierLabel(tier),
					prs: list,
				}));
		}
	}
}

// ── Main entry point ──────────────────────────────────────────────────────────

/**
 * Process a flat PR list by applying filter, grouping, and sorting.
 *
 * - Filtering is applied first to produce the matched set.
 * - Grouping is applied to the matched set.
 * - Sorting is applied within each group.
 * - Empty groups are omitted.
 */
export function processPRList(prs: PR[], options: ProcessOptions = {}): GroupedResult {
	const { groupBy = "none", sortBy, sortDir = "desc", filterText = "", currentUser } = options;

	// Step 1: Filter
	const matched = prs.filter((pr) => matchesPR(pr, filterText));

	// Step 2: Group
	const groups = groupByKey(matched, groupBy, currentUser);

	// Step 3: Sort within each group
	const sortedGroups =
		sortBy != null
			? groups.map((g) => ({ ...g, prs: sortPRs(g.prs, sortBy, sortDir) }))
			: groups;

	// Step 4: Drop empty groups (shouldn't happen but guard for safety)
	const nonEmpty = sortedGroups.filter((g) => g.prs.length > 0);

	const totalMatched = nonEmpty.reduce((sum, g) => sum + g.prs.length, 0);

	return { groups: nonEmpty, totalMatched };
}
