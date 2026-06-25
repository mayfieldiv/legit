# AGENTS.md

- **Primary TUI manual-testing path in this environment: drive a `herdr` pane.** See `docs/agents/manual-testing.md` — launch the real TUI, read frames, send keys, capture colour. The pane lives in the user's herdr session (the `manual-test` tab) so they can watch. The `tmux` flow below is an alternative when herdr isn't available.
- For TUI testing, use the `tmux` skill with a repo-local socket at `.tmux/legit-test.sock` (gitignored). Deriving it from the repo root keeps the path identical no matter the `$TMPDIR` or the current directory, so the attach command you give the user is always the same. Session name: `legit-test`.
  ```bash
  SOCKET="$(git rev-parse --show-toplevel)/.tmux/legit-test.sock"
  mkdir -p "$(dirname "$SOCKET")"
  tmux -S "$SOCKET" new-session -d -s legit-test -x 200 -y 40
  # Target the session by name (not legit-test:0.0) so it works regardless of
  # the user's base-index/pane-base-index.
  tmux -S "$SOCKET" send-keys -t legit-test -- 'cd "$(git rev-parse --show-toplevel)" && cargo run' Enter
  ```
  Always tell the user how to attach, resolving the path so it is copy-pasteable: `tmux -S "$(git rev-parse --show-toplevel)/.tmux/legit-test.sock" attach -t legit-test`
- **Do not kill the tmux session.** Leave `legit-test` running so the user can stay attached. If the session already exists, reuse it — just capture the pane or send keys. When the user reports a bug, capture the current pane state to see what they see.
- **Wait for TUI output** instead of using `sleep`. Use the tmux skill's `wait-for-text.sh`:
  ```bash
  SOCKET="$(git rev-parse --show-toplevel)/.tmux/legit-test.sock"
  .agents/skills/tmux/scripts/wait-for-text.sh \
    -S "$SOCKET" -t legit-test -p "open PRs" -T 10
  ```
- Prefer testing the real TUI in `tmux`, not only snapshot tests.
- Start the dev TUI from the repo root with `cargo run`. To exercise an installed binary, `cargo install --path .` puts `legit` on `PATH`.
- `~/immybot` can be used as a read-only dataset for TUI testing. Do not modify anything in that repo.
- **Never post to GitHub without explicit user approval.** Do not create issues, file PRs, post PR comments, reply to review threads, or perform any write action on GitHub unless the user explicitly asks for it.

## Agent skills

### Manual TUI testing

Drive the real TUI in a `herdr` pane — launch, read frames, send keys, capture colour. See `docs/agents/manual-testing.md`.

### Issue tracker

GitHub Issues at `mayfieldiv/legit`, accessed via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context — `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.
