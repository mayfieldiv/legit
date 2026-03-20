import { describe, test, expect, afterAll } from "bun:test";
import {
	loadConfig,
	saveConfig,
	addRepo,
	removeRepo,
	DEFAULT_CONFIG,
	type LegitConfig,
} from "../src/lib/config";
import { writeFileSync } from "fs";
import { join } from "path";
import { cleanupTmpDirs, tmpConfigPath } from "./helpers";
import { mkdtempSync } from "fs";
import { tmpdir } from "os";

afterAll(cleanupTmpDirs);

describe("loadConfig", () => {
	test("returns default config when file does not exist", () => {
		const config = loadConfig(tmpConfigPath());
		expect(config).toEqual(DEFAULT_CONFIG);
	});

	test("loads config from existing file", () => {
		const path = tmpConfigPath();
		const custom: LegitConfig = {
			...DEFAULT_CONFIG,
			user: "testuser",
			repos: ["acme/widgets"],
		};
		saveConfig(path, custom);
		const loaded = loadConfig(path);
		expect(loaded.user).toBe("testuser");
		expect(loaded.repos).toEqual(["acme/widgets"]);
	});

	test("fills missing fields with defaults", () => {
		const path = tmpConfigPath();
		writeFileSync(path, JSON.stringify({ user: "partial" }));
		const loaded = loadConfig(path);
		expect(loaded.user).toBe("partial");
		expect(loaded.repos).toEqual(DEFAULT_CONFIG.repos);
		expect(loaded.botLogins).toEqual(DEFAULT_CONFIG.botLogins);
		expect(loaded.ui).toEqual(DEFAULT_CONFIG.ui);
	});
});

describe("saveConfig", () => {
	test("creates parent directories if needed", () => {
		const dir = mkdtempSync(join(tmpdir(), "legit-config-test-"));
		const path = join(dir, "nested", "deep", "config.json");
		saveConfig(path, DEFAULT_CONFIG);
		const loaded = loadConfig(path);
		expect(loaded).toEqual(DEFAULT_CONFIG);
	});

	test("roundtrips config correctly", () => {
		const path = tmpConfigPath();
		const config: LegitConfig = {
			user: "mayfield",
			repos: ["acme/widgets", "acme/gadgets"],
			botLogins: ["app/devin-ai-integration", "app/copilot-swe-agent"],
			fileRules: [{ pattern: "**/migrations/**", category: "generated" }],
			ui: { defaultGroupBy: "smart-status", defaultSortBy: "updated" },
		};
		saveConfig(path, config);
		const loaded = loadConfig(path);
		expect(loaded).toEqual(config);
	});
});

describe("addRepo", () => {
	test("adds a repo to the config", () => {
		const config = { ...DEFAULT_CONFIG, repos: ["acme/widgets"] };
		const updated = addRepo(config, "acme/gadgets");
		expect(updated.repos).toEqual(["acme/widgets", "acme/gadgets"]);
	});

	test("does not add duplicate repos", () => {
		const config = { ...DEFAULT_CONFIG, repos: ["acme/widgets"] };
		const updated = addRepo(config, "acme/widgets");
		expect(updated.repos).toEqual(["acme/widgets"]);
	});
});

describe("removeRepo", () => {
	test("removes a repo from the config", () => {
		const config = {
			...DEFAULT_CONFIG,
			repos: ["acme/widgets", "acme/gadgets"],
		};
		const updated = removeRepo(config, "acme/widgets");
		expect(updated.repos).toEqual(["acme/gadgets"]);
	});

	test("returns unchanged config when repo not found", () => {
		const config = { ...DEFAULT_CONFIG, repos: ["acme/widgets"] };
		const updated = removeRepo(config, "acme/nonexistent");
		expect(updated.repos).toEqual(["acme/widgets"]);
	});
});
