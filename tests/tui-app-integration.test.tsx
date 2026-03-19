import { describe, test, expect, afterAll } from "bun:test";
import { testRender } from "@opentui/solid";
import { App } from "../src/App";
import { Legit, type LegitOptions } from "../src/lib/legit";
import {
	cleanupTmpDirs,
	makeTmpGitRepo,
	tmpConfigPath,
	mockAuthExec,
	mockHttpFetch,
	makeSampleRestPR,
	createMockFetch,
} from "./helpers";

afterAll(cleanupTmpDirs);

function createTestLegit(overrides?: Partial<LegitOptions>): Legit {
	return new Legit({
		cwd: makeTmpGitRepo("git@github.com:acme/widgets.git"),
		configPath: tmpConfigPath(),
		authExec: mockAuthExec(),
		httpFetch: mockHttpFetch([makeSampleRestPR(1), makeSampleRestPR(2)]),
		...overrides,
	});
}

describe("App integration", () => {
	test("renders loading state then PR list after fetch", async () => {
		const app = createTestLegit();

		const { renderOnce, captureCharFrame } = await testRender(
			() => <App app={app} />,
			{ width: 120, height: 20 },
		);

		// First render — resource is pending
		await renderOnce();
		const loadingFrame = captureCharFrame();
		// May show loading or may have already resolved (microtask)
		// Either way, rendering should not throw

		// Give the resource time to resolve
		await new Promise((r) => setTimeout(r, 50));
		await renderOnce();

		const frame = captureCharFrame();
		expect(frame).toContain("acme/widgets");
		expect(frame).toContain("PR #1");
		expect(frame).toContain("PR #2");
	});

	test("shows repo slug in header", async () => {
		const app = createTestLegit();

		const { renderOnce, captureCharFrame } = await testRender(
			() => <App app={app} />,
			{ width: 120, height: 20 },
		);

		await new Promise((r) => setTimeout(r, 50));
		await renderOnce();

		const frame = captureCharFrame();
		expect(frame).toContain("acme/widgets");
	});

	test("shows loading progress text while fetching", async () => {
		const { fetch } = createMockFetch([
			{
				url: /pulls/,
				response: { status: 200, body: [] },
			},
		]);
		const delayedFetch = async (url: string, init?: RequestInit) => {
			await new Promise((r) => setTimeout(r, 25));
			return fetch(url, init);
		};
		const app = createTestLegit({ httpFetch: delayedFetch });

		const { renderOnce, captureCharFrame } = await testRender(
			() => <App app={app} />,
			{ width: 120, height: 20 },
		);

		await renderOnce();
		const frame = captureCharFrame();
		expect(frame).toContain("Loading pull requests");
		expect(frame).toContain("page 1");
	});

	test("shows error when fetch fails", async () => {
		const { fetch } = createMockFetch([
			{
				url: /pulls/,
				response: { status: 500, body: { message: "Server error" } },
			},
		]);

		const app = createTestLegit({ httpFetch: fetch });

		const { renderOnce, captureCharFrame } = await testRender(
			() => <App app={app} />,
			{ width: 120, height: 20 },
		);

		// Wait for error to propagate
		await new Promise((r) => setTimeout(r, 50));
		await renderOnce();

		const frame = captureCharFrame();
		expect(frame).toContain("500");
	});

	test("r key triggers refetch", async () => {
		let fetchCount = 0;
		const { fetch } = createMockFetch([
			{
				url: /pulls/,
				response: { status: 200, body: [] },
			},
		]);

		// Wrap fetch to count calls
		const countingFetch = async (url: string, init?: RequestInit) => {
			if (typeof url === "string" && url.includes("/pulls")) {
				fetchCount++;
			}
			return fetch(url, init);
		};

		const app = createTestLegit({ httpFetch: countingFetch });

		const { renderOnce, mockInput } = await testRender(
			() => <App app={app} />,
			{ width: 120, height: 20 },
		);

		// Wait for initial fetch
		await new Promise((r) => setTimeout(r, 50));
		await renderOnce();

		const initialCount = fetchCount;

		// Press r to refetch
		mockInput.pressKey("r");
		await new Promise((r) => setTimeout(r, 50));
		await renderOnce();

		expect(fetchCount).toBeGreaterThan(initialCount);
	});
});
