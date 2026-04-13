import { createSignal, latest, type Accessor } from "solid-js";

export interface ListSelection {
  index: Accessor<number>;
  select: (i: number) => void;
  moveUp: () => void;
  moveDown: () => void;
  selectedItem: <T>(list: T[]) => T | undefined;
}

/**
 * Reactive list selection primitive.
 *
 * Manages a selection index that stays clamped within [0, listLength - 1].
 * The index accessor always returns a clamped value — if the list shrinks,
 * the index is automatically adjusted on read.
 */
export function createListSelection(listLength: Accessor<number>): ListSelection {
  const [version, setVersion] = createSignal(0);
  let rawIndex = 0;

  const clampIndex = (i: number): number => {
    const len = latest(listLength);
    if (len === 0) return 0;
    return Math.min(Math.max(i, 0), len - 1);
  };

  const syncIndex = (next: number) => {
    rawIndex = clampIndex(next);
    setVersion((v) => v + 1);
  };

  // Always-clamped index accessor
  const index: Accessor<number> = () => {
    version();
    rawIndex = clampIndex(rawIndex);
    return rawIndex;
  };

  return {
    index,

    select(i: number) {
      syncIndex(i);
    },

    moveDown() {
      if (listLength() > 0) {
        syncIndex(rawIndex + 1);
      }
    },

    moveUp() {
      syncIndex(rawIndex - 1);
    },

    selectedItem<T>(list: T[]): T | undefined {
      return list[index()];
    },
  };
}
