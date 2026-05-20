# Rebuild the TUI in Rust + ratatui

The current TUI is built on SolidJS + opentui and suffers persistent high CPU and laggy rendering — fine-grained reactivity over a terminal renderer ends up doing too much work for too little visible change. We are rewriting the TUI in Rust on ratatui, in a new `legit-rs/` subdirectory; the existing TypeScript code stays as a reference implementation until parity is reached and is then deleted. The new binary is named `legit-rs` for the transition, and will be renamed to `legit` at cutover.

## Considered Options

- **Keep SolidJS + opentui, optimise** — rejected: we've tuned reactivity, batched stream updates, memoised selectively, and CPU is still bad. The problem is structural, not a missed optimisation.
- **Bubbletea (Go)** — viable but ratatui has the richer widget ecosystem and better text rendering primitives for our markdown body, threads, and tab UI.
- **Textual (Python)** — viable but adds an interpreter dependency to a tool meant to be a fast single binary.

## Consequences

- One source-of-truth language for new feature work shifts to Rust mid-rewrite.
- The blocker engine, file categoriser, github transport, and worktree helpers all need ports; their TS implementations remain readable until cutover.
- No backwards compatibility constraints — there are no external users of the TS implementation.
