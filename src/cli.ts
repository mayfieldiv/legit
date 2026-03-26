#!/usr/bin/env bun
/**
 * CLI entry point for legit.
 *
 * Subcommands:
 *   (none)   — launch TUI
 *   detect   — print detected owner/repo as JSON
 *   auth     — print auth info as JSON (no token)
 *   config   — print current config as JSON
 *   prs      — fetch and print open PRs as JSON
 *   pr <n>   — fetch and print PR summary as JSON (PR detail plus checks, reviews, comment threads, files)
 *   files <n> — fetch and print file categorization as JSON
 *   blocker <n> — compute and print blocker/tier/reason as JSON
 */

import { Legit } from "./lib/legit";
import { computeBlocker } from "./lib/blocker-engine";
import { processPRList } from "./lib/group-filter-engine";
import type { GroupByKey, SortByKey, SortDir } from "./lib/group-filter-engine";
import type { PR, FileChange } from "./lib/types";

export interface CommandResult {
	output?: unknown;
	error?: string;
	launchTui?: boolean;
}

/**
 * Execute a CLI subcommand. Returns structured result for testability.
 * The thin entry point below handles printing and process.exit.
 */
export async function runCommand(args: string[], app: Legit): Promise<CommandResult> {
	const command = args[0];

	switch (command) {
		case "detect":
			return { output: app.repo };

		case "auth":
			return {
				output: { user: app.auth.user, tokenSource: app.auth.tokenSource },
			};

		case "config":
			return { output: app.config };

		case "repos":
			return { output: app.config.repos };

		case "prs": {
			const options = parsePrsArgs(args.slice(1));
			if (options.error) {
				return { error: options.error };
			}

			const useEngine =
				options.groupBy !== undefined ||
				options.sortBy !== undefined ||
				options.filter !== undefined;

			if (options.all) {
				const repos = trackedRepos(app);
				const byRepo: Record<string, PR[]> = {};
				for (const repo of repos) {
					let prs: PR[] = [];
					for await (const snapshot of app.fetchPRs(repo)) {
						prs = snapshot;
					}
					if (options.withBlockers) {
						const currentUser = app.currentUser;
						byRepo[repo] = prs.map((pr) => ({
							...pr,
							...computeBlocker(pr, currentUser),
						}));
					} else {
						byRepo[repo] = prs;
					}
				}
				return { output: byRepo };
			}

			let prs: PR[] = [];
			for await (const snapshot of app.fetchPRs(options.repo)) {
				prs = snapshot;
			}

			if (options.withBlockers) {
				const currentUser = app.currentUser;
				return {
					output: prs.map((pr) => ({ ...pr, ...computeBlocker(pr, currentUser) })),
				};
			}

			if (useEngine) {
				return {
					output: processPRList(prs, {
						groupBy: options.groupBy,
						sortBy: options.sortBy,
						sortDir: options.sortDir,
						filterText: options.filter,
						currentUser: app.currentUser,
					}),
				};
			}

			return { output: prs };
		}

		case "pr": {
			const rawNumber = args[1];
			if (!rawNumber || !/^[1-9]\d*$/.test(rawNumber)) {
				return { error: "Usage: legit pr <number>" };
			}
			const prNumber = Number(rawNumber);
			return { output: await app.fetchPRSummary(app.repoSlug, prNumber) };
		}

		case "files": {
			const rawNumber = args[1];
			if (!rawNumber || !/^[1-9]\d*$/.test(rawNumber)) {
				return { error: "Usage: legit files <number>" };
			}
			const prNumber = Number(rawNumber);
			let files: FileChange[] = [];
			for await (const snapshot of app.fetchFiles(app.repoSlug, prNumber)) {
				files = snapshot;
			}
			return { output: app.categorizeFiles(files) };
		}

		case "blocker": {
			const rawNumber = args[1];
			if (!rawNumber || !/^[1-9]\d*$/.test(rawNumber)) {
				return { error: "Usage: legit blocker <number>" };
			}
			const prNumber = Number(rawNumber);
			const summary = await app.fetchPRSummary(app.repoSlug, prNumber);
			const currentUser = app.currentUser;
			return {
				output: computeBlocker(summary, currentUser, {
					checks: summary.checks,
					reviews: summary.reviews,
				}),
			};
		}

		case undefined:
			return { launchTui: true };

		default:
			return {
				error: `Unknown command: ${command}\n\nUsage: legit [detect|auth|config|repos|prs [--repo=<owner/repo>|--all]|pr <number>|files <number>|blocker <number>]`,
			};
	}
}

function trackedRepos(app: Legit): string[] {
	return app.trackedRepos();
}

const VALID_GROUP_BY: GroupByKey[] = [
	"smart-status",
	"author",
	"repo",
	"size-category",
	"label",
	"none",
];

// "status" is a user-friendly alias for "smart-status"
const GROUP_BY_ALIASES: Record<string, GroupByKey> = {
	status: "smart-status",
};

const VALID_SORT_BY: SortByKey[] = ["size", "age", "updated"];
const VALID_SORT_DIR: SortDir[] = ["asc", "desc"];

const USAGE_PRS =
	"Usage: legit prs [--repo=<owner/repo>|--all] [--with-blockers] [--group-by=<key>] [--sort-by=<size|age|updated>] [--sort-dir=<asc|desc>] [--filter=<text>]";

function parsePrsArgs(args: string[]): {
	repo?: string;
	all: boolean;
	withBlockers: boolean;
	groupBy?: GroupByKey;
	sortBy?: SortByKey;
	sortDir?: SortDir;
	filter?: string;
	error?: string;
} {
	let repo: string | undefined;
	let all = false;
	let withBlockers = false;
	let groupBy: GroupByKey | undefined;
	let sortBy: SortByKey | undefined;
	let sortDir: SortDir | undefined;
	let filter: string | undefined;

	for (const arg of args) {
		if (arg === "--all") {
			all = true;
		} else if (arg === "--with-blockers") {
			withBlockers = true;
		} else if (arg.startsWith("--repo=")) {
			repo = arg.slice("--repo=".length);
		} else if (arg.startsWith("--group-by=")) {
			const val = arg.slice("--group-by=".length);
			const resolved =
				GROUP_BY_ALIASES[val] ??
				(VALID_GROUP_BY.includes(val as GroupByKey) ? (val as GroupByKey) : undefined);
			if (!resolved) {
				const allKeys = [...Object.keys(GROUP_BY_ALIASES), ...VALID_GROUP_BY];
				return {
					all: false,
					withBlockers: false,
					error: `Invalid --group-by value: "${val}". Valid keys: ${allKeys.join(", ")}`,
				};
			}
			groupBy = resolved;
		} else if (arg.startsWith("--sort-by=")) {
			const val = arg.slice("--sort-by=".length);
			if (!VALID_SORT_BY.includes(val as SortByKey)) {
				return {
					all: false,
					withBlockers: false,
					error: `Invalid --sort-by value: "${val}". Valid keys: ${VALID_SORT_BY.join(", ")}`,
				};
			}
			sortBy = val as SortByKey;
		} else if (arg.startsWith("--sort-dir=")) {
			const val = arg.slice("--sort-dir=".length);
			if (!VALID_SORT_DIR.includes(val as SortDir)) {
				return {
					all: false,
					withBlockers: false,
					error: `Invalid --sort-dir value: "${val}". Valid values: ${VALID_SORT_DIR.join(", ")}`,
				};
			}
			sortDir = val as SortDir;
		} else if (arg.startsWith("--filter=")) {
			filter = arg.slice("--filter=".length);
		} else {
			return { all: false, withBlockers: false, error: USAGE_PRS };
		}
	}

	if (all && repo) {
		return { all: false, withBlockers: false, error: USAGE_PRS };
	}

	if (withBlockers && (groupBy ?? sortBy ?? filter) !== undefined) {
		return {
			all: false,
			withBlockers: false,
			error: "--with-blockers cannot be combined with --group-by, --sort-by, or --filter",
		};
	}

	return { repo, all, withBlockers, groupBy, sortBy, sortDir, filter };
}

// ── Entry point ─────────────────────────────────────────────────────────────

if (import.meta.main) {
	try {
		const app = new Legit();
		const result = await runCommand(process.argv.slice(2), app);

		if (result.error) {
			console.error(result.error);
			process.exit(1);
		}

		if (result.launchTui) {
			// Register the Solid JSX transform before importing any .tsx files.
			// Can't rely on bunfig.toml alone — legit runs from arbitrary cwd.
			const { plugin } = await import("bun");
			const { default: solidPlugin } = await import("@opentui/solid/bun-plugin");
			plugin(solidPlugin);
			const { render } = await import("@opentui/solid");
			const { createApp } = await import("./App");
			await render(createApp(app));
		} else if (result.output !== undefined) {
			console.log(JSON.stringify(result.output, null, "\t"));
		}
	} catch (err: any) {
		console.error(err.message ?? String(err));
		process.exit(1);
	}
}
