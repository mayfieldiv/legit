// Stub for @babel/core — pulled in transitively by @opentui/solid/preload
// (preload.ts → solid-plugin.ts → @babel/core). The upstream file is missing
// a @ts-expect-error on line 1. This has been the case since at least 0.1.87
// but only surfaces when a consumer imports @opentui/solid/preload, which
// causes tsgo/tsc to follow the relative import chain into the .ts source
// (skipLibCheck doesn't help since it's a .ts file, not .d.ts).
declare module "@babel/core";
