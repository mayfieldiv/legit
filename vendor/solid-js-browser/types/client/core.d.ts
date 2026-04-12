import type { Accessor, EffectOptions } from "@solidjs/signals";
import type { JSX } from "../jsx.js";
import { FlowComponent } from "./component.js";
export declare const IS_DEV: string | boolean;
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
 * ```typescript
 * interface Context<T> {
 *   id: symbol;
 *   Provider: FlowComponent<{ value: T }>;
 *   defaultValue: T;
 * }
 * export function createContext<T>(
 *   defaultValue?: T,
 *   options?: { name?: string }
 * ): Context<T | undefined>;
 * ```
 * @param defaultValue optional default to inject into context
 * @param options allows to set a name in dev mode for debugging purposes
 * @returns The context that contains the Provider Component and that can be used with `useContext`
 *
 * @description https://docs.solidjs.com/reference/component-apis/create-context
 */
export declare function createContext<T>(
  defaultValue?: undefined,
  options?: EffectOptions,
): Context<T | undefined>;
export declare function createContext<T>(defaultValue: T, options?: EffectOptions): Context<T>;
/**
 * Uses a context to receive a scoped state from a parent's Context.Provider
 *
 * @param context Context object made by `createContext`
 * @returns the current or `defaultValue`, if present
 *
 * @description https://docs.solidjs.com/reference/component-apis/use-context
 */
export declare function useContext<T>(context: Context<T>): T;
export type ResolvedJSXElement = Exclude<JSX.Element, JSX.ArrayElement>;
export type ResolvedChildren = ResolvedJSXElement | ResolvedJSXElement[];
export type ChildrenReturn = Accessor<ResolvedChildren> & {
  toArray: () => ResolvedJSXElement[];
};
/**
 * Resolves child elements to help interact with children
 *
 * @param fn an accessor for the children
 * @returns a accessor of the same children, but resolved
 *
 * @description https://docs.solidjs.com/reference/component-apis/children
 */
export declare function children(fn: Accessor<JSX.Element>): ChildrenReturn;
export declare function devComponent<P, V>(Comp: (props: P) => V, props: P): V;
