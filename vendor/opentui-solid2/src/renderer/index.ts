import { createRenderer as createRendererDX } from "./universal.js";
import type { RendererOptions, Renderer } from "./universal.js";

export type { RendererOptions, Renderer } from "./universal.js";

export function createRenderer<NodeType>(options: RendererOptions<NodeType>): Renderer<NodeType> {
  return createRendererDX(options);
}
