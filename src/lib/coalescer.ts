/**
 * Coalesces rapid successive values into one callback invocation per macrotask
 * boundary. Values arriving in the same microtask burst are collapsed to the
 * latest; the callback fires on the next macrotask (setTimeout 0).
 *
 * Call flush() in a finally block to apply the last value synchronously and
 * cancel any pending macrotask — finally runs before macrotasks, so this is safe.
 *
 * `undefined` is reserved as the internal "no value scheduled" sentinel — do not
 * schedule `undefined` as a meaningful payload.
 *
 * If `schedule()` runs after `signal.aborted` is true, the scheduled macrotask may
 * still fire; `flush()` will skip `apply` when aborted. Prefer not scheduling
 * after abort.
 */
export function makeCoalescer<T>(apply: (v: T) => void, signal?: AbortSignal) {
	let latest: T | undefined;
	let pending: ReturnType<typeof setTimeout> | undefined;

	const flush = () => {
		clearTimeout(pending);
		pending = undefined;
		if (latest === undefined) return;
		const v = latest;
		latest = undefined;
		if (!signal?.aborted) apply(v);
	};

	const schedule = (v: T) => {
		latest = v;
		pending ??= setTimeout(flush, 0);
	};

	return { schedule, flush };
}
