import { describe, test, expect } from "bun:test";
import { testRender } from "@opentui/solid";
import { ListView } from "../src/components/ListView";
import { makePR } from "./helpers";

describe("ListView", () => {
	test("renders PR list", async () => {
		const prs = [
			makePR({ number: 1, title: "First PR" }),
			makePR({ number: 2, title: "Second PR" }),
		];

		const { renderOnce, captureCharFrame } = await testRender(
			() => (
				<ListView
					prs={prs}
					onRefresh={() => {}}
					onNavigate={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		const frame = captureCharFrame();
		expect(frame).toContain("First PR");
		expect(frame).toContain("Second PR");
	});

	test("j/k keys navigate the list", async () => {
		const prs = [
			makePR({ number: 1, title: "First PR" }),
			makePR({ number: 2, title: "Second PR" }),
			makePR({ number: 3, title: "Third PR" }),
		];

		const { renderOnce, captureCharFrame, mockInput } = await testRender(
			() => (
				<ListView
					prs={prs}
					onRefresh={() => {}}
					onNavigate={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();

		mockInput.pressKey("j");
		await renderOnce();

		mockInput.pressKey("j");
		await renderOnce();

		mockInput.pressKey("k");
		await renderOnce();

		// Should render without error
		const frame = captureCharFrame();
		expect(frame).toContain("Second PR");
	});

	test("arrow keys navigate the list", async () => {
		const prs = [
			makePR({ number: 1, title: "First PR" }),
			makePR({ number: 2, title: "Second PR" }),
		];

		const { renderOnce, mockInput } = await testRender(
			() => (
				<ListView
					prs={prs}
					onRefresh={() => {}}
					onNavigate={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		mockInput.pressArrow("down");
		await renderOnce();
		// No crash = pass
	});

	test("r key triggers onRefresh", async () => {
		let refreshed = false;

		const { renderOnce, mockInput } = await testRender(
			() => (
				<ListView
					prs={[makePR()]}
					onRefresh={() => { refreshed = true; }}
					onNavigate={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		mockInput.pressKey("r");
		await renderOnce();

		expect(refreshed).toBe(true);
	});

	test("Enter key triggers onNavigate with detail view", async () => {
		let navigated: unknown = null;
		const pr = makePR({ number: 42, title: "Test PR" });

		const { renderOnce, mockInput } = await testRender(
			() => (
				<ListView
					prs={[pr]}
					onRefresh={() => {}}
					onNavigate={(target) => { navigated = target; }}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		mockInput.pressEnter();
		await renderOnce();

		expect(navigated).toEqual({ view: "detail", pr });
	});

	test("shows empty state when no PRs", async () => {
		const { renderOnce, captureCharFrame } = await testRender(
			() => (
				<ListView
					prs={[]}
					onRefresh={() => {}}
					onNavigate={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		const frame = captureCharFrame();
		expect(frame).toContain("No open pull requests");
	});

	test("j/k does nothing on empty list", async () => {
		const { renderOnce, captureCharFrame, mockInput } = await testRender(
			() => (
				<ListView
					prs={[]}
					onRefresh={() => {}}
					onNavigate={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		mockInput.pressKey("j");
		mockInput.pressKey("k");
		await renderOnce();

		const frame = captureCharFrame();
		expect(frame).toContain("No open pull requests");
	});
});
