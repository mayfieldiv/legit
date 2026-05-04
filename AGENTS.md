# AGENTS.md

- For TUI testing, use the `tmux` skill with an isolated socket. Session name: `legit-test`.
  ```bash
  SOCKET_DIR=${TMPDIR:-/tmp}/claude-tmux-sockets
  mkdir -p "$SOCKET_DIR"
  SOCKET="$SOCKET_DIR/claude.sock"
  tmux -S "$SOCKET" new-session -d -s legit-test -x 200 -y 40
  tmux -S "$SOCKET" send-keys -t legit-test:0.0 -- 'cd /Users/mayfield/legit && legit' Enter
  ```
  Always tell the user how to attach: `tmux -S $TMPDIR/claude-tmux-sockets/claude.sock attach -t legit-test`
- **Do not kill the tmux session.** Leave `legit-test` running so the user can stay attached. If the session already exists, reuse it — just capture the pane or send keys. When the user reports a bug, capture the current pane state to see what they see.
- **Wait for TUI output** instead of using `sleep`. Use the tmux skill's `wait-for-text.sh`:
  ```bash
  SOCKET=${TMPDIR:-/tmp}/claude-tmux-sockets/claude.sock
  .agents/skills/tmux/scripts/wait-for-text.sh \
    -S "$SOCKET" -t legit-test:0.0 -p "open PRs" -T 10
  ```
- Prefer testing the real TUI in `tmux`, not only snapshot tests.
- `legit` is on the shell `PATH` and resolves to the current repo state; prefer running `legit` directly.
- `~/immybot` can be used as a read-only dataset for TUI testing. Do not modify anything in that repo.
- **Never post to GitHub without explicit user approval.** Do not create issues, file PRs, post PR comments, reply to review threads, or perform any write action on GitHub unless the user explicitly asks for it.

## Agent skills

### Issue tracker

GitHub Issues at `mayfieldiv/legit`, accessed via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context — `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.
