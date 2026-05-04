import { createEffect, onCleanup, type Accessor } from "solid-js";

export function createAbortableAsyncEffect<T>(
  source: Accessor<T>,
  run: (value: T, signal: AbortSignal, isCurrent: () => boolean) => void | Promise<void>,
  onError: (error: unknown, value: T) => void,
): void {
  let controller: AbortController | undefined;

  // Component-lifetime cleanup: aborts any in-flight run when the owning scope
  // disposes. Aborts on source-change (effect re-run) are handled separately
  // by the controller?.abort() at the top of the effect body — together they
  // cover both the reactive re-run path and the unmount path.
  onCleanup(() => {
    controller?.abort();
    controller = undefined;
  });

  createEffect(source, (value) => {
    controller?.abort();
    const activeController = new AbortController();
    controller = activeController;

    const isCurrent = () => controller === activeController && !activeController.signal.aborted;

    void Promise.resolve()
      .then(() => run(value, activeController.signal, isCurrent))
      .catch((error) => {
        if (!isCurrent()) return;
        onError(error, value);
      });
  });
}
