import {
  createEffect as solidCreateEffect,
  For as solidFor,
  Match as solidMatch,
  onSettled,
  Show as solidShow,
  Switch as solidSwitch,
  untrack,
} from "solid-js";
import type { JSX as OpenTuiJSX } from "@opentui/solid";
import type { Accessor, JSX as SolidJSX } from "solid-js";

export * from "solid-js";

type OpenTuiElement = OpenTuiJSX.Element;
type NonZeroParams<T extends (...args: never[]) => unknown> = Parameters<T>["length"] extends 0
  ? never
  : T;
type ConditionalRenderCallback<T> = (item: Accessor<NonNullable<T>>) => OpenTuiElement;
type ConditionalRenderChildren<
  T,
  F extends ConditionalRenderCallback<T> = ConditionalRenderCallback<T>,
> = OpenTuiElement | NonZeroParams<F>;

function resolvePropsSource<T>(source: T | (() => T)): T {
  if (typeof source === "function") {
    return (source as () => T)();
  }
  return source;
}

export function mergeProps<T extends object[]>(...sources: T): T[number] {
  return new Proxy(
    {},
    {
      get(_target, prop) {
        for (let i = sources.length - 1; i >= 0; i--) {
          const source = resolvePropsSource(sources[i]!) as object;
          if (Reflect.has(source, prop)) {
            return Reflect.get(source, prop);
          }
        }
      },
      has(_target, prop) {
        for (let i = sources.length - 1; i >= 0; i--) {
          const source = resolvePropsSource(sources[i]!) as object;
          if (Reflect.has(source, prop)) {
            return true;
          }
        }
        return false;
      },
      ownKeys() {
        const keys = new Set<string | symbol>();
        for (const source of sources) {
          for (const key of Reflect.ownKeys(resolvePropsSource(source) as object)) {
            if (typeof key === "string" || typeof key === "symbol") {
              keys.add(key);
            }
          }
        }
        return Array.from(keys);
      },
      getOwnPropertyDescriptor(_target, prop) {
        for (let i = sources.length - 1; i >= 0; i--) {
          const source = resolvePropsSource(sources[i]!) as object;
          const descriptor = Reflect.getOwnPropertyDescriptor(source, prop);
          if (descriptor) {
            return { ...descriptor, configurable: true };
          }
        }
        return undefined;
      },
    },
  ) as T[number];
}

export function on<T, U>(
  input: Accessor<T>,
  fn: (value: T, prev: T | undefined) => U,
  options: { defer?: boolean } = {},
): (prev?: U) => U | undefined {
  let initialized = false;
  let previousInput: T | undefined;

  return () => {
    const value = input();
    if (!initialized && options.defer) {
      previousInput = value;
      initialized = true;
      return undefined;
    }

    const result = fn(value, previousInput);
    previousInput = value;
    initialized = true;
    return result;
  };
}

export function onMount(fn: () => void | (() => void)): void {
  onSettled(fn);
}

export function For<T extends readonly unknown[], U extends OpenTuiElement>(props: {
  each: T | undefined | null | false;
  fallback?: OpenTuiElement;
  keyed?: boolean | ((item: T[number]) => unknown);
  children: (item: T[number], index: Accessor<number>) => U;
}): OpenTuiElement {
  return solidFor({
    get each() {
      return props.each;
    },
    get fallback() {
      return props.fallback as unknown as SolidJSX.Element;
    },
    keyed: props.keyed,
    children: (item, index) => props.children(item(), index) as unknown as SolidJSX.Element,
  }) as OpenTuiElement;
}

export function Show<T, F extends ConditionalRenderCallback<T>>(props: {
  when: T | undefined | null | false;
  keyed?: boolean;
  fallback?: OpenTuiElement;
  children: ConditionalRenderChildren<T, F>;
}): OpenTuiElement {
  return solidShow(props as never) as OpenTuiElement;
}

export type MatchProps<T, F extends ConditionalRenderCallback<T> = ConditionalRenderCallback<T>> = {
  when: T | undefined | null | false;
  keyed?: boolean;
  children: ConditionalRenderChildren<T, F>;
};

export function Match<T, F extends ConditionalRenderCallback<T>>(
  props: MatchProps<T, F>,
): OpenTuiElement {
  return solidMatch(props as never) as OpenTuiElement;
}

export function Switch(props: {
  fallback?: OpenTuiElement;
  children: OpenTuiElement;
}): OpenTuiElement {
  return solidSwitch(props as never) as OpenTuiElement;
}

export function createEffect<T>(fn: (prev?: T) => T, value?: T): void {
  onSettled(() => {
    let previous = value as T | undefined;
    solidCreateEffect(
      () => fn(previous),
      (next) => {
        previous = next as T | undefined;
      },
      value as T,
    );
  });
}

export function createComputed<T>(fn: (prev?: T) => T, value?: T): void {
  createEffect(fn, value);
}

export function batch<T>(fn: () => T): T {
  return untrack(fn);
}
