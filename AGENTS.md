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
- **Wait for TUI output** instead of using `sleep`. Poll for expected text:
    ```bash
    SOCKET=${TMPDIR:-/tmp}/claude-tmux-sockets/claude.sock
    # Wait for legit TUI to render (polls until "open PRs" appears or 10s timeout)
    for i in $(seq 1 20); do
      tmux -S "$SOCKET" capture-pane -p -J -t legit-test:0.0 -S -200 2>/dev/null | grep -q "open PRs" && break
      sleep 0.5
    done
    ```
    Note: the tmux skill's `wait-for-text.sh` does not support `-S` (isolated sockets), so use the inline pattern above.
- Prefer testing the real TUI in `tmux`, not only snapshot tests.
- `legit` is on the shell `PATH` and resolves to the current repo state; prefer running `legit` directly.
- `~/immybot` can be used as a read-only dataset for TUI testing. Do not modify anything in that repo.
- **Never post to GitHub without explicit user approval.** Do not create issues, file PRs, post PR comments, reply to review threads, or perform any write action on GitHub unless the user explicitly asks for it.
