/* @refresh skip */
import {
  BaseRenderable,
  createTextAttributes,
  InputRenderable,
  InputRenderableEvents,
  isTextNodeRenderable,
  parseColor,
  Renderable,
  RootTextNodeRenderable,
  ScrollBoxRenderable,
  SelectRenderable,
  SelectRenderableEvents,
  TabSelectRenderable,
  TabSelectRenderableEvents,
  TextNodeRenderable,
  TextRenderable,
  type TextNodeOptions,
} from "@opentui/core";
import { decodeHTML } from "entities";
import { useContext } from "solid-js/dist/solid.js";
import { createRenderer } from "./renderer/index.js";
import { getComponentCatalogue, RendererContext, SlotRenderable } from "./elements/index.js";
import { getNextId } from "./utils/id-counter.js";
import { log } from "./utils/log.js";

class TextNode extends TextNodeRenderable {
  public static override fromString(
    text: string,
    options: Partial<TextNodeOptions> = {},
  ): TextNode {
    const node = new TextNode(options);
    node.add(text);
    return node;
  }
}

export type DomNode = BaseRenderable;

/**
 * Gets the id of a node, or content if it's a text chunk.
 * Intended for use in logging.
 * @param node The node to get the id of.
 * @returns Log-friendly id of the node.
 */
const logId = (node?: DomNode): string | undefined => {
  if (!node) return undefined;
  return node.id;
};

const getNodeChildren = (node: DomNode) => {
  let children;
  if (node instanceof TextRenderable) {
    children = node.getTextChildren();
  } else {
    children = node.getChildren();
  }
  return children;
};

function _insertNode(parent: DomNode, node: DomNode, anchor?: DomNode): void {
  log(
    "Inserting node:",
    logId(node),
    "into parent:",
    logId(parent),
    "with anchor:",
    logId(anchor),
    node instanceof TextNode,
  );

  if (node instanceof SlotRenderable) {
    node.parent = parent;
    node = node.getSlotChild(parent);
  }

  if (anchor && anchor instanceof SlotRenderable) {
    anchor = anchor.getSlotChild(parent);
  }

  if (isTextNodeRenderable(node)) {
    if (!(parent instanceof TextRenderable) && !isTextNodeRenderable(parent)) {
      throw new Error(
        `Orphan text error: "${node
          .toChunks()
          .map((c) => c.text)
          .join("")}" must have a <text> as a parent: ${parent.id} above ${node.id}`,
      );
    }
  }

  // Renderable nodes
  if (!(parent instanceof BaseRenderable)) {
    console.error("[INSERT]", "Tried to mount a non base renderable");
    // Can't be a noop, have to panic
    throw new Error("Tried to mount a non base renderable");
  }

  if (!anchor) {
    parent.add(node);
    return;
  }

  const children = getNodeChildren(parent);

  const anchorIndex = children.findIndex((el) => el.id === anchor.id);
  if (anchorIndex === -1) {
    log(
      "[INSERT]",
      "Could not find anchor",
      logId(parent),
      logId(anchor),
      "[children]",
      ...children.map((c) => c.id),
    );
  }

  parent.add(node, anchorIndex);
}

function _removeNode(parent: DomNode, node: DomNode): void {
  log("Removing node:", logId(node), "from parent:", logId(parent));

  if (node instanceof SlotRenderable) {
    node.parent = null;
    node = node.getSlotChild(parent);
  }

  parent.remove(node.id);

  process.nextTick(() => {
    if (node instanceof BaseRenderable && !node.parent) {
      node.destroyRecursively();
      return;
    }
  });
}

function _createTextNode(value: string | number): TextNode {
  log("Creating text node:", value);

  const id = getNextId("text-node");

  if (typeof value === "number") {
    value = value.toString();
  }

  return TextNode.fromString(decodeHTML(value), { id });
}

export function createSlotNode(): SlotRenderable {
  const id = getNextId("slot-node");
  log("Creating slot node", id);
  return new SlotRenderable(id);
}

function _getParentNode(childNode: DomNode): DomNode | undefined {
  log("Getting parent of node:", logId(childNode));

  let parent = childNode.parent ?? undefined;
  if (parent instanceof RootTextNodeRenderable) {
    parent = parent.textParent ?? undefined;
  }
  // ScrollBox delegates add/remove to its internal `content` wrapper
  // (scrollbox → wrapper → viewport → content), so children report
  // `content` as their parent. Return the ScrollBox so the identity
  // check in cleanChildren (getParentNode(el) === parent) succeeds.
  const scrollBoxCandidate = parent?.parent?.parent?.parent;
  if (scrollBoxCandidate instanceof ScrollBoxRenderable && scrollBoxCandidate.content === parent) {
    parent = scrollBoxCandidate;
  }
  return parent;
}

export const {
  render: _render,
  effect,
  memo,
  createComponent,
  createElement,
  createTextNode,
  insertNode,
  insert,
  spread,
  ref,
  setProp,
  mergeProps,
  use,
} = createRenderer<DomNode>({
  createElement(tagName: string): DomNode {
    log("Creating element:", tagName);
    const id = getNextId(tagName);
    const solidRenderer = useContext(RendererContext);
    if (!solidRenderer) {
      throw new Error("No renderer found");
    }
    const elements = getComponentCatalogue();

    if (!elements[tagName]) {
      throw new Error(`[Reconciler] Unknown component type: ${tagName}`);
    }

    const element = new elements[tagName](solidRenderer, { id });
    log("Element created with id:", id);
    return element;
  },

  createTextNode: _createTextNode,

  createSlotNode,

  replaceText(textNode: TextNode, value: string): void {
    log("Replacing text:", value, "in node:", logId(textNode));

    if (!(textNode instanceof TextNode)) return;
    textNode.replace(decodeHTML(value), 0);
  },

  // Property values originate from Solid compiler-generated JSX and are
  // inherently dynamic. We accept `unknown` and narrow per-case.
  setProperty(node: DomNode, name: string, value: unknown, prev: unknown): void {
    // Dynamic property bag — used in cases where we set arbitrary
    // properties on the node that BaseRenderable's type doesn't expose.
    const dynNode = node as unknown as Record<string, unknown>;

    if (name.startsWith("on:")) {
      const eventName = name.slice(3);
      if (value) {
        node.on(eventName, value as (...args: unknown[]) => void);
      }
      if (prev) {
        node.off(eventName, prev as (...args: unknown[]) => void);
      }

      return;
    }

    if (isTextNodeRenderable(node)) {
      if (name === "href") {
        node.link = { url: value as string };
        return;
      }

      if (name === "style") {
        const style = value as Record<string, string>;
        node.attributes |= createTextAttributes(style);
        node.fg = style.fg ? parseColor(style.fg) : node.fg;
        node.bg = style.bg ? parseColor(style.bg) : node.bg;
        return;
      }

      return;
    }

    switch (name) {
      case "id":
        log("Id mapped", node.id, "=", value);
        node[name] = value as string;
        break;
      case "focused":
        if (!(node instanceof Renderable)) return;
        if (value) {
          node.focus();
        } else {
          node.blur();
        }
        break;
      case "onChange": {
        let event: string | undefined = undefined;
        if (node instanceof SelectRenderable) {
          event = SelectRenderableEvents.SELECTION_CHANGED;
        } else if (node instanceof TabSelectRenderable) {
          event = TabSelectRenderableEvents.SELECTION_CHANGED;
        } else if (node instanceof InputRenderable) {
          event = InputRenderableEvents.CHANGE;
        }
        if (!event) break;

        if (value) {
          node.on(event, value as (...args: unknown[]) => void);
        }
        if (prev) {
          node.off(event, prev as (...args: unknown[]) => void);
        }
        break;
      }
      case "onInput":
        if (node instanceof InputRenderable) {
          if (value) {
            node.on(InputRenderableEvents.INPUT, value as (...args: unknown[]) => void);
          }

          if (prev) {
            node.off(InputRenderableEvents.INPUT, prev as (...args: unknown[]) => void);
          }
        }

        break;
      case "onSubmit":
        if (node instanceof InputRenderable) {
          if (value) {
            node.on(InputRenderableEvents.ENTER, value as (...args: unknown[]) => void);
          }

          if (prev) {
            node.off(InputRenderableEvents.ENTER, prev as (...args: unknown[]) => void);
          }
        } else {
          dynNode[name] = value;
        }
        break;
      case "onSelect":
        if (node instanceof SelectRenderable) {
          if (value) {
            node.on(SelectRenderableEvents.ITEM_SELECTED, value as (...args: unknown[]) => void);
          }

          if (prev) {
            node.off(SelectRenderableEvents.ITEM_SELECTED, prev as (...args: unknown[]) => void);
          }
        } else if (node instanceof TabSelectRenderable) {
          if (value) {
            node.on(TabSelectRenderableEvents.ITEM_SELECTED, value as (...args: unknown[]) => void);
          }

          if (prev) {
            node.off(TabSelectRenderableEvents.ITEM_SELECTED, prev as (...args: unknown[]) => void);
          }
        }
        break;
      case "style": {
        const styleValue = value as Record<string, unknown>;
        const stylePrev = prev as Record<string, unknown> | undefined;
        for (const prop in styleValue) {
          const propVal = styleValue[prop];
          if (stylePrev !== undefined && propVal === stylePrev[prop]) continue;
          dynNode[prop] = propVal;
        }
        break;
      }
      case "text":
      case "content": {
        const textValue =
          typeof value === "string" ? value : Array.isArray(value) ? value.join("") : `${value}`;
        dynNode[name] = decodeHTML(textValue);
        break;
      }

      default:
        dynNode[name] = value;
    }
  },

  isTextNode(node: DomNode): boolean {
    return node instanceof TextNode;
  },

  insertNode: _insertNode,

  removeNode: _removeNode,

  getParentNode: _getParentNode,

  getFirstChild(node: DomNode): DomNode | undefined {
    log("Getting first child of node:", logId(node));

    const firstChild = getNodeChildren(node)[0];

    if (!firstChild) {
      log("No first child found for node:", logId(node));
      return undefined;
    }

    log("First child found:", logId(firstChild), "for node:", logId(node));
    return firstChild;
  },

  getNextSibling(node: DomNode): DomNode | undefined {
    log("Getting next sibling of node:", logId(node));

    const parent = _getParentNode(node);
    if (!parent) {
      log("No parent found for node:", logId(node));
      return undefined;
    }
    const siblings = getNodeChildren(parent);

    const index = siblings.indexOf(node);

    if (index === -1 || index === siblings.length - 1) {
      log("No next sibling found for node:", logId(node));
      return undefined;
    }

    const nextSibling = siblings[index + 1];

    if (!nextSibling) {
      log("Next sibling is null for node:", logId(node));
      return undefined;
    }

    log("Next sibling found:", logId(nextSibling), "for node:", logId(node));
    return nextSibling;
  },
});
