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

const args = process.argv.slice(2);
const command = args[0];

function fail(message: string): never {
	console.error(message);
	process.exit(1);
}

function print(data: unknown): void {
	console.log(JSON.stringify(data, null, "\t"));
}

try {
	const app = new Legit();

	switch (command) {
		case "detect":
			print(app.repo);
			break;

		case "auth":
			print({ user: app.auth.user, tokenSource: app.auth.tokenSource });
			break;

		case "config":
			print(app.config);
			break;

		case "prs":
			print(await app.fetchPRs());
			break;

		case "pr": {
			const prNumber = parseInt(args[1], 10);
			if (isNaN(prNumber)) {
				fail("Usage: legit pr <number>");
			}
			print(await app.fetchPR(app.repoSlug, prNumber));
			break;
		}

		case undefined: {
			// Register the Solid JSX transform plugin before importing any .tsx files.
			// bunfig.toml only applies relative to CWD, but legit runs from anywhere.
			await import("@opentui/solid/preload");
			const { render } = await import("@opentui/solid");
			const { default: App } = await import("./App");
			await render(App);
			break;
		}

		default:
			fail(
				`Unknown command: ${command}\n\nUsage: legit [detect|auth|config|prs|pr <number>]`,
			);
	}
} catch (err: any) {
	fail(err.message ?? String(err));
}
