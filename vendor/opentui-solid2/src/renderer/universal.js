import {
  createRoot,
  createRenderEffect as solidCreateRenderEffect,
  createMemo as solidCreateMemo,
  createComponent,
  untrack,
} from "solid-js/dist/solid.js";

function effect(compute, effectFn, value) {
  solidCreateRenderEffect(compute, effectFn, value);
}

const memo = (fn, equal) =>
  solidCreateMemo(fn, undefined, equal === undefined ? undefined : { equals: equal });

const resolvePropsSource = (source) => {
  if (typeof source === "function") {
    return source() ?? {};
  }
  return source ?? {};
};

const mergeProps = (...sources) =>
  new Proxy(
    {},
    {
      get(_target, prop) {
        for (let i = sources.length - 1; i >= 0; i--) {
          const source = resolvePropsSource(sources[i]);
          if (Reflect.has(source, prop)) {
            return Reflect.get(source, prop);
          }
        }
      },
      has(_target, prop) {
        for (let i = sources.length - 1; i >= 0; i--) {
          const source = resolvePropsSource(sources[i]);
          if (Reflect.has(source, prop)) {
            return true;
          }
        }
        return false;
      },
      ownKeys() {
        const keys = new Set();
        for (const source of sources) {
          for (const key of Reflect.ownKeys(resolvePropsSource(source))) {
            keys.add(key);
          }
        }
        return Array.from(keys);
      },
      getOwnPropertyDescriptor(_target, prop) {
        for (let i = sources.length - 1; i >= 0; i--) {
          const source = resolvePropsSource(sources[i]);
          const descriptor = Reflect.getOwnPropertyDescriptor(source, prop);
          if (descriptor) {
            return { ...descriptor, configurable: true };
          }
        }
      },
    },
  );

function ref(getter, node) {
  let previous;
  solidCreateRenderEffect(getter, (target) => {
    if (target === previous) return;
    previous = target;

    if (typeof target === "function") {
      target(node);
      return;
    }

    if (Array.isArray(target) && !target.includes(node)) {
      target.push(node);
    }
  });
}

export function createRenderer({
  createElement,
  createTextNode,
  createSlotNode,
  isTextNode,
  replaceText,
  insertNode,
  removeNode,
  setProperty,
  getParentNode,
  getFirstChild,
  getNextSibling,
}) {
  function insert(parent, accessor, marker, initial) {
    if (marker !== undefined && !initial) initial = [];
    if (typeof accessor !== "function") return insertExpression(parent, accessor, initial, marker);
    let current = initial;
    solidCreateRenderEffect(accessor, (value) => {
      current = insertExpression(parent, value, current, marker);
    });
  }
  function insertExpression(parent, value, current, marker, unwrapArray) {
    while (typeof current === "function") current = current();
    if (value === current) return current;
    const t = typeof value,
      multi = marker !== undefined;
    if (t === "string" || t === "number") {
      if (t === "number") value = value.toString();
      if (multi) {
        let node = current[0];
        if (node && isTextNode(node)) {
          replaceText(node, value);
        } else node = createTextNode(value);
        current = cleanChildren(parent, current, marker, node);
      } else {
        if (current !== "" && typeof current === "string") {
          replaceText(getFirstChild(parent), (current = value));
        } else {
          cleanChildren(parent, current, marker, createTextNode(value));
          current = value;
        }
      }
    } else if (value == null || t === "boolean") {
      current = cleanChildren(parent, current, marker);
    } else if (t === "function") {
      solidCreateRenderEffect(
        () => {
          let v = value();
          while (typeof v === "function") v = v();
          return v;
        },
        (v) => {
          current = insertExpression(parent, v, current, marker);
        },
      );
      return () => current;
    } else if (Array.isArray(value)) {
      const array = [];
      if (normalizeIncomingArray(array, value, unwrapArray)) {
        solidCreateRenderEffect(
          () => {
            const nextArray = [];
            normalizeIncomingArray(nextArray, value, true);
            return nextArray;
          },
          (nextArray) => {
            current = insertExpression(parent, nextArray, current, marker, true);
          },
        );
        return () => current;
      }
      if (array.length === 0) {
        const replacement = cleanChildren(parent, current, marker);
        if (multi) return (current = replacement);
      } else {
        if (Array.isArray(current)) {
          if (current.length === 0) {
            appendNodes(parent, array, marker);
          } else reconcileArrays(parent, current, array);
        } else if (current == null || current === "") {
          appendNodes(parent, array);
        } else {
          reconcileArrays(parent, (multi && current) || [getFirstChild(parent)], array);
        }
      }
      current = array;
    } else {
      if (Array.isArray(current)) {
        if (multi) return (current = cleanChildren(parent, current, marker, value));
        cleanChildren(parent, current, null, value);
      } else if (current == null || current === "" || !getFirstChild(parent)) {
        insertNode(parent, value);
      } else replaceNode(parent, value, getFirstChild(parent));
      current = value;
    }
    return current;
  }
  function normalizeIncomingArray(normalized, array, unwrap) {
    let dynamic = false;
    for (let i = 0, len = array.length; i < len; i++) {
      let item = array[i],
        t;
      if (item == null || item === true || item === false);
      else if (Array.isArray(item)) {
        dynamic = normalizeIncomingArray(normalized, item) || dynamic;
      } else if ((t = typeof item) === "string" || t === "number") {
        normalized.push(createTextNode(item));
      } else if (t === "function") {
        if (unwrap) {
          while (typeof item === "function") item = item();
          dynamic =
            normalizeIncomingArray(normalized, Array.isArray(item) ? item : [item]) || dynamic;
        } else {
          normalized.push(item);
          dynamic = true;
        }
      } else normalized.push(item);
    }
    return dynamic;
  }
  function reconcileArrays(parentNode, a, b) {
    let bLength = b.length,
      aEnd = a.length,
      bEnd = bLength,
      aStart = 0,
      bStart = 0,
      after = getNextSibling(a[aEnd - 1]),
      map = null;
    while (aStart < aEnd || bStart < bEnd) {
      if (a[aStart] === b[bStart]) {
        aStart++;
        bStart++;
        continue;
      }
      while (a[aEnd - 1] === b[bEnd - 1]) {
        aEnd--;
        bEnd--;
      }
      if (aEnd === aStart) {
        const node =
          bEnd < bLength ? (bStart ? getNextSibling(b[bStart - 1]) : b[bEnd - bStart]) : after;
        while (bStart < bEnd) insertNode(parentNode, b[bStart++], node);
      } else if (bEnd === bStart) {
        while (aStart < aEnd) {
          if (!map || !map.has(a[aStart])) removeNode(parentNode, a[aStart]);
          aStart++;
        }
      } else if (a[aStart] === b[bEnd - 1] && b[bStart] === a[aEnd - 1]) {
        const node = getNextSibling(a[--aEnd]);
        insertNode(parentNode, b[bStart++], getNextSibling(a[aStart++]));
        insertNode(parentNode, b[--bEnd], node);
        a[aEnd] = b[bEnd];
      } else {
        if (!map) {
          map = new Map();
          let i = bStart;
          while (i < bEnd) map.set(b[i], i++);
        }
        const index = map.get(a[aStart]);
        if (index != null) {
          if (bStart < index && index < bEnd) {
            let i = aStart,
              sequence = 1,
              t;
            while (++i < aEnd && i < bEnd) {
              if ((t = map.get(a[i])) == null || t !== index + sequence) break;
              sequence++;
            }
            if (sequence > index - bStart) {
              const node = a[aStart];
              while (bStart < index) insertNode(parentNode, b[bStart++], node);
            } else replaceNode(parentNode, b[bStart++], a[aStart++]);
          } else aStart++;
        } else removeNode(parentNode, a[aStart++]);
      }
    }
  }
  function cleanChildren(parent, current, marker, replacement) {
    if (marker === undefined) {
      let removed;
      while ((removed = getFirstChild(parent))) removeNode(parent, removed);
      if (replacement) {
        insertNode(parent, replacement);
      }
      return replacement ?? "";
    }
    const node = replacement || createSlotNode();
    if (current.length) {
      let inserted = false;
      for (let i = current.length - 1; i >= 0; i--) {
        const el = current[i];
        if (node !== el) {
          const isParent = getParentNode(el) === parent;
          if (!inserted && !i) {
            if (isParent) {
              replaceNode(parent, node, el);
            } else {
              insertNode(parent, node, marker);
            }
          } else if (isParent) {
            removeNode(parent, el);
          }
        } else inserted = true;
      }
    } else insertNode(parent, node, marker);
    return [node];
  }
  function appendNodes(parent, array, marker) {
    for (let i = 0, len = array.length; i < len; i++) insertNode(parent, array[i], marker);
  }
  function replaceNode(parent, newNode, oldNode) {
    insertNode(parent, newNode, oldNode);
    removeNode(parent, oldNode);
  }
  function spreadExpression(node, propsAccessor, prevProps = {}, skipChildren) {
    const readProps = () => resolvePropsSource(propsAccessor);

    if (!skipChildren) {
      let current = prevProps.children;
      solidCreateRenderEffect(
        () => readProps().children,
        (children) => {
          current = insertExpression(node, children, current);
          prevProps.children = current;
        },
      );
    }

    solidCreateRenderEffect(
      () => readProps().ref,
      (refTarget) => {
        if (typeof refTarget === "function") {
          refTarget(node);
        } else if (Array.isArray(refTarget) && !refTarget.includes(node)) {
          refTarget.push(node);
        }
      },
    );

    solidCreateRenderEffect(
      () => {
        const props = readProps();
        const nextProps = {};
        for (const prop in props) {
          if (prop === "children" || prop === "ref") continue;
          nextProps[prop] = props[prop];
        }
        return nextProps;
      },
      (props) => {
        for (const prop in props) {
          const value = props[prop];
          if (value === prevProps[prop]) continue;
          setProperty(node, prop, value, prevProps[prop]);
          prevProps[prop] = value;
        }
      },
    );

    return prevProps;
  }
  return {
    render(code, element) {
      let disposer;
      createRoot((dispose) => {
        disposer = dispose;
        insert(element, code());
      });
      return disposer;
    },
    insert,
    spread(node, accessor, skipChildren) {
      spreadExpression(node, accessor, undefined, skipChildren);
    },
    createElement,
    createTextNode,
    insertNode,
    ref,
    setProp(node, name, value, prev) {
      setProperty(node, name, value, prev);
      return value;
    },
    mergeProps,
    effect,
    memo,
    createComponent,
    use(fn, element, arg) {
      return untrack(() => fn(element, arg));
    },
  };
}
