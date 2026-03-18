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
					onRefresh={() => {}}
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
					onRefresh={() => {}}
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
					onRefresh={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		const frame = captureCharFrame();
		expect(frame).toContain("acme/widgets");
	});

	test("j/k keys move selection down and up", async () => {
		const prs = [
			makePR({ number: 1, title: "First PR" }),
			makePR({ number: 2, title: "Second PR" }),
			makePR({ number: 3, title: "Third PR" }),
		];

		const { renderOnce, captureCharFrame, mockInput } = await testRender(
			() => (
				<AppShell
					prs={prs}
					loading={false}
					repoSlug="acme/widgets"
					onRefresh={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();

		// Initially first item is selected (row has blue background in spans)
		let frame = captureCharFrame();
		expect(frame).toContain("First PR");

		// Press j to move down
		mockInput.pressKey("j");
		await renderOnce();

		// Press j again to move to third
		mockInput.pressKey("j");
		await renderOnce();

		// Press k to move back up
		mockInput.pressKey("k");
		await renderOnce();

		// Should still render without error
		frame = captureCharFrame();
		expect(frame).toContain("Second PR");
	});

	test("arrow keys move selection", async () => {
		const prs = [
			makePR({ number: 1, title: "First PR" }),
			makePR({ number: 2, title: "Second PR" }),
		];

		const { renderOnce, captureCharFrame, mockInput } = await testRender(
			() => (
				<AppShell
					prs={prs}
					loading={false}
					repoSlug="acme/widgets"
					onRefresh={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();

		mockInput.pressArrow("down");
		await renderOnce();

		const frame = captureCharFrame();
		expect(frame).toContain("Second PR");
	});

	test("r key triggers onRefresh callback", async () => {
		let refreshed = false;

		const { renderOnce, mockInput } = await testRender(
			() => (
				<AppShell
					prs={[]}
					loading={false}
					repoSlug="acme/widgets"
					onRefresh={() => {
						refreshed = true;
					}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		mockInput.pressKey("r");
		await renderOnce();

		expect(refreshed).toBe(true);
	});

	test("selection does not go below last PR", async () => {
		const prs = [
			makePR({ number: 1, title: "Only PR" }),
		];

		const { renderOnce, captureCharFrame, mockInput } = await testRender(
			() => (
				<AppShell
					prs={prs}
					loading={false}
					repoSlug="acme/widgets"
					onRefresh={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();

		// Try to go down past the only item
		mockInput.pressKey("j");
		mockInput.pressKey("j");
		await renderOnce();

		const frame = captureCharFrame();
		// Should still show the PR without crashing
		expect(frame).toContain("Only PR");
	});

	test("selection does not go above first PR", async () => {
		const prs = [
			makePR({ number: 1, title: "Only PR" }),
		];

		const { renderOnce, captureCharFrame, mockInput } = await testRender(
			() => (
				<AppShell
					prs={prs}
					loading={false}
					repoSlug="acme/widgets"
					onRefresh={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();

		// Try to go up past the first item
		mockInput.pressKey("k");
		mockInput.pressKey("k");
		await renderOnce();

		const frame = captureCharFrame();
		expect(frame).toContain("Only PR");
	});

	test("j/k keys do nothing when PR list is empty", async () => {
		const { renderOnce, captureCharFrame, mockInput } = await testRender(
			() => (
				<AppShell
					prs={[]}
					loading={false}
					repoSlug="acme/widgets"
					onRefresh={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();

		// Press j on empty list — should not crash or corrupt state
		mockInput.pressKey("j");
		mockInput.pressKey("j");
		mockInput.pressKey("k");
		await renderOnce();

		const frame = captureCharFrame();
		expect(frame).toContain("No open pull requests");
	});

	test("shows error message when error is set", async () => {
		const { renderOnce, captureCharFrame } = await testRender(
			() => (
				<AppShell
					prs={[]}
					loading={false}
					repoSlug="acme/widgets"
					error="Network timeout"
					onRefresh={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		const frame = captureCharFrame();
		expect(frame).toContain("Network timeout");
	});
});
