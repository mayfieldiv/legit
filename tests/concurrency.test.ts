import { describe, test, expect } from "bun:test";
import { withConcurrencyLimit } from "../src/lib/concurrency";

describe("withConcurrencyLimit", () => {
	test("tracks in-flight and waiting against the limit", async () => {
		let release!: () => void;
		const gate = new Promise<void>((r) => {
			release = r;
		});

		const baseFetch = async () => {
			await gate;
			return new Response("ok");
		};

		const { fetch, getSnapshot, subscribe } = withConcurrencyLimit(2, baseFetch);

		const snapshots: { inFlight: number; waiting: number }[] = [];
		subscribe(() => snapshots.push(getSnapshot()));

		const p1 = fetch("https://a", {});
		expect(getSnapshot()).toEqual({ inFlight: 1, waiting: 0 });

		const p2 = fetch("https://b", {});
		expect(getSnapshot()).toEqual({ inFlight: 2, waiting: 0 });

		const p3 = fetch("https://c", {});
		expect(getSnapshot().waiting).toBe(1);

		release();
		await Promise.all([p1, p2, p3]);
		expect(getSnapshot()).toEqual({ inFlight: 0, waiting: 0 });
		expect(snapshots.length).toBeGreaterThan(0);
	});
});
