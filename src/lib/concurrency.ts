/**
 * HTTP fetch wrapper with concurrency control.
 * Limits the number of simultaneous in-flight requests.
 */

import type { HttpFetch } from "./github-transport";

export interface GitHubNetworkStats {
	/** Requests currently executing (HTTP in flight). */
	inFlight: number;
	/**
	 * Requests blocked on the HTTP concurrency semaphore (have entered fetch, no slot yet).
	 * The UI may replace this with a broader “app waiting” count (TanStack fetching − inFlight).
	 */
	waiting: number;
}

export type ConcurrencyLimitedFetch = {
	fetch: HttpFetch;
	getSnapshot: () => GitHubNetworkStats;
	subscribe: (listener: () => void) => () => void;
};

export function withConcurrencyLimit(
	maxConcurrent: number,
	fetch: HttpFetch,
): ConcurrencyLimitedFetch {
	let active = 0;
	const queue: Array<() => void> = [];
	const subs = new Set<() => void>();

	function notify() {
		for (const fn of subs) fn();
	}

	function getSnapshot(): GitHubNetworkStats {
		return { inFlight: active, waiting: queue.length };
	}

	const wrapped: HttpFetch = async (url, init) => {
		if (active >= maxConcurrent) {
			await new Promise<void>((resolve) => {
				queue.push(resolve);
				notify();
			});
		}
		active++;
		notify();
		try {
			return await fetch(url, init);
		} finally {
			active--;
			queue.shift()?.();
			notify();
		}
	};

	return {
		fetch: wrapped,
		getSnapshot,
		subscribe(listener) {
			subs.add(listener);
			return () => subs.delete(listener);
		},
	};
}
