import { describe, test, expect, afterEach } from "bun:test";
import { createLogWriter } from "./log-writer";
import { readFile, unlink } from "fs/promises";
import { join } from "path";
import { tmpdir } from "os";

function tmpLog(): string {
  return join(tmpdir(), `legit-log-test-${Date.now()}-${Math.random().toString(36).slice(2)}.log`);
}

async function cleanup(path: string) {
  try {
    await unlink(path);
  } catch {}
}

describe("LogWriter", () => {
  const paths: string[] = [];
  afterEach(async () => {
    for (const p of paths) await cleanup(p);
    paths.length = 0;
  });

  test("writes header and buffered lines on dispose", async () => {
    const path = tmpLog();
    paths.push(path);
    const writer = createLogWriter({
      path,
      header: "# header\n",
      flushIntervalMs: 60_000, // won't fire during test
    });
    writer.write("line 1\n");
    writer.write("line 2\n");
    await writer.dispose();

    const content = await readFile(path, "utf8");
    expect(content).toBe("# header\nline 1\nline 2\n");
  });

  test("flushes automatically when buffer exceeds threshold", async () => {
    const path = tmpLog();
    paths.push(path);
    const writer = createLogWriter({
      path,
      flushThresholdBytes: 20,
      flushIntervalMs: 60_000,
    });
    // Write enough to exceed the 20-byte threshold
    writer.write("a]".repeat(15) + "\n"); // 31 bytes
    // Give the async flush a moment to complete
    await new Promise((r) => setTimeout(r, 50));

    const content = await readFile(path, "utf8");
    expect(content.length).toBeGreaterThan(0);
    await writer.dispose();
  });

  test("flushes on timer interval", async () => {
    const path = tmpLog();
    paths.push(path);
    const writer = createLogWriter({
      path,
      flushIntervalMs: 50,
      flushThresholdBytes: 1_000_000, // won't trigger
    });
    writer.write("timed\n");
    // Wait for the interval to fire
    await new Promise((r) => setTimeout(r, 150));

    const content = await readFile(path, "utf8");
    expect(content).toBe("timed\n");
    await writer.dispose();
  });

  test("does not block the caller", async () => {
    const path = tmpLog();
    paths.push(path);
    const writer = createLogWriter({ path, flushIntervalMs: 60_000 });

    const start = performance.now();
    for (let i = 0; i < 1000; i++) {
      writer.write(`line ${i}\n`);
    }
    const elapsed = performance.now() - start;

    // 1000 writes should complete in well under 10ms (no I/O)
    expect(elapsed).toBeLessThan(10);
    await writer.dispose();

    const content = await readFile(path, "utf8");
    const lines = content.trim().split("\n");
    expect(lines.length).toBe(1000);
  });

  test("write after dispose is silently ignored", async () => {
    const path = tmpLog();
    paths.push(path);
    const writer = createLogWriter({ path, flushIntervalMs: 60_000 });
    writer.write("before\n");
    await writer.dispose();
    writer.write("after\n");

    const content = await readFile(path, "utf8");
    expect(content).toBe("before\n");
  });
});
