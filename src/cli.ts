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
 */

import { Legit } from "./lib/legit";
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
			if (options.all) {
				const repos = trackedRepos(app);
				const byRepo: Record<string, PR[]> = {};
				for (const repo of repos) {
					let prs: PR[] = [];
					for await (const snapshot of app.fetchPRs(repo)) {
						prs = snapshot;
					}
					byRepo[repo] = prs;
				}
				return { output: byRepo };
			}

			let prs: PR[] = [];
			for await (const snapshot of app.fetchPRs(options.repo)) {
				prs = snapshot;
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

		case undefined:
			return { launchTui: true };

		default:
			return {
				error: `Unknown command: ${command}\n\nUsage: legit [detect|auth|config|repos|prs [--repo=<owner/repo>|--all]|pr <number>|files <number>]`,
			};
	}
}

function trackedRepos(app: Legit): string[] {
	return app.trackedRepos();
}

function parsePrsArgs(args: string[]): { repo?: string; all: boolean; error?: string } {
	let repo: string | undefined;
	let all = false;
	for (const arg of args) {
		if (arg === "--all") {
			all = true;
		} else if (arg.startsWith("--repo=")) {
			repo = arg.slice("--repo=".length);
		} else {
			return { all: false, error: "Usage: legit prs [--repo=<owner/repo>|--all]" };
		}
	}
	if (all && repo) {
		return {
			all: false,
			error: "Usage: legit prs [--repo=<owner/repo>|--all]",
		};
	}
	return { repo, all };
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
