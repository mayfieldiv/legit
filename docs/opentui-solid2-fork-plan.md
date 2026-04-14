# OpenTUI Solid 2 fork status

## Goal

Get `legit` running on:

- `solid-js@2.0.0-beta.6`
- local forked `@opentui/solid`
- `@tanstack/solid-query@6.0.0-beta.3`

without replacing `@opentui/core`.

## Current Status

The local OpenTUI Solid fork now owns the Solid 2 compatibility boundary. The app no longer has an app-side Solid compatibility module.

Completed:

- Root dependencies point at the local Solid 2-compatible packages:
  - `@opentui/core` -> `0.1.97`
  - `@opentui/solid` -> `file:vendor/opentui-solid2`
  - `@tanstack/solid-query` -> `6.0.0-beta.3`
  - `solid-js` -> `file:vendor/solid-js-browser`
- `vendor/opentui-solid2/src/renderer/universal.js` uses Solid 2's two-phase render effects directly for compiler helpers such as `effect`, `insert`, `spread`, and `ref`.
- `vendor/opentui-solid2/index.ts` exports OpenTUI-typed Solid 2 control-flow components: `For`, `Show`, `Switch`, and `Match`.
- Solid 2 callable context providers are used directly; fake `.Provider` aliases have been removed.
- App code imports Solid primitives from `solid-js` and OpenTUI JSX/control-flow helpers from `@opentui/solid`.
- `src/lib/solid-compat.ts` has been removed.

## Adapter Surface

The fork currently supports the surface `legit` uses:

- `render`
- `testRender`
- `useKeyboard`
- `useTerminalDimensions`
- compiler helpers: `createElement`, `createComponent`, `insert`, `insertNode`, `spread`, `setProp`, `mergeProps`, `effect`, `memo`, `ref`
- control flow: `For`, `Show`, `Switch`, `Match`
- JSX tags used by the app, including `box`, `text`, `span`, `scrollbox`, `a`, and the existing OpenTUI renderables in `src/elements/index.ts`

Deferred:

- `Dynamic`
- `Portal`
- runtime plugin support
- `time-to-first-draw`

## Important Notes

- Bun still copies the file dependency into `node_modules/@opentui/solid`; run `bun install` after editing `vendor/opentui-solid2` so tests use the updated fork.
- Solid 2 effect functions return cleanup callbacks. Do not return callback refs or previous values from renderer effect functions.
- Solid 2 `For` passes item accessors. App render callbacks should use `item()` rather than relying on Solid 1-style value arguments.
- Solid 2 effects split dependency tracking and side effects: prefer `createEffect(() => source(), (value) => { ... })`.
- OpenTUI JSX types are not the same as Solid's DOM JSX types, so control-flow components exported from `@opentui/solid` carry OpenTUI-specific JSX typings while delegating to Solid 2 runtime implementations.

## Verification

Known-good checks for this migration:

```bash
bun run check
bun test
```

The real TUI should also be smoke-tested in tmux:

```bash
SOCKET=${TMPDIR:-/tmp}/claude-tmux-sockets/claude.sock
tmux -S "$SOCKET" attach -t legit-test
```
