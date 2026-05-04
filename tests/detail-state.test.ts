import { describe, test, expect } from "bun:test";
import { createRoot, createSignal, flush } from "solid-js";
import {
  createDetailState,
  type DetailFetchResult,
  type DetailStateDeps,
} from "../src/lib/detail-state";
import type { PRIdentity } from "../src/lib/pr-identity";
import type { StatusMessage } from "../src/lib/ui-state";
import type { PRDetail, IssueComment } from "../src/lib/types";

interface Harness {
  detailPr: ReturnType<typeof createSignal<PRIdentity | undefined>>;
  state: ReturnType<typeof createDetailState>[0];
  actions: ReturnType<typeof createDetailState>[1];
  fetchCalls: PRIdentity[];
  resolveFetch: (result: DetailFetchResult) => void;
  rejectFetch: (err: Error) => void;
  fetched: DetailFetchResult[];
  messages: (StatusMessage | null)[];
  dispose: () => void;
}

function makeFetchResult(pr: PRIdentity): DetailFetchResult {
  return {
    pr: { number: pr.number, repoSlug: pr.repoSlug } as unknown as PRDetail,
    threads: [],
    comments: [{ id: pr.number, body: `c${pr.number}` } as unknown as IssueComment],
  };
}

/** Pump microtasks so async chains inside the primitive can settle. */
async function tick(times = 3): Promise<void> {
  for (let i = 0; i < times; i++) await Promise.resolve();
  flush();
}

function harness(options: { initialPr?: PRIdentity; rejectFirst?: boolean } = {}): Harness {
  let resolveFetch: ((r: DetailFetchResult) => void) | undefined;
  let rejectFetch: ((e: Error) => void) | undefined;
  const fetchCalls: PRIdentity[] = [];
  const fetched: DetailFetchResult[] = [];
  const messages: (StatusMessage | null)[] = [];

  const [detailPr, setDetailPr] = createSignal<PRIdentity | undefined>(options.initialPr);

  const deps: DetailStateDeps = {
    detailPr,
    fetch: (pr, _signal) => {
      fetchCalls.push(pr);
      return new Promise<DetailFetchResult>((resolve, reject) => {
        resolveFetch = resolve;
        rejectFetch = reject;
      });
    },
    onFetched: (_pr, result) => {
      fetched.push(result);
    },
    setStatusMessage: (m) => messages.push(m),
  };

  let stateActions!: ReturnType<typeof createDetailState>;
  const dispose = createRoot((d) => {
    stateActions = createDetailState(deps);
    return d;
  });

  return {
    detailPr: [detailPr, setDetailPr] as ReturnType<typeof createSignal<PRIdentity | undefined>>,
    state: stateActions[0],
    actions: stateActions[1],
    fetchCalls,
    resolveFetch: (r) => resolveFetch!(r),
    rejectFetch: (e) => rejectFetch!(e),
    fetched,
    messages,
    dispose,
  };
}

describe("createDetailState", () => {
  test("starts idle when detailPr is undefined", async () => {
    const h = harness();
    await tick();
    expect(h.state().kind).toBe("idle");
    expect(h.fetchCalls).toEqual([]);
    h.dispose();
  });

  test("transitions idle → loading → ready when detailPr is set and fetch resolves", async () => {
    const h = harness();
    const pr: PRIdentity = { number: 1, repoSlug: "a/b" };
    const [, setDetailPr] = h.detailPr;
    setDetailPr(pr);
    await tick();
    expect(h.state()).toEqual({ kind: "loading", pr });
    expect(h.fetchCalls).toEqual([pr]);

    const result = makeFetchResult(pr);
    h.resolveFetch(result);
    await tick();
    expect(h.state()).toEqual({ kind: "ready", pr, comments: result.comments });
    expect(h.fetched).toEqual([result]);
    h.dispose();
  });

  test("transitions to error and reports via setStatusMessage when fetch rejects", async () => {
    const h = harness();
    const pr: PRIdentity = { number: 2, repoSlug: "a/b" };
    const [, setDetailPr] = h.detailPr;
    setDetailPr(pr);
    await tick();
    h.rejectFetch(new Error("boom"));
    await tick();
    const s = h.state();
    expect(s.kind).toBe("error");
    if (s.kind === "error") {
      expect(s.pr).toEqual(pr);
      expect(s.error.message).toBe("boom");
    }
    expect(h.messages).toEqual([{ text: "detail fetch failed: boom", kind: "error" }]);
    h.dispose();
  });

  test("changing detailPr mid-flight aborts the in-flight fetch", async () => {
    const aborted: boolean[] = [];
    const fetchCalls: PRIdentity[] = [];
    const settlers: Array<{
      resolve: (r: DetailFetchResult) => void;
      reject: (e: Error) => void;
      signal: AbortSignal;
    }> = [];

    const [detailPr, setDetailPr] = createSignal<PRIdentity | undefined>(undefined);
    const deps: DetailStateDeps = {
      detailPr,
      fetch: (pr, signal) => {
        fetchCalls.push(pr);
        signal.addEventListener("abort", () => aborted.push(true));
        return new Promise<DetailFetchResult>((resolve, reject) => {
          settlers.push({ resolve, reject, signal });
        });
      },
      setStatusMessage: () => {},
    };

    let stateActions!: ReturnType<typeof createDetailState>;
    const dispose = createRoot((d) => {
      stateActions = createDetailState(deps);
      return d;
    });
    const [state] = stateActions;

    const prA: PRIdentity = { number: 10, repoSlug: "a/b" };
    const prB: PRIdentity = { number: 20, repoSlug: "a/b" };
    setDetailPr(prA);
    await tick();
    expect(state()).toEqual({ kind: "loading", pr: prA });

    setDetailPr(prB);
    await tick();
    expect(aborted.length).toBe(1);
    expect(state()).toEqual({ kind: "loading", pr: prB });
    expect(fetchCalls).toEqual([prA, prB]);

    dispose();
  });

  test("returning to list view (detailPr → undefined) resets to idle", async () => {
    const h = harness();
    const pr: PRIdentity = { number: 1, repoSlug: "a/b" };
    const [, setDetailPr] = h.detailPr;
    setDetailPr(pr);
    await tick();
    h.resolveFetch(makeFetchResult(pr));
    await tick();
    expect(h.state().kind).toBe("ready");

    setDetailPr(undefined);
    await tick();
    expect(h.state()).toEqual({ kind: "idle" });
    h.dispose();
  });

  test("refresh re-runs the fetch for the current detail pr", async () => {
    const h = harness();
    const pr: PRIdentity = { number: 5, repoSlug: "a/b" };
    const [, setDetailPr] = h.detailPr;
    setDetailPr(pr);
    await tick();
    h.resolveFetch(makeFetchResult(pr));
    await tick();
    expect(h.state().kind).toBe("ready");

    h.actions.refresh();
    await tick();
    expect(h.state()).toEqual({ kind: "loading", pr });
    expect(h.fetchCalls.length).toBe(2);
    h.dispose();
  });
});
