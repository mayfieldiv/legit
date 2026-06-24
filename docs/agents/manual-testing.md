# Manual TUI testing with herdr

`herdr` is this environment's terminal workspace manager (`~/.local/bin/herdr`,
a background server reached over a socket). Use it to launch the real `legit`
TUI in a pane, read rendered frames, send keystrokes, and capture colour — so
changes are verified against a live render, not only snapshot tests.

The pane lives in the user's own herdr session, so they can watch by switching
to the `manual-test` tab.

## One-time: a reusable test pane

```bash
herdr tab create --workspace w3 --cwd "$(git rev-parse --show-toplevel)" \
  --label manual-test --no-focus
# -> result.root_pane.pane_id, e.g. w3:p7
```

Reuse it across runs. If it's gone (the server was restarted), list tabs with
`herdr tab list --workspace w3` and recreate. `w3` is the legit workspace; adjust
if the repo is open elsewhere (`herdr pane list` shows each pane's `cwd`).

## The loop

```bash
PANE=w3:p7

# Run a command (sends the text + Enter; prints nothing on success)
herdr pane run "$PANE" './target/debug/legit'      # or: cargo run

# Block until the first frame renders — wait, don't sleep
herdr wait output "$PANE" --match "open PRs" --source visible --timeout 20000

# Read the rendered screen
herdr pane read "$PANE" --source visible --lines 40

# Capture colour (RGB SGR codes) to verify styling
herdr pane read "$PANE" --format ansi --source visible --lines 40

# Drive the TUI (list view: j/k nav, h/l repo tabs, / filter, Enter open, q quit)
herdr pane send-keys "$PANE" l l l l    # switch repo tab
herdr pane send-keys "$PANE" j j j j    # move selection
```

## Gotchas

- Use `--source visible` for reads/waits. `--source recent` came back empty.
- `--format ansi` splits each styled run into its own escape-wrapped span, so a
  string spanning two styles is **not** a contiguous `grep` match — grep one
  span's text (e.g. `Analyze (csharp)`, not `CodeQL / Analyze`).
- `herdr pane run` writes command text + Enter; `herdr agent send` writes literal
  text with no Enter.

## Conventions

- **Leave the pane running.** Quit the app with `q` to return to a shell prompt;
  don't close the `manual-test` tab — the user stays attached to it.
- **Wait for output, never `sleep`** — use `herdr wait output`.
- Prefer testing the real TUI here, not only snapshot tests.
- `~/immybot` is a read-only dataset for TUI testing. Do not modify it.
- **Never post to GitHub without explicit user approval** (no issues, PRs, or
  review-thread writes).
