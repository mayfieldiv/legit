import { describe, test, expect } from "bun:test";
import { createRoot, createSignal } from "solid-js";
import { createListSelection } from "../src/lib/list-selection";

describe("createListSelection", () => {
	test("starts at index 0", () => {
		createRoot(() => {
			const sel = createListSelection(() => 5);
			expect(sel.index()).toBe(0);
		});
	});

	test("moveDown increments index", () => {
		createRoot(() => {
			const sel = createListSelection(() => 5);
			sel.moveDown();
			expect(sel.index()).toBe(1);
		});
	});

	test("moveDown does not exceed list length - 1", () => {
		createRoot(() => {
			const sel = createListSelection(() => 3);
			sel.moveDown();
			sel.moveDown();
			sel.moveDown();
			sel.moveDown();
			expect(sel.index()).toBe(2);
		});
	});

	test("moveUp decrements index", () => {
		createRoot(() => {
			const sel = createListSelection(() => 5);
			sel.moveDown();
			sel.moveDown();
			sel.moveUp();
			expect(sel.index()).toBe(1);
		});
	});

	test("moveUp does not go below 0", () => {
		createRoot(() => {
			const sel = createListSelection(() => 5);
			sel.moveUp();
			sel.moveUp();
			expect(sel.index()).toBe(0);
		});
	});

	test("moveDown does nothing on empty list", () => {
		createRoot(() => {
			const sel = createListSelection(() => 0);
			sel.moveDown();
			expect(sel.index()).toBe(0);
		});
	});

	test("select sets index directly", () => {
		createRoot(() => {
			const sel = createListSelection(() => 5);
			sel.select(3);
			expect(sel.index()).toBe(3);
		});
	});

	test("select clamps to valid range", () => {
		createRoot(() => {
			const sel = createListSelection(() => 3);
			sel.select(10);
			expect(sel.index()).toBe(2);
			sel.select(-1);
			expect(sel.index()).toBe(0);
		});
	});

	test("index clamps when list shrinks", () => {
		createRoot(() => {
			const [len, setLen] = createSignal(5);
			const sel = createListSelection(len);
			sel.select(4); // last item
			expect(sel.index()).toBe(4);

			setLen(2); // shrink list
			// Need to trigger the effect — access the index
			expect(sel.index()).toBe(1);
		});
	});

	test("selectedItem returns item at current index", () => {
		createRoot(() => {
			const sel = createListSelection(() => 3);
			const items = ["a", "b", "c"];
			sel.moveDown();
			expect(sel.selectedItem(items)).toBe("b");
		});
	});

	test("selectedItem returns undefined for empty list", () => {
		createRoot(() => {
			const sel = createListSelection(() => 0);
			expect(sel.selectedItem([])).toBeUndefined();
		});
	});
});
