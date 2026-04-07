/**
 * HTTP fetch wrapper with concurrency control.
 * Limits the number of simultaneous in-flight requests.
 */

import { appendFileSync, writeFileSync } from "fs";
import { join } from "path";
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
      await new Promise<void>((resolve, reject) => {
        queue.push(resolve);
        notify();
        const signal = init?.signal;
        if (signal) {
          if (signal.aborted) {
            const idx = queue.indexOf(resolve);
            if (idx >= 0) queue.splice(idx, 1);
            reject(signal.reason ?? new DOMException("Aborted", "AbortError"));
            notify();
            return;
          }
          signal.addEventListener(
            "abort",
            () => {
              const idx = queue.indexOf(resolve);
              if (idx >= 0) {
                queue.splice(idx, 1);
                reject(signal.reason ?? new DOMException("Aborted", "AbortError"));
                notify();
              }
            },
            { once: true },
          );
        }
      });
    }
    active++;
    notify();
    const start = performance.now();
    let status = 0;
    try {
      const res = await fetch(url, init);
      status = res.status;
      return res;
    } finally {
      const duration = performance.now() - start;
      logRequest(url, init?.method ?? "GET", status, duration);
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

// ── Request logger ──────────────────────────────────────────────────────────

const LOG_PATH = join(process.env.HOME ?? "/tmp", ".config", "legit", "requests.log");
let logInitialized = false;
const startTime = Date.now();

function logRequest(url: string, method: string, status: number, durationMs: number) {
  if (!logInitialized) {
    try {
      writeFileSync(
        LOG_PATH,
        `# legit request log — ${new Date().toISOString()}\n# method  status  duration_ms  relative_ms  url\n`,
      );
    } catch {
      // Non-fatal — logging is best-effort
      return;
    }
    logInitialized = true;
  }
  const relative = Date.now() - startTime;
  const line = `${method.padEnd(6)} ${String(status).padStart(3)}  ${durationMs.toFixed(0).padStart(7)}ms  @${String(relative).padStart(8)}ms  ${url}\n`;
  try {
    appendFileSync(LOG_PATH, line);
  } catch {
    // Non-fatal
  }
}
