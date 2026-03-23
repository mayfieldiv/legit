import { describe, test, expect } from "bun:test";
import { testRender } from "@opentui/solid";
import { ListView, computeScrollTarget } from "../src/components/ListView";
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
					onRefreshSelected={() => {}}
					onRefreshAll={() => {}}
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
					onRefreshSelected={() => {}}
					onRefreshAll={() => {}}
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
					onRefreshSelected={() => {}}
					onRefreshAll={() => {}}
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

	test("r key triggers onRefreshSelected", async () => {
		let refreshedSelected = false;

		const { renderOnce, mockInput } = await testRender(
			() => (
				<ListView
					prs={[makePR()]}
					onRefreshSelected={() => {
						refreshedSelected = true;
					}}
					onRefreshAll={() => {}}
					onNavigate={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		mockInput.pressKey("r");
		await renderOnce();

		expect(refreshedSelected).toBe(true);
	});

	test("R key triggers onRefreshAll", async () => {
		let refreshedAll = false;

		const { renderOnce, mockInput } = await testRender(
			() => (
				<ListView
					prs={[makePR()]}
					onRefreshSelected={() => {}}
					onRefreshAll={() => {
						refreshedAll = true;
					}}
					onNavigate={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		mockInput.pressKey("r", { shift: true });
		await renderOnce();

		expect(refreshedAll).toBe(true);
	});

	test("uppercase R triggers onRefreshAll", async () => {
		let refreshedAll = false;

		const { renderOnce, mockInput } = await testRender(
			() => (
				<ListView
					prs={[makePR()]}
					onRefreshSelected={() => {}}
					onRefreshAll={() => {
						refreshedAll = true;
					}}
					onNavigate={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		mockInput.pressKey("R");
		await renderOnce();

		expect(refreshedAll).toBe(true);
	});

	test("Enter key triggers onNavigate with detail view", async () => {
		let navigated: unknown = null;
		const pr = makePR({ number: 42, title: "Test PR" });

		const { renderOnce, mockInput } = await testRender(
			() => (
				<ListView
					prs={[pr]}
					onRefreshSelected={() => {}}
					onRefreshAll={() => {}}
					onNavigate={(target) => {
						navigated = target;
					}}
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
					onRefreshSelected={() => {}}
					onRefreshAll={() => {}}
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
					onRefreshSelected={() => {}}
					onRefreshAll={() => {}}
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

	test("fires onSelectionChange when navigating", async () => {
		const prs = [
			makePR({ number: 1, title: "First PR" }),
			makePR({ number: 2, title: "Second PR" }),
		];
		const selections: number[] = [];

		const { renderOnce, mockInput } = await testRender(
			() => (
				<ListView
					prs={prs}
					onRefreshSelected={() => {}}
					onRefreshAll={() => {}}
					onNavigate={() => {}}
					onSelectionChange={(pr) => selections.push(pr.number)}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		mockInput.pressKey("j");
		await renderOnce();

		expect(selections).toEqual([2]);
	});
});

// ── Scroll logic (pure function tests) ──────────────────────────────────────

describe("computeScrollTarget", () => {
	// All tests use viewportHeight=20, which gives margin=2 (10% of 20)
	const scroll = computeScrollTarget;

	test("no scroll when selection is well within viewport", () => {
		expect(scroll({ idx: 10, scrollTop: 0, viewportHeight: 20, direction: "down" })).toBeNull();
		expect(scroll({ idx: 10, scrollTop: 0, viewportHeight: 20, direction: "up" })).toBeNull();
	});

	test("scrolls to margin position when entering bottom margin going down", () => {
		expect(scroll({ idx: 18, scrollTop: 0, viewportHeight: 20, direction: "down" })).toBe(1);
	});

	test("scrolls to margin position when entering top margin going up", () => {
		expect(scroll({ idx: 6, scrollTop: 5, viewportHeight: 20, direction: "up" })).toBe(4);
	});

	test("does not scroll for bottom margin when going up", () => {
		expect(scroll({ idx: 18, scrollTop: 0, viewportHeight: 20, direction: "up" })).toBeNull();
	});

	test("does not scroll for top margin when going down", () => {
		expect(scroll({ idx: 6, scrollTop: 5, viewportHeight: 20, direction: "down" })).toBeNull();
	});

	test("off-screen below: positions near bottom with margin", () => {
		const target = scroll({ idx: 30, scrollTop: 0, viewportHeight: 20, direction: "down" });
		expect(target).toBe(13);
		expect(30 - target!).toBe(17); // margin=2 from bottom
	});

	test("off-screen above: positions near top with margin", () => {
		const target = scroll({ idx: 5, scrollTop: 20, viewportHeight: 20, direction: "up" });
		expect(target).toBe(3);
		expect(5 - target!).toBe(2); // margin=2 from top
	});

	test("clamps to 0 when selection is near top", () => {
		expect(scroll({ idx: 0, scrollTop: 10, viewportHeight: 20, direction: "up" })).toBe(0);
		expect(scroll({ idx: 1, scrollTop: 10, viewportHeight: 20, direction: "up" })).toBe(0);
	});

	test("continuous j keeps selection at margin distance from bottom", () => {
		let scrollTop = 0;
		for (let idx = 0; idx < 40; idx++) {
			const target = scroll({ idx, scrollTop, viewportHeight: 20, direction: "down" });
			if (target !== null) scrollTop = target;
		}
		expect(39 - scrollTop).toBe(17);
	});

	test("continuous k keeps selection at margin distance from top", () => {
		let scrollTop = 30;
		for (let idx = 39; idx >= 0; idx--) {
			const target = scroll({ idx, scrollTop, viewportHeight: 20, direction: "up" });
			if (target !== null) scrollTop = target;
		}
		expect(scrollTop).toBe(0);
	});

	test("in margin zone: repositions to margin distance on j", () => {
		const target = scroll({ idx: 18, scrollTop: 0, viewportHeight: 20, direction: "down" });
		expect(target).toBe(1);
		expect(18 - target!).toBe(17);
	});

	test("off-screen above: appears near top on j", () => {
		expect(scroll({ idx: 1, scrollTop: 10, viewportHeight: 20, direction: "down" })).toBe(0);
	});

	test("off-screen below: appears near bottom on k", () => {
		expect(scroll({ idx: 30, scrollTop: 0, viewportHeight: 20, direction: "up" })).toBe(13);
	});

	test("far off-screen below: repositions with margin on j", () => {
		const target = scroll({ idx: 50, scrollTop: 0, viewportHeight: 20, direction: "down" });
		expect(target).toBe(33);
		expect(50 - target!).toBe(17);
	});

	test("far off-screen above: repositions with margin on k", () => {
		const target = scroll({ idx: 5, scrollTop: 40, viewportHeight: 20, direction: "up" });
		expect(target).toBe(3);
		expect(5 - target!).toBe(2);
	});

	test("each j scrolls by 1 once in margin zone", () => {
		expect(scroll({ idx: 17, scrollTop: 0, viewportHeight: 20, direction: "down" })).toBeNull();
		expect(scroll({ idx: 18, scrollTop: 0, viewportHeight: 20, direction: "down" })).toBe(1);
		expect(scroll({ idx: 19, scrollTop: 1, viewportHeight: 20, direction: "down" })).toBe(2);
		expect(scroll({ idx: 20, scrollTop: 2, viewportHeight: 20, direction: "down" })).toBe(3);
	});
});
