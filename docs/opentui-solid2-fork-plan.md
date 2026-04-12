# OpenTUI Solid 2 fork plan

## Goal

Get `legit` running on:

- `solid-js@2.0.0-beta.6`
- local forked `@opentui/solid`
- `@tanstack/solid-query@6.0.0-beta.3`

without replacing `@opentui/core`.

## Current status

### Completed

A first migration spike has already been started.

Created:

- `vendor/opentui-solid2/`
  - copied from `/Users/mayfield/vendor/opentui/packages/solid/`
- `vendor/solid-js-browser/`
  - copied from installed `solid-js`, then package metadata edited to prefer `dist/solid.js` instead of `dist/server.js`
- `src/lib/solid-compat.ts`
  - temporary app-side shim for removed Solid 1 helpers

Root dependency switch already applied:

- `@opentui/core` -> `0.1.97`
- `@opentui/solid` -> `file:vendor/opentui-solid2`
- `@tanstack/solid-query` -> `6.0.0-beta.3`
- `solid-js` -> `file:vendor/solid-js-browser`

Adapter fork already patched in these areas:

- `vendor/opentui-solid2/package.json`
- `vendor/opentui-solid2/index.ts`
- `vendor/opentui-solid2/jsx-runtime.d.ts`
- `vendor/opentui-solid2/scripts/solid-plugin.ts`
- `vendor/opentui-solid2/src/elements/hooks.ts`
- `vendor/opentui-solid2/src/elements/index.ts`
- `vendor/opentui-solid2/src/reconciler.ts`
- `vendor/opentui-solid2/src/renderer/index.ts`
- `vendor/opentui-solid2/src/renderer/universal.js`
- `vendor/opentui-solid2/src/renderer/universal.d.ts`

App-side Solid 1 import patches already applied:

- `src/App.tsx`
- `src/components/DetailView.tsx`
- `src/components/ListView.tsx`
- `src/lib/details-store.ts`
- `src/lib/ui-state.ts`
- `src/lib/use-queries-lite.ts`

### What works

- `bun install` succeeds with the local fork.
- Basic adapter import smoke test succeeds:
  - `bun -e "import('@opentui/solid') ..."`
- The fork exports expected helpers like `render`, `testRender`, `useKeyboard`, `useTerminalDimensions`, `createElement`, `ref`.

### What is still broken

Targeted test still fails:

```bash
bun test tests/tui-list-view.test.tsx
```

The current hard blocker is inside the forked adapter runtime:

- `vendor/opentui-solid2/src/renderer/universal.js`

Representative error:

- `TypeError: this.$e is not a function`

This is happening in Solid 2 signals internals during OpenTUI's custom host renderer effect path.

Interpretation:

- the current fork still assumes Solid 1-style `createRenderEffect` / host renderer callback semantics
- wrapping the old code with small shims is not sufficient
- the remaining work is a real port of the custom universal renderer layer, not just package wiring

There is also a secondary app-side ordering issue visible in some failing traces:

- `ReferenceError: Cannot access '_anchor' before initialization`
- source: `src/components/ListView.tsx`

This likely reflects changed effect timing under Solid 2 and should be fixed after the host renderer is stabilized.

## Scope for first pass

Keep only the adapter surface `legit` uses:

- `render`
- `testRender`
- `useKeyboard`
- `useTerminalDimensions`
- JSX tags:
  - `box`
  - `text`
  - `span`
  - `scrollbox`
  - `a`

Defer for now:

- `Dynamic`
- `Portal`
- slot/plugin support beyond whatever the reconciler minimally requires
- runtime plugin support
- `time-to-first-draw`

## Files already changed

### Root repo

- `bun.lock`
- `package.json`
- `src/App.tsx`
- `src/components/DetailView.tsx`
- `src/components/ListView.tsx`
- `src/lib/details-store.ts`
- `src/lib/ui-state.ts`
- `src/lib/use-queries-lite.ts`
- `src/lib/solid-compat.ts`
- `docs/opentui-solid2-fork-plan.md`

### Forked adapter

- `vendor/opentui-solid2/package.json`
- `vendor/opentui-solid2/index.ts`
- `vendor/opentui-solid2/jsx-runtime.d.ts`
- `vendor/opentui-solid2/scripts/solid-plugin.ts`
- `vendor/opentui-solid2/src/elements/hooks.ts`
- `vendor/opentui-solid2/src/elements/index.ts`
- `vendor/opentui-solid2/src/reconciler.ts`
- `vendor/opentui-solid2/src/renderer/index.ts`
- `vendor/opentui-solid2/src/renderer/universal.js`
- `vendor/opentui-solid2/src/renderer/universal.d.ts`

### Local Solid override

- `vendor/solid-js-browser/package.json`
- rest copied from installed `solid-js`

## Known Solid 2 compatibility issues

Confirmed during this spike:

- Solid 2 compiler emits `ref` helper import from the adapter module for `generate: "universal"`
- current OpenTUI adapter did not provide `ref`
- current adapter imported `mergeProps` from `solid-js`
- current adapter hooks used `onMount`
- current plugin rewrote old `solid-js/store` server entry
- Solid 2 context objects no longer expose `.Provider` the old way
- Bun/Node condition resolution tends to land on Solid's server runtime unless explicitly forced away from it
- the old OpenTUI custom universal renderer logic is not directly compatible with Solid 2 effect semantics

## Important observations from the spike

### 1. `@opentui/solid` is thin; the real blocker is its host renderer

The package wiring was straightforward.

The difficult part is:

- `vendor/opentui-solid2/src/renderer/universal.js`

This file is the real migration hotspot.

### 2. The adapter still pulls a nested `solid-js`

Even with the root package using `file:vendor/solid-js-browser`, Bun still materializes:

- `node_modules/@opentui/solid/node_modules/solid-js`

Direct imports in the fork were changed to `solid-js/dist/solid.js` in a few files, but nested-resolution behavior should still be assumed to be tricky.

### 3. Small shims got us through package/API breakage, but not runtime semantics

Temporary shims solved:

- missing `ref`
- missing `mergeProps`
- missing `.Provider`
- removed Solid 1 top-level helpers used by app code

But they did **not** solve:

- host renderer effect lifecycle compatibility

## Commands already run

Dependency install:

```bash
bun install
```

Smoke import:

```bash
bun -e "import('@opentui/solid').then((m)=>{ ... })"
```

Focused failing test:

```bash
bun test tests/tui-list-view.test.tsx
```

Useful direct runtime inspections that were done earlier in the spike:

```bash
node -e "import('solid-js/dist/solid.js').then(m => console.log(Object.keys(m)))"
npm view solid-js dist-tags --json
npm view @tanstack/solid-query dist-tags --json
npm view babel-preset-solid dist-tags --json
```

## Recommended next step

Do **not** keep piling on app-side compat shims.

Next step should be:

- port `vendor/opentui-solid2/src/renderer/universal.js` properly against Solid 2 runtime expectations

Specifically:

- stop emulating Solid 1 previous-value render-effect behavior in ad hoc wrappers
- re-evaluate how Solid 2's universal compiler output expects `insert`, `spread`, `ref`, and effect scheduling to behave
- compare against a minimal Solid 2 universal renderer fixture instead of iterating blindly inside the full app

After the adapter host runtime is stable, then:

- remove as much of `src/lib/solid-compat.ts` as possible
- fix app-level ordering issues like `_anchor` in `ListView`
- rerun `tests/tui-list-view.test.tsx`
- then expand to other TUI tests
- then rerun issue #31 repro

## Additional detail needed by a fresh agent

If starting fresh, read these first:

### Source of truth

- `/Users/mayfield/vendor/opentui/packages/solid/`
- `/Users/mayfield/vendor/opentui/packages/core/`
- `docs/opentui-solid2-fork-plan.md`

### Most important files

- `vendor/opentui-solid2/src/renderer/universal.js`
- `vendor/opentui-solid2/src/reconciler.ts`
- `vendor/opentui-solid2/src/elements/hooks.ts`
- `vendor/opentui-solid2/scripts/solid-plugin.ts`
- `vendor/opentui-solid2/index.ts`
- `src/lib/solid-compat.ts`
- `src/components/ListView.tsx`

### Current root working theory

Package-level migration is mostly done.

The remaining blocker is not dependency resolution anymore; it is that the OpenTUI Solid host renderer still encodes Solid 1 assumptions about effect callbacks and insertion behavior.

### Expected first useful task for a fresh agent

Build or inspect a **minimal Solid 2 universal renderer fixture** and use that to rewrite `vendor/opentui-solid2/src/renderer/universal.js` correctly.

### Current uncommitted state

There is no commit yet.

The repo contains significant uncommitted changes under:

- root app files
- `vendor/opentui-solid2/`
- `vendor/solid-js-browser/`

Do not assume any of this is production-ready; treat it as an in-progress migration spike.
