# TUI state follows the Elm Architecture

The Rust TUI uses an Elm-style architecture: one `Model` struct holds all app state; one `Msg` enum represents every event (terminal input, PR-arrived, refresh-tick, network-stats-changed, fetch-failed, etc.); `fn update(&mut Model, Msg) -> Vec<Cmd>` is pure and synchronous and is the only thing that mutates the model; `fn view(&Model, &mut Frame)` is the only thing that draws. The runtime owns the side-effect layer — it spawns each `Cmd` as a tokio task and tasks send Msgs back over a single `mpsc::Sender<Msg>`. The runtime's event loop is `select(crossterm events, msg_rx)`; on activity it drains all pending Msgs through `update`, then calls `view` exactly once, then loops. No reactive subscriptions, no `Arc<Mutex<App>>`, no fixed-FPS timer.

## Why

The previous implementation used SolidJS fine-grained reactivity over opentui. Fine-grained signals + a render loop that re-runs computations on dependency changes produced unpredictable CPU spikes and laggy frames. The Elm shape eliminates the entire category by structure: rendering only happens when something asks for it (a Msg), and computations live in either pure `update` or background tasks — never in the render path.

## Consequences

- `update` must stay synchronous and pure (only mutates the passed `&mut Model`). Anything that awaits or does IO is a `Cmd`.
- Errors are values: a failed `Cmd` produces a `Msg::*Failed { context, error }` rather than panicking. The Model holds the last-error state; the view surfaces it.
- Testing strategy follows: `update` is unit-tested with synthetic `Msg`s and asserted against `(Model, Vec<Cmd>)`; `view` is snapshot-tested via ratatui's `TestBackend`; the runtime / command dispatch layer is exercised through tmux integration.
- Adding a feature touches three places: a Msg variant, an `update` arm, and a view change. Sometimes a Cmd variant too. No fourth place for "register a subscription."
