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

	test("o key triggers onOpenInBrowser with selected PR", async () => {
		let openedPr: any = null;
		const pr = makePR({ number: 42 });

		const { renderOnce, mockInput } = await testRender(
			() => (
				<ListView
					prs={[pr]}
					onRefreshSelected={() => {}}
					onRefreshAll={() => {}}
					onNavigate={() => {}}
					onOpenInBrowser={(p) => {
						openedPr = p;
					}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		mockInput.pressKey("o");
		await renderOnce();

		expect(openedPr).not.toBeNull();
		expect(openedPr.number).toBe(42);
	});

	test("o key does nothing when no onOpenInBrowser handler", async () => {
		const { renderOnce, mockInput } = await testRender(
			() => (
				<ListView
					prs={[makePR()]}
					onRefreshSelected={() => {}}
					onRefreshAll={() => {}}
					onNavigate={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		// Should not throw
		mockInput.pressKey("o");
		await renderOnce();
	});
});

// ── Filter mode ──────────────────────────────────────────────────────────────

describe("ListView — filter", () => {
	test("/ key activates filter mode and shows filter bar", async () => {
		const prs = [
			makePR({ number: 1, title: "Fix bug" }),
			makePR({ number: 2, title: "Add feature" }),
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
		mockInput.pressKey("/");
		await renderOnce();

		const frame = captureCharFrame();
		// Filter bar should appear
		expect(frame).toMatch(/[Ff]ilter/i);
	});

	test("typing in filter mode narrows the visible PRs", async () => {
		const prs = [
			makePR({ number: 1, title: "Fix bug" }),
			makePR({ number: 2, title: "Add feature" }),
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
		mockInput.pressKey("/");
		await renderOnce();

		// Type "fix"
		mockInput.pressKey("f");
		mockInput.pressKey("i");
		mockInput.pressKey("x");
		await renderOnce();

		const frame = captureCharFrame();
		expect(frame).toContain("Fix bug");
		expect(frame).not.toContain("Add feature");
	});

	test("Escape clears filter and restores all PRs", async () => {
		const prs = [
			makePR({ number: 1, title: "Fix bug" }),
			makePR({ number: 2, title: "Add feature" }),
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
		mockInput.pressKey("/");
		await renderOnce();
		mockInput.pressKey("f");
		mockInput.pressKey("i");
		mockInput.pressKey("x");
		await renderOnce();

		// Escape closes filter — requires small delay for ESC buffer flush (default 10ms timeout)
		mockInput.pressEscape();
		await new Promise((r) => setTimeout(r, 20));
		await renderOnce();

		const frame = captureCharFrame();
		expect(frame).toContain("Fix bug");
		expect(frame).toContain("Add feature");
	});

	test("backspace removes last char from filter text", async () => {
		const prs = [
			makePR({ number: 1, title: "Fix bug" }),
			makePR({ number: 2, title: "Add feature" }),
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
		mockInput.pressKey("/");
		await renderOnce();
		mockInput.pressKey("f");
		mockInput.pressKey("i");
		mockInput.pressKey("x");
		await renderOnce();

		// Both PRs hidden by "fix" filter (only PR1 matches)
		const frameBefore = captureCharFrame();
		expect(frameBefore).not.toContain("Add feature");

		// Backspace three times to clear "fix" — use pressBackspace() which sends \b (0x08)
		mockInput.pressBackspace();
		mockInput.pressBackspace();
		mockInput.pressBackspace();
		await renderOnce();

		const frameAfter = captureCharFrame();
		expect(frameAfter).toContain("Add feature");
	});

	test("j/k navigation still works when filter is active", async () => {
		const prs = [makePR({ number: 1, title: "Fix A" }), makePR({ number: 2, title: "Fix B" })];
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
		// Activate filter and type
		mockInput.pressKey("/");
		await renderOnce();
		mockInput.pressKey("f");
		mockInput.pressKey("i");
		mockInput.pressKey("x");
		await renderOnce();

		// Navigate with j — should move within filtered results
		mockInput.pressKey("j");
		await renderOnce();
		expect(selections.length).toBeGreaterThan(0);
	});

	test("no match shows empty state message", async () => {
		const prs = [makePR({ number: 1, title: "Fix bug" })];

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
		mockInput.pressKey("/");
		await renderOnce();
		mockInput.pressKey("z");
		mockInput.pressKey("z");
		mockInput.pressKey("z");
		await renderOnce();

		const frame = captureCharFrame();
		expect(frame).toMatch(/no.*match|no.*result|0.*result/i);
	});
});

// ── Group panel ───────────────────────────────────────────────────────────────

describe("ListView — grouping panel", () => {
	test("g key opens grouping panel", async () => {
		const { renderOnce, captureCharFrame, mockInput } = await testRender(
			() => (
				<ListView
					prs={[makePR()]}
					onRefreshSelected={() => {}}
					onRefreshAll={() => {}}
					onNavigate={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		mockInput.pressKey("g");
		await renderOnce();

		const frame = captureCharFrame();
		// Panel should show grouping options
		expect(frame).toMatch(/group|status|author|label/i);
	});

	test("Escape closes grouping panel", async () => {
		const { renderOnce, captureCharFrame, mockInput } = await testRender(
			() => (
				<ListView
					prs={[makePR()]}
					onRefreshSelected={() => {}}
					onRefreshAll={() => {}}
					onNavigate={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		mockInput.pressKey("g");
		await renderOnce();

		// Escape requires small delay for ESC buffer flush (default 10ms timeout in stdin parser)
		mockInput.pressEscape();
		await new Promise((r) => setTimeout(r, 20));
		await renderOnce();

		// After escape, panel should be gone and the list should be visible
		const frame = captureCharFrame();
		expect(frame).not.toMatch(/Group by/i);
	});

	test("selecting author grouping shows group headers", async () => {
		const prs = [
			makePR({ number: 1, author: "alice", title: "PR alpha" }),
			makePR({ number: 2, author: "bob", title: "PR beta" }),
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
		mockInput.pressKey("g");
		await renderOnce();

		// Navigate to "author" option and select it
		// The panel lists options; navigate until we find "author" (it should be near the top)
		// First option is typically "smart-status", second might be "author"
		// Let's navigate down once to get to author
		mockInput.pressKey("j");
		await renderOnce();
		mockInput.pressEnter();
		await renderOnce();

		const frame = captureCharFrame();
		// Group header "alice" or "bob" should appear
		expect(frame).toMatch(/alice|bob/);
	});
});

// ── Grouped rendering ─────────────────────────────────────────────────────────

describe("ListView — grouped rendering", () => {
	test("smart-status groupBy shows tier group headers", async () => {
		const prs = [
			makePR({ number: 1, title: "Blocked", requestedReviewers: ["me"] }),
			makePR({ number: 2, title: "Waiting", isDraft: true }),
		];

		const { renderOnce, captureCharFrame } = await testRender(
			() => (
				<ListView
					prs={prs}
					currentUser="me"
					groupBy="smart-status"
					onRefreshSelected={() => {}}
					onRefreshAll={() => {}}
					onNavigate={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		const frame = captureCharFrame();
		// Should show tier headers
		expect(frame).toMatch(/[Mm]e blocking|[Ww]aiting/i);
	});

	test("author groupBy shows author name as group headers", async () => {
		const prs = [
			makePR({ number: 1, author: "alice", title: "Alice PR" }),
			makePR({ number: 2, author: "bob", title: "Bob PR" }),
		];

		const { renderOnce, captureCharFrame } = await testRender(
			() => (
				<ListView
					prs={prs}
					groupBy="author"
					onRefreshSelected={() => {}}
					onRefreshAll={() => {}}
					onNavigate={() => {}}
				/>
			),
			{ width: 120, height: 20 },
		);

		await renderOnce();
		const frame = captureCharFrame();
		expect(frame).toContain("Alice PR");
		expect(frame).toContain("Bob PR");
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
