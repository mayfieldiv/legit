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

import { detectRepo } from "./lib/detect-repo";
import { resolveAuth } from "./lib/auth";
import {
	loadConfig,
	saveConfig,
	addRepo,
	DEFAULT_CONFIG,
} from "./lib/config";
import { createGitHubClient } from "./lib/github-client";

const CONFIG_PATH =
	process.env.LEGIT_CONFIG_PATH ??
	`${process.env.HOME}/.config/legit/config.json`;

const args = process.argv.slice(2);
const command = args[0];

function fail(message: string): never {
	console.error(message);
	process.exit(1);
}

function ensureConfig(auth?: { user: string }) {
	let config = loadConfig(CONFIG_PATH);

	// Auto-detect user if not set
	if (!config.user && auth) {
		config = { ...config, user: auth.user };
		saveConfig(CONFIG_PATH, config);
	}

	return config;
}

try {
	switch (command) {
		case "detect": {
			const repo = detectRepo();
			console.log(JSON.stringify(repo, null, "\t"));
			break;
		}

		case "auth": {
			const auth = resolveAuth();
			console.log(
				JSON.stringify(
					{ user: auth.user, tokenSource: auth.tokenSource },
					null,
					"\t",
				),
			);
			break;
		}

		case "config": {
			try {
				const auth = resolveAuth();
				const config = ensureConfig(auth);
				console.log(JSON.stringify(config, null, "\t"));
			} catch {
				const config = ensureConfig();
				console.log(JSON.stringify(config, null, "\t"));
			}
			break;
		}

		case "prs": {
			const auth = resolveAuth();
			const config = ensureConfig(auth);
			const client = createGitHubClient(auth.token);
			const repo = detectRepo();
			const repoSlug = `${repo.owner}/${repo.repo}`;

			// Auto-add repo to config if not tracked
			if (!config.repos.includes(repoSlug)) {
				const updated = addRepo(config, repoSlug);
				saveConfig(CONFIG_PATH, updated);
			}

			const prs = await client.fetchOpenPRs(repoSlug);
			console.log(JSON.stringify(prs, null, "\t"));
			break;
		}

		case "pr": {
			const prNumber = parseInt(args[1], 10);
			if (isNaN(prNumber)) {
				fail("Usage: legit pr <number>");
			}

			const auth = resolveAuth();
			const client = createGitHubClient(auth.token);
			const repo = detectRepo();
			const repoSlug = `${repo.owner}/${repo.repo}`;

			const pr = await client.fetchPR(repoSlug, prNumber);
			console.log(JSON.stringify(pr, null, "\t"));
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
