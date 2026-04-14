import { CliRenderer, createCliRenderer, engine, type CliRendererConfig } from "@opentui/core";
import { createTestRenderer, type TestRendererOptions } from "@opentui/core/testing";
import {
  For as solidFor,
  Match as solidMatch,
  Show as solidShow,
  Switch as solidSwitch,
} from "solid-js/dist/solid.js";
import type { Accessor } from "solid-js";
import type { JSX } from "./jsx-runtime";
import { RendererContext } from "./src/elements/index.js";
import { _render as renderInternal, createComponent } from "./src/reconciler.js";

type DisposeFn = () => void;
type RenderInternal = (node: () => JSX.Element, root: CliRenderer["root"]) => DisposeFn;
type RenderComponent = <T>(Comp: (props: T) => JSX.Element, props: T) => JSX.Element;
type ConditionalRenderCallback<T> = (item: Accessor<NonNullable<T>>) => JSX.Element;
type ConditionalRenderChildren<
  T,
  F extends ConditionalRenderCallback<T> = ConditionalRenderCallback<T>,
> = JSX.Element | (Parameters<F>["length"] extends 0 ? never : F);

const renderJsx = renderInternal as unknown as RenderInternal;
const renderComponent = createComponent as unknown as RenderComponent;

export const For = solidFor as unknown as <
  T extends readonly unknown[],
  U extends JSX.Element,
>(props: {
  each: T | undefined | null | false;
  fallback?: JSX.Element;
  keyed?: boolean | ((item: T[number]) => unknown);
  children: (item: Accessor<T[number]>, index: Accessor<number>) => U;
}) => JSX.Element;

export const Show = solidShow as unknown as <T, F extends ConditionalRenderCallback<T>>(props: {
  when: T | undefined | null | false;
  keyed?: boolean;
  fallback?: JSX.Element;
  children: ConditionalRenderChildren<T, F>;
}) => JSX.Element;

export type MatchProps<T, F extends ConditionalRenderCallback<T> = ConditionalRenderCallback<T>> = {
  when: T | undefined | null | false;
  keyed?: boolean;
  children: ConditionalRenderChildren<T, F>;
};

export const Match = solidMatch as unknown as <T, F extends ConditionalRenderCallback<T>>(
  props: MatchProps<T, F>,
) => JSX.Element;

export const Switch = solidSwitch as unknown as (props: {
  fallback?: JSX.Element;
  children: JSX.Element;
}) => JSX.Element;

// Mount a Solid root into a CliRenderer, deferring any destroy/dispose
// calls that happen synchronously during the initial render.
const mountSolidRoot = (renderer: CliRenderer, node: () => JSX.Element) => {
  let dispose: DisposeFn | undefined;
  let mounting = true;
  let deferredDispose = false;
  let deferredDestroy = false;

  const originalDestroy = renderer.destroy.bind(renderer);

  // Dispose the Solid root when the renderer is destroyed. If the event
  // fires during mount (before dispose is available), defer until after.
  renderer.once("destroy", () => {
    if (dispose) {
      dispose();
    } else {
      deferredDispose = true;
    }
  });

  // Defer renderer.destroy() calls during mount to avoid tearing down
  // the renderer while renderJsx is still synchronously executing.
  renderer.destroy = () => {
    if (mounting) {
      deferredDestroy = true;
      return;
    }
    originalDestroy();
  };

  try {
    dispose = renderJsx(
      () =>
        renderComponent(
          RendererContext as unknown as (props: {
            value: CliRenderer;
            children: JSX.Element;
          }) => JSX.Element,
          {
            get value() {
              return renderer;
            },
            get children() {
              return renderComponent(node as unknown as (props: {}) => JSX.Element, {});
            },
          },
        ),
      renderer.root,
    );
  } finally {
    mounting = false;
    renderer.destroy = originalDestroy;
  }

  if (deferredDispose) dispose();
  if (deferredDestroy) originalDestroy();
};

export const render = async (
  node: () => JSX.Element,
  rendererOrConfig: CliRenderer | CliRendererConfig = {},
) => {
  const renderer =
    rendererOrConfig instanceof CliRenderer
      ? rendererOrConfig
      : await createCliRenderer({
          ...rendererOrConfig,
          onDestroy: () => {
            rendererOrConfig.onDestroy?.();
          },
        });

  engine.attach(renderer);
  mountSolidRoot(renderer, node);
};

export const testRender = async (
  node: () => JSX.Element,
  renderConfig: TestRendererOptions = {},
) => {
  const testSetup = await createTestRenderer({
    ...renderConfig,
    onDestroy: () => {
      renderConfig.onDestroy?.();
    },
  });

  engine.attach(testSetup.renderer);
  mountSolidRoot(testSetup.renderer, node);

  return testSetup;
};

export * from "./src/reconciler.js";
export * from "./src/elements/index.js";
export * from "./src/types/elements.js";
export { type JSX };
