import { createEffect, type Accessor } from "solid-js";

export function createAbortableAsyncEffect<T>(
  source: Accessor<T>,
  run: (value: T, signal: AbortSignal, isCurrent: () => boolean) => void | Promise<void>,
  onError: (error: unknown, value: T) => void,
): void {
  // The returned cleanup aborts the in-flight controller on both source change
  // (effect re-run) and component unmount; nothing escapes the per-run scope.
  createEffect(source, (value) => {
    const controller = new AbortController();
    const isCurrent = () => !controller.signal.aborted;

    void Promise.resolve()
      .then(() => run(value, controller.signal, isCurrent))
      .catch((error) => {
        if (!isCurrent()) return;
        onError(error, value);
      });

    return () => controller.abort();
  });
}
