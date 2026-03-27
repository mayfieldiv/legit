import { describe, test, expect, afterAll } from "bun:test";
import { testRender } from "@opentui/solid";
import { App, prUrl } from "../src/App";
import {
	cleanupTmpDirs,
	makeSampleRestPR,
	mockHttpFetch,
	createTestLegit,
	createMockFetch,
	makeGraphQLResponse,
	SAMPLE_GQL_META,
} from "./helpers";

afterAll(cleanupTmpDirs);

describe("App integration", () => {
	test("renders loading state then PR list after fetch", async () => {
		const app = createTestLegit({
			httpFetch: mockHttpFetch([makeSampleRestPR(1), makeSampleRestPR(2)]),
		});

		const { renderOnce, captureCharFrame } = await testRender(() => <App app={app} />, {
			width: 160,
			height: 20,
		});

		// First render — resource is pending
		await renderOnce();
		const _loadingFrame = captureCharFrame();
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

		const { renderOnce, captureCharFrame } = await testRender(() => <App app={app} />, {
			width: 160,
			height: 20,
		});

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

		const { renderOnce, captureCharFrame } = await testRender(() => <App app={app} />, {
			width: 160,
			height: 20,
		});

		await renderOnce();
		const frame = captureCharFrame();
		expect(frame).toContain("Loading pull requests");
	});

	test("shows error when fetch fails", async () => {
		const { fetch } = createMockFetch([
			{
				url: /pulls/,
				response: { status: 500, body: { message: "Server error" } },
			},
		]);

		const app = createTestLegit({ httpFetch: fetch });

		const { renderOnce, captureCharFrame } = await testRender(() => <App app={app} />, {
			width: 160,
			height: 20,
		});

		// Wait for error to propagate
		await new Promise((r) => setTimeout(r, 50));
		await renderOnce();

		const frame = captureCharFrame();
		expect(frame).toContain("500");
	});

	test("R key triggers full refetch", async () => {
		const { fetch, calls } = createMockFetch([
			{ url: /pulls/, response: { status: 200, body: [] } },
		]);

		const app = createTestLegit({ httpFetch: fetch });

		const { renderOnce, mockInput } = await testRender(() => <App app={app} />, {
			width: 160,
			height: 20,
		});

		// Wait for initial fetch
		await new Promise((r) => setTimeout(r, 50));
		await renderOnce();

		const initialCount = calls.filter((c) => c.url.includes("/pulls")).length;

		// Press R (shift+R) to refetch all
		mockInput.pressKey("r", { shift: true });
		await new Promise((r) => setTimeout(r, 50));
		await renderOnce();

		const newCount = calls.filter((c) => c.url.includes("/pulls")).length;
		expect(newCount).toBeGreaterThan(initialCount);
	});

	test("split layout renders list and summary panel separator", async () => {
		const app = createTestLegit({
			httpFetch: mockHttpFetch([makeSampleRestPR(1)]),
		});

		const { renderOnce, captureCharFrame } = await testRender(() => <App app={app} />, {
			width: 160,
			height: 20,
		});

		await new Promise((r) => setTimeout(r, 50));
		await renderOnce();

		const frame = captureCharFrame();
		expect(frame).toContain("PR #1");
		expect(frame).toContain("│");
	});

	test("loads tracked repos and shows All tab aggregate", async () => {
		const { fetch } = createMockFetch([
			{
				url: /\/repos\/acme\/widgets\/pulls\?/,
				response: { status: 200, body: [makeSampleRestPR(1)] },
			},
			{
				url: /\/graphql/,
				method: "POST",
				response: {
					status: 200,
					body: makeGraphQLResponse([
						{ ...SAMPLE_GQL_META, number: 1, additions: 5, deletions: 1 },
					]),
				},
			},
			{
				url: /\/repos\/acme\/gadgets\/pulls\?/,
				response: { status: 200, body: [makeSampleRestPR(2)] },
			},
			{
				url: /\/graphql/,
				method: "POST",
				response: {
					status: 200,
					body: makeGraphQLResponse([
						{ ...SAMPLE_GQL_META, number: 2, additions: 7, deletions: 2 },
					]),
				},
			},
		]);
		const app = createTestLegit({ httpFetch: fetch });
		app.config.repos = ["acme/widgets", "acme/gadgets"];

		const { renderOnce, captureCharFrame } = await testRender(() => <App app={app} />, {
			// Wide enough to show title column with Threads + Blocker + Repo columns.
			width: 180,
			height: 20,
		});

		await new Promise((r) => setTimeout(r, 100));
		await renderOnce();

		const frame = captureCharFrame();
		expect(frame).toContain("All");
		expect(frame).toContain("acme/widgets");
		expect(frame).toContain("acme/gadgets");
		expect(frame).toContain("PR #1");
		expect(frame).toContain("PR #2");
		// Repo column should show short repo names
		expect(frame).toContain("widgets");
		expect(frame).toContain("gadgets");
		expect(frame).toContain("Repo");
	});

	test("switching tabs keeps a PR selected for summary panel", async () => {
		const { fetch } = createMockFetch([
			{
				url: /\/repos\/acme\/widgets\/pulls\?/,
				response: { status: 200, body: [makeSampleRestPR(1)] },
			},
			{
				url: /\/graphql/,
				method: "POST",
				response: {
					status: 200,
					body: makeGraphQLResponse([{ ...SAMPLE_GQL_META, number: 1 }]),
				},
			},
			{
				url: /\/repos\/acme\/gadgets\/pulls\?/,
				response: { status: 200, body: [makeSampleRestPR(2)] },
			},
			{
				url: /\/graphql/,
				method: "POST",
				response: {
					status: 200,
					body: makeGraphQLResponse([{ ...SAMPLE_GQL_META, number: 2 }]),
				},
			},
		]);
		const app = createTestLegit({ httpFetch: fetch });
		app.config.repos = ["acme/widgets", "acme/gadgets"];

		const { renderOnce, captureCharFrame, mockInput } = await testRender(
			() => <App app={app} />,
			{
				// Wide enough to show title column with Threads + Blocker + Repo columns.
				width: 180,
				height: 20,
			},
		);

		await new Promise((r) => setTimeout(r, 120));
		await renderOnce();
		mockInput.pressKey("3");
		await new Promise((r) => setTimeout(r, 50));
		await renderOnce();

		const frame = captureCharFrame();
		expect(frame).toContain("PR #2");
		expect(frame).not.toContain("No PR selected");
	});

	test("selection resets to first PR when switching back to a tab", async () => {
		// Regression: navigating down on one tab, then switching back to All,
		// left the list highlight on a different row than the summary panel showed.
		const { fetch } = createMockFetch([
			{
				url: /\/repos\/acme\/widgets\/pulls\?/,
				response: {
					status: 200,
					body: [makeSampleRestPR(1), makeSampleRestPR(2), makeSampleRestPR(3)],
				},
			},
			{
				url: /\/graphql/,
				method: "POST",
				response: {
					status: 200,
					body: makeGraphQLResponse([
						{ ...SAMPLE_GQL_META, number: 1 },
						{ ...SAMPLE_GQL_META, number: 2 },
						{ ...SAMPLE_GQL_META, number: 3 },
					]),
				},
			},
			{
				url: /\/repos\/acme\/gadgets\/pulls\?/,
				response: {
					status: 200,
					body: [makeSampleRestPR(10), makeSampleRestPR(11)],
				},
			},
			{
				url: /\/graphql/,
				method: "POST",
				response: {
					status: 200,
					body: makeGraphQLResponse([
						{ ...SAMPLE_GQL_META, number: 10 },
						{ ...SAMPLE_GQL_META, number: 11 },
					]),
				},
			},
		]);
		const app = createTestLegit({ httpFetch: fetch });
		app.config.repos = ["acme/widgets", "acme/gadgets"];

		const { renderOnce, captureCharFrame, mockInput } = await testRender(
			() => <App app={app} />,
			{ width: 150, height: 20 },
		);

		await new Promise((r) => setTimeout(r, 120));
		await renderOnce();

		// All tab — first PR is selected, summary shows PR #1
		let frame = captureCharFrame();
		expect(frame).toContain("PR #1");

		// Switch to acme/widgets tab (tab index 1 → key "1")
		mockInput.pressKey("1");
		await new Promise((r) => setTimeout(r, 50));
		await renderOnce();

		// Move down twice (to PR #3)
		mockInput.pressKey("j");
		mockInput.pressKey("j");
		await renderOnce();
		frame = captureCharFrame();
		expect(frame).toContain("PR #3");

		// Switch back to All tab (key "0")
		mockInput.pressKey("0");
		await new Promise((r) => setTimeout(r, 50));
		await renderOnce();

		// The highlighted row and the summary panel should both show PR #1
		// (the first PR on the All tab), not PR #3 from the old selection
		frame = captureCharFrame();
		const lines = frame.split("\n");
		// Line 0: header, Line 1: tabs, Line 2: column headers, Line 3: first data row
		const firstDataRow = lines[3] ?? "";
		expect(firstDataRow).toContain("#1");
		expect(firstDataRow).not.toContain("#3");
		// Summary panel should show PR #1
		expect(frame).toContain("PR #1");
	});
});

describe("prUrl", () => {
	test("builds correct GitHub PR URL", () => {
		expect(prUrl("acme/widgets", 42)).toBe("https://github.com/acme/widgets/pull/42");
	});
});
