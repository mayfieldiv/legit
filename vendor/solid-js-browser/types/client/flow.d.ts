import type { Accessor } from "@solidjs/signals";
import type { JSX } from "../jsx.js";
type NonZeroParams<T extends (...args: any[]) => any> = Parameters<T>["length"] extends 0
  ? never
  : T;
type ConditionalRenderCallback<T> = (item: Accessor<NonNullable<T>>) => JSX.Element;
type ConditionalRenderChildren<
  T,
  F extends ConditionalRenderCallback<T> = ConditionalRenderCallback<T>,
> = JSX.Element | NonZeroParams<F>;
/**
 * Creates a list of elements from a list
 *
 * it receives a map function as its child that receives list element and index accessors and returns a JSX-Element; if the list is empty, an optional fallback is returned:
 * ```typescript
 * <For each={items} fallback={<div>No items</div>}>
 *   {(item, index) => <div data-index={index()}>{item()}</div>}
 * </For>
 * ```
 *
 * @description https://docs.solidjs.com/reference/components/for
 */
export declare function For<T extends readonly any[], U extends JSX.Element>(props: {
  each: T | undefined | null | false;
  fallback?: JSX.Element;
  keyed?: boolean | ((item: T[number]) => any);
  children: (item: Accessor<T[number]>, index: Accessor<number>) => U;
}): JSX.Element;
/**
 * Creates a list elements from a count
 *
 * it receives a map function as its child that receives the index and returns a JSX-Element; if the list is empty, an optional fallback is returned:
 * ```typescript
 * <Repeat count={items.length} fallback={<div>No items</div>}>
 *   {(index) => <div data-index={index}>{items[index]}</div>}
 * </Repeat>
 * ```
 *
 * @description https://docs.solidjs.com/reference/components/repeat
 */
export declare function Repeat<T extends JSX.Element>(props: {
  count: number;
  from?: number | undefined;
  fallback?: JSX.Element;
  children: ((index: number) => T) | T;
}): JSX.Element;
/**
 * Conditionally render its children or an optional fallback component
 * @description https://docs.solidjs.com/reference/components/show
 */
export declare function Show<T, F extends ConditionalRenderCallback<T>>(props: {
  when: T | undefined | null | false;
  keyed?: boolean;
  fallback?: JSX.Element;
  children: ConditionalRenderChildren<T, F>;
}): JSX.Element;
/**
 * Switches between content based on mutually exclusive conditions
 * ```typescript
 * <Switch fallback={<FourOhFour />}>
 *   <Match when={state.route === 'home'}>
 *     <Home />
 *   </Match>
 *   <Match when={state.route === 'settings'}>
 *     <Settings />
 *   </Match>
 * </Switch>
 * ```
 * @description https://docs.solidjs.com/reference/components/switch-and-match
 */
export declare function Switch(props: {
  fallback?: JSX.Element;
  children: JSX.Element;
}): JSX.Element;
export type MatchProps<T, F extends ConditionalRenderCallback<T> = ConditionalRenderCallback<T>> = {
  when: T | undefined | null | false;
  keyed?: boolean;
  children: ConditionalRenderChildren<T, F>;
};
/**
 * Selects a content based on condition when inside a `<Switch>` control flow
 * ```typescript
 * <Match when={condition()}>
 *   <Content/>
 * </Match>
 * ```
 * @description https://docs.solidjs.com/reference/components/switch-and-match
 */
export declare function Match<T, F extends ConditionalRenderCallback<T>>(
  props: MatchProps<T, F>,
): JSX.Element;
/**
 * Catches uncaught errors inside components and renders a fallback content
 *
 * Also supports a callback form that passes the error and a reset function:
 * ```typescript
 * <Errored fallback={
 *   (err, reset) => <div onClick={reset}>Error: {err.toString()}</div>
 * }>
 *   <MyComp />
 * </Errored>
 * ```
 * Errors thrown from the fallback can be caught by a parent Errored
 *
 * @description https://docs.solidjs.com/reference/components/error-boundary
 */
export declare function Errored(props: {
  fallback: JSX.Element | ((err: any, reset: () => void) => JSX.Element);
  children: JSX.Element;
}): JSX.Element;
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
export declare function Loading(props: {
  fallback?: JSX.Element;
  on?: any;
  children: JSX.Element;
}): JSX.Element;
/**
 * Coordinates the reveal timing of sibling `<Loading>` boundaries.
 *
 * - **Sequential** (default): boundaries reveal in DOM order as each resolves.
 * - **Together** (`together`): all boundaries wait until the group is ready, then reveal at once.
 * - **Collapsed** (`collapsed`, sequential only): only the frontier boundary shows its fallback;
 *   later boundaries produce nothing until their turn.
 *
 * ```typescript
 * <Reveal>
 *   <Loading fallback={<Skeleton />}><ProfileHeader /></Loading>
 *   <Loading fallback={<Skeleton />}><Posts /></Loading>
 * </Reveal>
 * ```
 *
 * @description https://docs.solidjs.com/reference/components/reveal
 */
export declare function Reveal(props: {
  together?: boolean;
  collapsed?: boolean;
  children: JSX.Element;
}): JSX.Element;
