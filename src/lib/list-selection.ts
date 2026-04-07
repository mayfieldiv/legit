import { createSignal, type Accessor } from "solid-js";

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
  const [rawIndex, setRawIndex] = createSignal(0);

  // Always-clamped index accessor
  const index: Accessor<number> = () => {
    const len = listLength();
    if (len === 0) return 0;
    const i = rawIndex();
    return Math.min(Math.max(i, 0), len - 1);
  };

  return {
    index,

    select(i: number) {
      const len = listLength();
      if (len === 0) {
        setRawIndex(0);
      } else {
        setRawIndex(Math.min(Math.max(i, 0), len - 1));
      }
    },

    moveDown() {
      const len = listLength();
      if (len > 0) {
        setRawIndex((i) => Math.min(i + 1, len - 1));
      }
    },

    moveUp() {
      setRawIndex((i) => {
        const len = listLength();
        // Clamp to current list bounds before decrementing,
        // so a stale raw index doesn't require many presses after list shrink
        const clamped = len === 0 ? 0 : Math.min(Math.max(i, 0), len - 1);
        return Math.max(clamped - 1, 0);
      });
    },

    selectedItem<T>(list: T[]): T | undefined {
      return list[index()];
    },
  };
}
