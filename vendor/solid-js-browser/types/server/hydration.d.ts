import type { Context } from "./signals.js";
import type { JSX } from "../jsx.js";
export { sharedConfig, NoHydrateContext } from "./shared.js";
export type { HydrationContext, SSRTemplateObject } from "./shared.js";
export type ServerRevealGroup = {
  id: string;
  register(
    key: string,
    options?: {
      onActivate?: () => void;
    },
  ): boolean;
  onResolved(key: string): void;
};
export declare const RevealGroupContext: Context<ServerRevealGroup | null>;
/**
 * Handles errors during SSR rendering.
 * Returns the promise source for NotReadyError (for async handling),
 * or delegates to the ErrorContext handler.
 */
export declare function ssrHandleError(err: any): Promise<any> | undefined;
/**
 * Tracks all resources inside a component and renders a fallback until they are all resolved
 *
 * On the server, this is SSR-aware: it handles async mode (streaming) by registering
 * fragments and resolving asynchronously, and sync mode by serializing fallback markers.
 *
 * @description https://docs.solidjs.com/reference/components/suspense
 */
export declare function createLoadingBoundary(
  fn: () => any,
  fallback: () => any,
  options?: {
    on?: () => any;
  },
): () => unknown;
/**
 * Disables hydration for its children during SSR.
 * Elements inside will not receive hydration keys (`_hk`) and signals will not be serialized.
 * Use `Hydration` to re-enable hydration within a `NoHydration` zone.
 */
export declare function NoHydration(props: { children: JSX.Element }): JSX.Element;
/**
 * Re-enables hydration within a `NoHydration` zone, establishing a new ID namespace.
 * Pass an `id` prop matching the client's `hydrate({ renderId })` to align hydration keys.
 * Has no effect when not inside a `NoHydration` zone (passthrough).
 */
export declare function Hydration(props: { id?: string; children: JSX.Element }): JSX.Element;
