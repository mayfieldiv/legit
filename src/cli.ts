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
 *   pr <n>   — fetch and print single PR detail as JSON
 */

import { Legit } from "./lib/legit";

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

		case "prs":
			return { output: await app.fetchPRs() };

		case "pr": {
			const rawNumber = args[1];
			if (!rawNumber || !/^[1-9]\d*$/.test(rawNumber)) {
				return { error: "Usage: legit pr <number>" };
			}
			const prNumber = Number(rawNumber);
			return { output: await app.fetchPR(app.repoSlug, prNumber) };
		}

		case undefined:
			return { launchTui: true };

		default:
			return {
				error: `Unknown command: ${command}\n\nUsage: legit [detect|auth|config|prs|pr <number>]`,
			};
	}
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
