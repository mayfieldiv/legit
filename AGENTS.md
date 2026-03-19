# AGENTS.md

- For TUI testing, prefer a long-lived `tmux` session named `legit-test` so the user can watch along.
- Prefer testing the real TUI in `tmux`, not only snapshot tests.
- `legit` is on the shell `PATH` and resolves to the current repo state; prefer running `legit` directly.
- `~/immybot` can be used as a read-only dataset for TUI testing. Do not modify anything in that repo.
