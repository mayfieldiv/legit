import {
  createErrorBoundary as coreErrorBoundary,
  createMemo as coreMemo,
  createSignal as coreSignal,
  createOptimistic as coreOptimistic,
  createRenderEffect as coreRenderEffect,
  createEffect as coreEffect,
  $REFRESH,
  type ProjectionOptions,
  type Store,
  type StoreSetter,
  type Context,
} from "@solidjs/signals";
import { JSX } from "../jsx.js";
type HydrationSsrFields = {
  deferStream?: boolean;
  ssrSource?: "server" | "hybrid" | "initial" | "client";
};
declare module "@solidjs/signals" {
  interface MemoOptions<T> extends HydrationSsrFields {}
  interface SignalOptions<T> extends HydrationSsrFields {}
  interface EffectOptions extends HydrationSsrFields {}
}
export type HydrationProjectionOptions = ProjectionOptions & {
  ssrSource?: "server" | "hybrid" | "initial" | "client";
};
export type HydrationContext = {};
export declare const NoHydrateContext: Context<boolean>;
type SharedConfig = {
  hydrating: boolean;
  resources?: {
    [key: string]: any;
  };
  load?: (id: string) => Promise<any> | any;
  has?: (id: string) => boolean;
  gather?: (key: string) => void;
  cleanupFragment?: (id: string) => void;
  registry?: Map<string, Element>;
  completed?: WeakSet<Element> | null;
  events?: any[] | null;
  verifyHydration?: () => void;
  done: boolean;
  getNextContextId(): string;
};
export declare const sharedConfig: SharedConfig;
/**
 * Registers a callback to run once when all hydration completes
 * (all boundaries hydrated or cancelled). If hydration is already
 * complete (or not hydrating), fires via queueMicrotask.
 */
export declare function onHydrationEnd(callback: () => void): void;
export declare function enableHydration(): void;
export declare const createMemo: typeof coreMemo;
export declare const createSignal: typeof coreSignal;
export declare const createErrorBoundary: typeof coreErrorBoundary;
export declare const createOptimistic: typeof coreOptimistic;
export declare const createProjection: <T extends object = {}>(
  fn: (draft: T) => void | T | Promise<void | T> | AsyncIterable<void | T>,
  initialValue?: T,
  options?: HydrationProjectionOptions,
) => Store<T> & {
  [$REFRESH]: any;
};
type NoFn<T> = T extends Function ? never : T;
export declare const createStore: {
  <T extends object = {}>(store: NoFn<T> | Store<NoFn<T>>): [get: Store<T>, set: StoreSetter<T>];
  <T extends object = {}>(
    fn: (store: T) => void | T | Promise<void | T> | AsyncIterable<void | T>,
    store?: NoFn<T> | Store<NoFn<T>>,
    options?: HydrationProjectionOptions,
  ): [
    get: Store<T> & {
      [$REFRESH]: any;
    },
    set: StoreSetter<T>,
  ];
};
export declare const createOptimisticStore: {
  <T extends object = {}>(store: NoFn<T> | Store<NoFn<T>>): [get: Store<T>, set: StoreSetter<T>];
  <T extends object = {}>(
    fn: (store: T) => void | T | Promise<void | T> | AsyncIterable<void | T>,
    store?: NoFn<T> | Store<NoFn<T>>,
    options?: HydrationProjectionOptions,
  ): [
    get: Store<T> & {
      [$REFRESH]: any;
    },
    set: StoreSetter<T>,
  ];
};
export declare const createRenderEffect: typeof coreRenderEffect;
export declare const createEffect: typeof coreEffect;
/**
 * Tracks all resources inside a component and renders a fallback until they are all resolved
 * ```typescript
 * const AsyncComponent = lazy(() => import('./component'));
 *
 * <Loading fallback={<LoadingIndicator />}>
 *   <AsyncComponent />
 * </Loading>
 * ```
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
 * Disables hydration for its children on the client.
 * During hydration, skips the subtree entirely (returns undefined so DOM is left untouched).
 * After hydration, renders children fresh.
 */
export declare function NoHydration(props: { children: JSX.Element }): JSX.Element;
/**
 * Re-enables hydration within a NoHydration zone (passthrough on client).
 */
export declare function Hydration(props: { id?: string; children: JSX.Element }): JSX.Element;
