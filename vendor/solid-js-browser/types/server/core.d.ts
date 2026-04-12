import type { Accessor, EffectOptions } from "./signals.js";
import type { JSX } from "../jsx.js";
import type { FlowComponent } from "./component.js";
export declare const $DEVCOMP: unique symbol;
export type NoInfer<T> = [T][T extends unknown ? 0 : never];
export type ContextProviderComponent<T> = FlowComponent<{
  value: T;
}>;
export interface Context<T> extends ContextProviderComponent<T> {
  id: symbol;
  defaultValue: T;
}
/**
 * Creates a Context to handle a state scoped for the children of a component
 * @param defaultValue optional default to inject into context
 * @param options allows to set a name in dev mode for debugging purposes
 * @returns The context that contains the Provider Component and that can be used with `useContext`
 */
export declare function createContext<T>(
  defaultValue?: undefined,
  options?: EffectOptions,
): Context<T | undefined>;
export declare function createContext<T>(defaultValue: T, options?: EffectOptions): Context<T>;
/**
 * Uses a context to receive a scoped state from a parent's Context.Provider
 * @param context Context object made by `createContext`
 * @returns the current or `defaultValue`, if present
 */
export declare function useContext<T>(context: Context<T>): T;
export type ResolvedJSXElement = Exclude<JSX.Element, JSX.ArrayElement>;
export type ResolvedChildren = ResolvedJSXElement | ResolvedJSXElement[];
export type ChildrenReturn = Accessor<ResolvedChildren> & {
  toArray: () => ResolvedJSXElement[];
};
/**
 * Resolves child elements to help interact with children
 * @param fn an accessor for the children
 * @returns a accessor of the same children, but resolved
 */
export declare function children(fn: Accessor<JSX.Element>): ChildrenReturn;
/**
 * Pass-through for SSR dynamic expressions.
 * On the client, insert() render effects are transparent (0 owner slots),
 * so the server doesn't need to create owners for these either.
 */
export declare function ssrRunInScope(fn: () => any): () => any;
export declare function ssrRunInScope(array: (() => any)[]): (() => any)[];
