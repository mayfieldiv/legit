/**
 * Reactive store for <details> expand/collapse state.
 *
 * Each FocusableCard creates one controller. All <details> elements
 * rendered inside that card register with it, getting their own
 * expand/collapse signal. The controller also exposes toggleAll()
 * for the Enter-key interaction.
 */

import { createSignal, createContext, useContext } from "solid-js";
import type { JSX as OpenTuiJSX } from "@opentui/solid";
import type { Accessor } from "solid-js";

type ProviderComponent<T> = (props: {
  value: T;
  children?: OpenTuiJSX.Element;
}) => OpenTuiJSX.Element;

type LegacyProviderContext<T> = ReturnType<typeof createContext<T>> & {
  Provider: ProviderComponent<T>;
};

export interface DetailsHandle {
  expanded: Accessor<boolean>;
  toggle: () => void;
}

export interface DetailsController {
  /** Register a <details> instance — returns its expand state + toggle fn. */
  register(): DetailsHandle;
  /** Toggle all: if all expanded → collapse; if any collapsed → expand all. */
  toggleAll(): void;
  /** True when at least one <details> has been registered. */
  hasItems(): boolean;
}

export function createDetailsController(): DetailsController {
  const items: Array<{ get: Accessor<boolean>; set: (v: boolean) => void }> = [];

  return {
    register() {
      const [expanded, setExpanded] = createSignal(false);
      items.push({ get: expanded, set: setExpanded });
      return {
        expanded,
        toggle: () => setExpanded(!expanded()),
      };
    },
    toggleAll() {
      if (items.length === 0) return;
      const allExpanded = items.every((i) => i.get());
      const next = !allExpanded;
      for (const item of items) {
        item.set(next);
      }
    },
    hasItems() {
      return items.length > 0;
    },
  };
}

/** Context so MarkdownBody can register <details> without prop-drilling. */
const detailsContext = createContext<DetailsController | null>(null);
export const DetailsCtx = Object.assign(detailsContext, {
  Provider: detailsContext,
}) as LegacyProviderContext<DetailsController | null>;
export const useDetails = () => useContext(DetailsCtx);
