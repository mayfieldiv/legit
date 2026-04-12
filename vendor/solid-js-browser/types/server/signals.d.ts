export {
  createRoot,
  createOwner,
  runWithOwner,
  getOwner,
  isDisposed,
  onCleanup,
  getNextChildId,
  createContext,
  setContext,
  getContext,
  NotReadyError,
  NoOwnerError,
  ContextNotFoundError,
  isEqual,
  isWrappable,
  SUPPORTS_PROXY,
  enableExternalSource,
  enforceLoadingBoundary,
} from "@solidjs/signals";
export { flatten } from "@solidjs/signals";
export { snapshot, merge, omit, storePath, $PROXY, $REFRESH, $TRACK } from "@solidjs/signals";
export type {
  Accessor,
  ComputeFunction,
  EffectFunction,
  EffectBundle,
  EffectOptions,
  ExternalSource,
  ExternalSourceConfig,
  ExternalSourceFactory,
  MemoOptions,
  NoInfer,
  SignalOptions,
  Setter,
  Signal,
  Owner,
  Maybe,
  Store,
  StoreSetter,
  StoreNode,
  NotWrappable,
  SolidStore,
  Merge,
  Omit,
  Context,
  ContextRecord,
  IQueue,
  StorePathRange,
  ArrayFilterFn,
  CustomPartial,
  Part,
  PathSetter,
} from "@solidjs/signals";
import type {
  Accessor,
  ComputeFunction,
  EffectFunction,
  EffectBundle,
  EffectOptions,
  MemoOptions,
  SignalOptions,
  Signal,
  Owner,
  Store,
  StoreSetter,
  Context,
} from "@solidjs/signals";
import { sharedConfig, NoHydrateContext } from "./shared.js";
interface ServerComputation<T = any> {
  owner: Owner;
  value: T;
  compute: ComputeFunction<any, T>;
  error: unknown;
  computed: boolean;
  disposed: boolean;
}
export declare function getObserver(): ServerComputation<any> | null;
export declare function createSignal<T>(): Signal<T | undefined>;
export declare function createSignal<T>(
  value: Exclude<T, Function>,
  options?: SignalOptions<T>,
): Signal<T>;
export declare function createSignal<T>(
  fn: ComputeFunction<T>,
  initialValue?: T,
  options?: SignalOptions<T>,
): Signal<T>;
export declare function createMemo<Next extends Prev, Prev = Next>(
  compute: ComputeFunction<undefined | NoInfer<Prev>, Next>,
): Accessor<Next>;
export declare function createMemo<Next extends Prev, Init = Next, Prev = Next>(
  compute: ComputeFunction<Init | Prev, Next>,
  value: Init,
  options?: MemoOptions<Next>,
): Accessor<Next>;
export type PatchOp =
  | [path: PropertyKey[]]
  | [path: PropertyKey[], value: any]
  | [path: PropertyKey[], value: any, insert: 1];
export declare function createDeepProxy<T extends object>(
  target: T,
  patches: PatchOp[],
  basePath?: PropertyKey[],
): T;
export declare function createEffect<Next>(
  compute: ComputeFunction<undefined | NoInfer<Next>, Next>,
  effectFn: EffectFunction<NoInfer<Next>, Next> | EffectBundle<NoInfer<Next>, Next>,
): void;
export declare function createEffect<Next, Init = Next>(
  compute: ComputeFunction<Init | Next, Next>,
  effect: EffectFunction<Next, Next> | EffectBundle<Next, Next>,
  value: Init,
  options?: EffectOptions,
): void;
export declare function createRenderEffect<Next>(
  compute: ComputeFunction<undefined | NoInfer<Next>, Next>,
  effectFn: EffectFunction<NoInfer<Next>, Next>,
): void;
export declare function createRenderEffect<Next, Init = Next>(
  compute: ComputeFunction<Init | Next, Next>,
  effectFn: EffectFunction<Next, Next>,
  value: Init,
  options?: EffectOptions,
): void;
export declare function createTrackedEffect(
  compute: () => void | (() => void),
  options?: EffectOptions,
): void;
export declare function createReaction(
  effectFn: EffectFunction<undefined> | EffectBundle<undefined>,
  options?: EffectOptions,
): (tracking: () => void) => void;
export declare function createOptimistic<T>(): Signal<T | undefined>;
export declare function createOptimistic<T>(
  value: Exclude<T, Function>,
  options?: SignalOptions<T>,
): Signal<T>;
export declare function createOptimistic<T>(
  fn: ComputeFunction<T>,
  initialValue?: T,
  options?: SignalOptions<T>,
): Signal<T>;
export declare function createStore<T extends object>(
  first: T | Store<T> | ((store: T) => void | T | Promise<void | T>),
  second?: T | Store<T>,
): [get: Store<T>, set: StoreSetter<T>];
export declare const createOptimisticStore: typeof createStore;
export declare function createProjection<T extends object>(
  fn: (draft: T) => void | T | Promise<void | T> | AsyncIterable<void | T>,
  initialValue?: T,
  options?: {
    deferStream?: boolean;
    ssrSource?: string;
  },
): Store<T>;
export declare function reconcile<T extends U, U extends object>(value: T): (state: U) => T;
export declare function deep<T extends object>(store: Store<T>): Store<T>;
export declare function mapArray<T, U>(
  list: Accessor<readonly T[] | undefined | null | false>,
  mapFn: (v: Accessor<T>, i: Accessor<number>) => U,
  options?: {
    keyed?: boolean | ((item: T) => any);
    fallback?: Accessor<any>;
  },
): () => U[];
export declare function repeat<T>(
  count: Accessor<number>,
  mapFn: (i: number) => T,
  options?: {
    fallback?: Accessor<any>;
    from?: Accessor<number | undefined>;
  },
): () => T[];
declare const ErrorContext: Context<((err: any) => void) | null>;
export { ErrorContext };
export declare function runWithBoundaryErrorContext<T>(
  owner: Owner,
  render: () => T,
  onError: (err: any, parentHandler: ((err: any) => void) | null) => void,
  context?: NonNullable<typeof sharedConfig.context>,
  boundaryId?: string,
): T;
export { NoHydrateContext };
export declare function createErrorBoundary<U>(
  fn: () => any,
  fallback: (error: unknown, reset: () => void) => U,
): () => unknown;
export declare function createLoadingBoundary(
  fn: () => any,
  fallback: () => any,
  options?: {
    on?: () => any;
  },
): () => unknown;
export declare function createRevealOrder<T>(
  fn: () => T,
  _options?: {
    together?: () => boolean;
    collapsed?: () => boolean;
  },
): T;
export declare function untrack<T>(fn: () => T): T;
export declare function flush(): void;
export declare function resolve<T>(fn: () => T): Promise<T>;
export declare function isPending(fn: () => any, fallback?: boolean): boolean;
export declare function latest<T>(fn: () => T): T;
export declare function isRefreshing(): boolean;
export declare function refresh<T>(fn: () => T): T;
export declare function action<T extends (...args: any[]) => any>(fn: T): T;
export declare function onSettled(callback: () => void | (() => void)): void;
type NoInfer<T> = [T][T extends unknown ? 0 : never];
