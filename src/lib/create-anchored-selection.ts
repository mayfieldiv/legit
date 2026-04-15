import { createEffect, createMemo, createSignal, type Accessor } from "solid-js";
import type { ListSelection } from "./list-selection";
import { findPrIndex, prKey, samePr, samePrKey, type PRIdentity } from "./pr-identity";

type EnsureVisibleDirection = "up" | "down";

export interface CreateAnchoredSelectionOptions<T extends PRIdentity> {
  items: Accessor<T[]>;
  selection: ListSelection;
  parentSelectedItem?: Accessor<T | undefined>;
  onSelectionChange?: (item: T) => void;
  ensureVisible?: (direction: EnsureVisibleDirection) => void;
}

export interface AnchoredSelectionHandle {
  clearAnchor: () => void;
}

export function createAnchoredSelection<T extends PRIdentity>(
  options: CreateAnchoredSelectionOptions<T>,
): AnchoredSelectionHandle {
  let anchor: PRIdentity | null = null;
  const [displayVersion, setDisplayVersion] = createSignal(0);
  let lastNotifiedItem: PRIdentity | null = null;
  let lastNotifiedDisplayVersion = -1;

  const selectedItem = createMemo(() => options.selection.selectedItem(options.items()));

  let didTrackItems = false;
  createEffect(
    () => options.items(),
    () => {
      if (!didTrackItems) {
        didTrackItems = true;
        return;
      }
      setDisplayVersion((v) => v + 1);
    },
  );

  createEffect(
    () => ({ items: options.items(), selectedItem: options.parentSelectedItem?.() }),
    ({ items, selectedItem: parentSelectedItem }) => {
      if (!parentSelectedItem) return;

      const current = options.selection.selectedItem(items);
      if (samePr(current, parentSelectedItem)) return;

      const idx = findPrIndex(items, parentSelectedItem);
      if (idx < 0) return;

      const prevIdx = options.selection.index();
      options.selection.select(idx);
      anchor = prKey(parentSelectedItem);
      options.ensureVisible?.(idx >= prevIdx ? "down" : "up");
    },
  );

  createEffect(
    () => ({ items: options.items(), item: selectedItem(), version: displayVersion() }),
    ({ items, item, version }) => {
      if (!item) return;

      const anchoredIdx = anchor === null ? -1 : findPrIndex(items, anchor);
      if (anchoredIdx >= 0 && !samePrKey(item, anchor) && version !== lastNotifiedDisplayVersion) {
        return;
      }

      if (samePrKey(item, lastNotifiedItem)) {
        lastNotifiedDisplayVersion = version;
        return;
      }

      anchor = prKey(item);
      lastNotifiedItem = anchor;
      lastNotifiedDisplayVersion = version;
      options.onSelectionChange?.(item);
    },
  );

  let didProcessItems = false;
  createEffect(
    () => options.items(),
    (items) => {
      if (!didProcessItems) {
        didProcessItems = true;
        return;
      }
      if (anchor === null) return;

      const current = options.selection.selectedItem(items);
      if (current && samePrKey(current, anchor)) return;

      const idx = findPrIndex(items, anchor);
      if (idx < 0 || idx === options.selection.index()) return;

      const prevIdx = options.selection.index();
      options.selection.select(idx);
      options.ensureVisible?.(idx >= prevIdx ? "down" : "up");
    },
  );

  return {
    clearAnchor() {
      anchor = null;
    },
  };
}
