# Changelog

## [Unreleased]

### Changed

- Extract PR data store from `App.tsx` into `src/lib/pr-store.ts` — all data-fetching orchestration (fetch, cache, abort, background loading, coalescing) now lives in a standalone `createPRStore` factory, testable without rendering the TUI (#21)
- Extract `makeCoalescer` into `src/lib/coalescer.ts` with safe double-flush behavior
- Rename tab refresh API to `refreshAllActive`; simplify loaders

### Added

- Unit tests for `pr-store` and `coalescer`
