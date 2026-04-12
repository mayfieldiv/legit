export {
  $PROXY,
  $REFRESH,
  $TRACK,
  action,
  createOwner,
  createReaction,
  createRevealOrder,
  createRoot,
  createTrackedEffect,
  deep,
  flatten,
  flush,
  getNextChildId,
  getObserver,
  getOwner,
  isDisposed,
  isEqual,
  isRefreshing,
  isPending,
  isWrappable,
  mapArray,
  merge,
  omit,
  onCleanup,
  onSettled,
  latest,
  reconcile,
  refresh,
  repeat,
  resolve,
  NotReadyError,
  runWithOwner,
  enableExternalSource,
  enforceLoadingBoundary,
  snapshot,
  storePath,
  untrack,
} from "@solidjs/signals";
export type {
  Accessor,
  ComputeFunction,
  EffectFunction,
  EffectOptions,
  ExternalSource,
  ExternalSourceConfig,
  ExternalSourceFactory,
  Merge,
  NoInfer,
  NotWrappable,
  Omit,
  Owner,
  Signal,
  SignalOptions,
  Setter,
  Store,
  SolidStore,
  StoreNode,
  StoreSetter,
  StorePathRange,
  ArrayFilterFn,
  CustomPartial,
  Part,
  PathSetter,
} from "@solidjs/signals";
export { $DEVCOMP, children, createContext, useContext } from "./client/core.js";
export type {
  ChildrenReturn,
  Context,
  ContextProviderComponent,
  ResolvedChildren,
  ResolvedJSXElement,
} from "./client/core.js";
export * from "./client/component.js";
export * from "./client/flow.js";
export {
  sharedConfig,
  enableHydration,
  createErrorBoundary,
  createLoadingBoundary,
  createMemo,
  createSignal,
  createStore,
  createProjection,
  createOptimistic,
  createOptimisticStore,
  createRenderEffect,
  createEffect,
  NoHydration,
  Hydration,
  NoHydrateContext,
} from "./client/hydration.js";
export declare function ssrHandleError(): void;
export declare function ssrRunInScope(): void;
import type { JSX } from "./jsx.js";
type JSXElement = JSX.Element;
export type { JSXElement, JSX };
import { type Dev } from "@solidjs/signals";
export declare const DEV: Dev | undefined;
declare global {
  var Solid$$: boolean;
}
