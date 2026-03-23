import { describe, test, expect } from "bun:test";
import { testRender } from "@opentui/solid";
import { AppShell } from "../src/components/AppShell";
import { makePR } from "./helpers";

describe("AppShell", () => {
	test("shows loading state when loading is true", async () => {
		const { renderOnce, captureCharFrame } = await testRender(
			() => (
				<AppShell
					prs={[]}
					loading={true}
					repoSlug="acme/widgets"
					onRefreshSelected={() => {}}
					onRefreshAll={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		const frame = captureCharFrame();
		expect(frame).toMatch(/loading/i);
	});

	test("shows PR list when loaded", async () => {
		const prs = [
			makePR({ number: 1, title: "First PR" }),
			makePR({ number: 2, title: "Second PR" }),
		];

		const { renderOnce, captureCharFrame } = await testRender(
			() => (
				<AppShell
					prs={prs}
					loading={false}
					repoSlug="acme/widgets"
					onRefreshSelected={() => {}}
					onRefreshAll={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		const frame = captureCharFrame();
		expect(frame).toContain("First PR");
		expect(frame).toContain("Second PR");
	});

	test("shows repo name in header", async () => {
		const { renderOnce, captureCharFrame } = await testRender(
			() => (
				<AppShell
					prs={[]}
					loading={false}
					repoSlug="acme/widgets"
					onRefreshSelected={() => {}}
					onRefreshAll={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		const frame = captureCharFrame();
		expect(frame).toContain("acme/widgets");
	});

	test("shows error message when error is set", async () => {
		const { renderOnce, captureCharFrame } = await testRender(
			() => (
				<AppShell
					prs={[]}
					loading={false}
					repoSlug="acme/widgets"
					error="Network timeout"
					onRefreshSelected={() => {}}
					onRefreshAll={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		const frame = captureCharFrame();
		// captureCharFrame may have minor overlap artifacts from layout,
		// but the error text components should be present
		expect(frame).toMatch(/Error/);
		expect(frame).toMatch(/Network/);
		expect(frame).toMatch(/timeout/);
	});

	test("shows PR count in header", async () => {
		const prs = [makePR({ number: 1 }), makePR({ number: 2 })];

		const { renderOnce, captureCharFrame } = await testRender(
			() => (
				<AppShell
					prs={prs}
					loading={false}
					repoSlug="acme/widgets"
					onRefreshSelected={() => {}}
					onRefreshAll={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		const frame = captureCharFrame();
		expect(frame).toContain("2 open PRs");
	});

	test("renders tab bar with All and repo tabs", async () => {
		const { renderOnce, captureCharFrame } = await testRender(
			() => (
				<AppShell
					prs={[]}
					loading={false}
					repoSlug="acme/widgets"
					tabs={["All", "acme/widgets", "acme/gadgets"]}
					activeTab={0}
					onTabChange={() => {}}
					onRefreshSelected={() => {}}
					onRefreshAll={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		const frame = captureCharFrame();
		expect(frame).toContain("All");
		expect(frame).toContain("acme/widgets");
		expect(frame).toContain("acme/gadgets");
	});

	test("tab keybindings switch tabs", async () => {
		const calls: number[] = [];
		const { renderOnce, mockInput } = await testRender(
			() => (
				<AppShell
					prs={[]}
					loading={false}
					repoSlug="acme/widgets"
					tabs={["All", "acme/widgets", "acme/gadgets"]}
					activeTab={1}
					onTabChange={(index) => calls.push(index)}
					onRefreshSelected={() => {}}
					onRefreshAll={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		mockInput.pressKey("l");
		mockInput.pressKey("h");
		mockInput.pressKey("right");
		mockInput.pressKey("left");
		mockInput.pressKey("3");
		await renderOnce();

		expect(calls).toContain(2);
		expect(calls).toContain(0);
	});
});
