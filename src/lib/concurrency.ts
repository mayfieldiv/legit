/**
 * HTTP fetch wrapper with concurrency control.
 * Limits the number of simultaneous in-flight requests.
 */

import type { HttpFetch } from "./github-transport";

export function withConcurrencyLimit(maxConcurrent: number, fetch: HttpFetch): HttpFetch {
	let active = 0;
	const queue: Array<() => void> = [];

	return async (url, init) => {
		if (active >= maxConcurrent) {
			await new Promise<void>((resolve) => queue.push(resolve));
		}
		active++;
		try {
			return await fetch(url, init);
		} finally {
			active--;
			queue.shift()?.();
		}
	};
}
