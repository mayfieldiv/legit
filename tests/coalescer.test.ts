import { describe, test, expect } from "bun:test";
import { makeCoalescer } from "../src/lib/coalescer";

describe("makeCoalescer", () => {
	test("applies once per macrotask for rapid schedules", async () => {
		const applied: number[] = [];
		const { schedule } = makeCoalescer<number>((v) => applied.push(v));

		schedule(1);
		schedule(2);
		schedule(3);
		expect(applied).toEqual([]);

		await new Promise<void>((r) => setTimeout(r, 0));
		expect(applied).toEqual([3]);
	});

	test("flush applies immediately and clears pending macrotask", async () => {
		const applied: string[] = [];
		const { schedule, flush } = makeCoalescer<string>((v) => applied.push(v));

		schedule("a");
		flush();
		expect(applied).toEqual(["a"]);

		await new Promise<void>((r) => setTimeout(r, 0));
		expect(applied).toEqual(["a"]);
	});

	test("second flush does not re-apply the same value", () => {
		const applied: number[] = [];
		const { schedule, flush } = makeCoalescer<number>((v) => applied.push(v));

		schedule(1);
		flush();
		expect(applied).toEqual([1]);
		flush();
		expect(applied).toEqual([1]);
	});

	test("skips apply when signal is aborted", () => {
		const applied: number[] = [];
		const ac = new AbortController();
		ac.abort();
		const { schedule, flush } = makeCoalescer<number>((v) => applied.push(v), ac.signal);

		schedule(1);
		flush();
		expect(applied).toEqual([]);
	});
});
