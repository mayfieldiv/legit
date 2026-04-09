/**
 * Async, non-blocking file log writer.
 *
 * Callers push lines via `write()` which returns immediately. Lines are
 * buffered in memory and flushed to disk asynchronously on a timer or
 * when the buffer exceeds a size threshold. All I/O happens off the
 * critical path so the event loop is never blocked by log writes.
 */

import { appendFile, writeFile } from "fs/promises";

export interface LogWriter {
  /** Enqueue a line (or multi-line string) to be written. Never blocks. */
  write(line: string): void;
  /** Flush any buffered content to disk and stop the flush timer. */
  dispose(): Promise<void>;
}

export interface LogWriterOptions {
  /** Absolute path to the log file. */
  path: string;
  /** Header written once when the file is first created this session. */
  header?: string;
  /** Max ms between automatic flushes. Default: 500. */
  flushIntervalMs?: number;
  /** Flush immediately when the buffer exceeds this many bytes. Default: 8192. */
  flushThresholdBytes?: number;
}

export function createLogWriter(options: LogWriterOptions): LogWriter {
  const { path, header, flushIntervalMs = 500, flushThresholdBytes = 8192 } = options;

  let buffer = "";
  let headerWritten = false;
  let flushInFlight: Promise<void> | undefined;
  let disposed = false;
  let timer: ReturnType<typeof setInterval> | undefined;

  // Start periodic flush. unref() so the timer doesn't keep the process alive.
  timer = setInterval(() => void flush(), flushIntervalMs);
  timer.unref();

  async function flush(): Promise<void> {
    if (buffer.length === 0) return;

    // Grab everything currently buffered and reset.
    let chunk = buffer;
    buffer = "";

    try {
      if (!headerWritten) {
        if (header) chunk = header + chunk;
        headerWritten = true;
        // First write creates/truncates the file for a clean session.
        await writeFile(path, chunk);
      } else {
        await appendFile(path, chunk);
      }
    } catch {
      // Logging is best-effort — drop on error.
    }
  }

  function scheduleFlush(): void {
    if (flushInFlight) return; // already running, flush() will be re-called by drain()
    flushInFlight = drain();
  }

  /** Run flush in a loop until the buffer is fully drained. */
  async function drain(): Promise<void> {
    try {
      while (buffer.length > 0) {
        await flush();
      }
    } finally {
      flushInFlight = undefined;
    }
  }

  function write(line: string): void {
    if (disposed) return;
    buffer += line;
    if (buffer.length >= flushThresholdBytes) {
      scheduleFlush();
    }
  }

  async function dispose(): Promise<void> {
    disposed = true;
    if (timer !== undefined) {
      clearInterval(timer);
      timer = undefined;
    }
    // Wait for any in-flight flush to finish, then flush remaining buffer.
    if (flushInFlight) {
      await flushInFlight;
    }
    await flush();
  }

  return { write, dispose };
}
