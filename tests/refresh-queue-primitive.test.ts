import { describe, test, expect } from "bun:test";
import { createRoot, flush } from "solid-js";
import {
  createRefreshQueue,
  type QueueItem,
  type RefreshQueueDeps,
} from "../src/lib/refresh-queue";
import type { PRIdentity } from "../src/lib/pr-identity";
import type { StatusMessage } from "../src/lib/ui-state";

interface Pending {
  item: QueueItem;
  resolve: () => void;
  reject: (e: Error) => void;
}

interface Harness {
  state: ReturnType<typeof createRefreshQueue>[0];
  actions: ReturnType<typeof createRefreshQueue>[1];
  pending: Pending[];
  ran: QueueItem[];
  messages: (StatusMessage | null)[];
  dispose: () => void;
}

async function tick(times = 4): Promise<void> {
  for (let i = 0; i < times; i++) await Promise.resolve();
  flush();
}

function harness(options: { maxActive?: number; defaultRepoSlug?: string } = {}): Harness {
  const pending: Pending[] = [];
  const ran: QueueItem[] = [];
  const messages: (StatusMessage | null)[] = [];

  const deps: RefreshQueueDeps = {
    defaultRepoSlug: options.defaultRepoSlug ?? "owner/fallback",
    runRefresh: (item) => {
      ran.push(item);
      return new Promise<void>((resolve, reject) => {
        pending.push({ item, resolve, reject });
      });
    },
    setStatusMessage: (m) => messages.push(m),
    maxActive: options.maxActive,
  };

  let stateActions!: ReturnType<typeof createRefreshQueue>;
  const dispose = createRoot((d) => {
    stateActions = createRefreshQueue(deps);
    return d;
  });

  return {
    state: stateActions[0],
    actions: stateActions[1],
    pending,
    ran,
    messages,
    dispose,
  };
}

const prKey = (n: number): PRIdentity => ({ number: n, repoSlug: "a/b" });

describe("createRefreshQueue", () => {
  test("queuePrRefresh kicks the pump and runRefresh is invoked", async () => {
    const h = harness();
    h.actions.queuePrRefresh(prKey(1), { priority: 2, includeFiles: false });
    await tick();
    expect(h.ran.length).toBe(1);
    expect(h.ran[0]!.pr).toEqual(prKey(1));
    h.dispose();
  });

  test("refreshStateForPr reports queued, then refreshing, then undefined", async () => {
    const h = harness({ maxActive: 1 });

    h.actions.queuePrRefresh(prKey(1), { priority: 2, includeFiles: false });
    h.actions.queuePrRefresh(prKey(2), { priority: 2, includeFiles: false });
    expect(h.state.refreshStateForPr(prKey(1))).toBe("queued");
    expect(h.state.refreshStateForPr(prKey(2))).toBe("queued");

    await tick();
    expect(h.state.refreshStateForPr(prKey(1))).toBe("refreshing");
    expect(h.state.refreshStateForPr(prKey(2))).toBe("queued");

    h.pending[0]!.resolve();
    await tick();
    expect(h.state.refreshStateForPr(prKey(1))).toBeUndefined();
    expect(h.state.refreshStateForPr(prKey(2))).toBe("refreshing");

    h.pending[1]!.resolve();
    await tick();
    expect(h.state.refreshStateForPr(prKey(2))).toBeUndefined();
    h.dispose();
  });

  test("higher priority items run before lower priority", async () => {
    const h = harness({ maxActive: 1 });
    // Enqueue low first, then high. Pump should pick high first.
    h.actions.queuePrRefresh(prKey(10), { priority: 4, includeFiles: false });
    h.actions.queuePrRefresh(prKey(20), { priority: 1, includeFiles: false });
    h.actions.queuePrRefresh(prKey(30), { priority: 3, includeFiles: false });
    await tick();
    expect(h.ran[0]!.pr.number).toBe(20);

    h.pending[0]!.resolve();
    await tick();
    expect(h.ran[1]!.pr.number).toBe(30);

    h.pending[1]!.resolve();
    await tick();
    expect(h.ran[2]!.pr.number).toBe(10);
    h.dispose();
  });

  test("FIFO ordering within the same priority tier", async () => {
    const h = harness({ maxActive: 1 });
    h.actions.queuePrRefresh(prKey(1), { priority: 2, includeFiles: false });
    h.actions.queuePrRefresh(prKey(2), { priority: 2, includeFiles: false });
    h.actions.queuePrRefresh(prKey(3), { priority: 2, includeFiles: false });
    await tick();

    h.pending[0]!.resolve();
    await tick();
    h.pending[1]!.resolve();
    await tick();
    h.pending[2]!.resolve();
    await tick();

    expect(h.ran.map((r) => r.pr.number)).toEqual([1, 2, 3]);
    h.dispose();
  });

  test("respects the concurrency cap", async () => {
    const h = harness({ maxActive: 3 });
    for (let i = 1; i <= 8; i++) {
      h.actions.queuePrRefresh(prKey(i), { priority: 2, includeFiles: false });
    }
    await tick();
    expect(h.ran.length).toBe(3);

    h.pending[0]!.resolve();
    await tick();
    expect(h.ran.length).toBe(4);

    h.pending[1]!.resolve();
    h.pending[2]!.resolve();
    await tick();
    expect(h.ran.length).toBe(6);
    h.dispose();
  });

  test("re-queuing while queued upgrades priority and unions includeFiles", async () => {
    const h = harness({ maxActive: 1 });
    h.actions.queuePrRefresh(prKey(99), { priority: 4, includeFiles: false });
    h.actions.queuePrRefresh(prKey(98), { priority: 4, includeFiles: false });
    // Re-queue 99 with higher priority and includeFiles=true. It should
    // jump ahead of 98 and the run should carry includeFiles.
    h.actions.queuePrRefresh(prKey(99), { priority: 1, includeFiles: true });
    await tick();
    expect(h.ran[0]!.pr.number).toBe(99);
    expect(h.ran[0]!.includeFiles).toBe(true);
    h.dispose();
  });

  test("re-queuing while refreshing is a no-op", async () => {
    const h = harness({ maxActive: 1 });
    h.actions.queuePrRefresh(prKey(1), { priority: 4, includeFiles: false });
    await tick();
    expect(h.state.refreshStateForPr(prKey(1))).toBe("refreshing");

    // Should not enqueue a duplicate or upgrade priority on the in-flight item.
    h.actions.queuePrRefresh(prKey(1), { priority: 1, includeFiles: true });
    await tick();
    expect(h.ran.length).toBe(1);

    h.pending[0]!.resolve();
    await tick();
    // After the in-flight settles, the item is fully cleared — no re-run.
    expect(h.ran.length).toBe(1);
    expect(h.state.refreshStateForPr(prKey(1))).toBeUndefined();
    h.dispose();
  });

  test("runRefresh failure surfaces a status message and the entry clears", async () => {
    const h = harness();
    h.actions.queuePrRefresh(prKey(7), { priority: 2, includeFiles: false });
    await tick();
    h.pending[0]!.reject(new Error("network down"));
    await tick();
    expect(h.messages).toEqual([{ text: "refresh failed for #7: network down", kind: "error" }]);
    expect(h.state.refreshStateForPr(prKey(7))).toBeUndefined();
    h.dispose();
  });
});
