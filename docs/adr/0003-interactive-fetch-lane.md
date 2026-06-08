# Network limiter has an interactive lane that borrows background capacity

The shared `NetworkLimiter` is a single fair (FIFO) semaphore of 8 slots, so a fetch the user is actively waiting on — the detail body on drill-in (`FetchPRDetail`), the selected PR's files (`FetchFiles`) — parks behind the entire list-wide enrichment fan-out and only loads once that backlog drains. We split the limiter into two priority lanes so interactive fetches never queue behind background work: a hard total cap of 16 in-flight, with background (the open-PR listing and the enrichment fan-out) sub-capped at 8. Interactive fetches draw only against the 16; since background can never hold more than 8, at least 8 slots are always free for interactive, so it is guaranteed 8 immediately and borrows up to all 16 when background is idle. Borrowing is asymmetric: interactive reaches into background's idle slots, but background stays hard-capped at 8 and cannot use interactive's headroom.

## Considered options

- **Priority queue-jump within one 8-slot pool** — interactive waiters park ahead of background waiters but still wait for an in-flight slot to free. Rejected: leaves up to one request-RTT of artificial latency on the exact action the user is blocked on.
- **Bypass the limiter for interactive** — no permit at all, truly instant. Rejected: `FetchFiles` fires per cursor move and is never cancelled, so holding an arrow key would fan out an _uncapped_ burst (the precise shape GitHub's abuse detection watches for), and bypassed requests wouldn't show in `NetworkStats`.
- **Static reserved lane (e.g. background 6 / interactive 2)** — permanently cuts background throughput even when no interactive fetch is in flight. Rejected: borrowing gives interactive the same guarantee without taxing initial enrichment, which keeps its full 8.

## Consequences

- The "8" was never a real GitHub ceiling — it was asserted by a comment citing a non-existent PRD (the TS reference used 10). GitHub's actual documented secondary-rate-limit ceiling is ~100 concurrent requests shared across REST + GraphQL, so a 16 total cap is comfortably safe. The `runtime.rs` constants and comment were rewritten to state this real reasoning.
- Fetch Priority is a static property of the `Cmd` variant (detail/files → interactive, listing/enrichment → background), so no model state is needed; `cmd::run` picks the lane per variant. This is orthogonal to the smart-status **Priority Queue**, which orders _background refreshes_ among themselves.
- Fast list navigation can momentarily drive interactive to 16 concurrent file-fetches and pause enrichment until the scroll settles; the fetches for scrolled-past PRs are wasted. This burst pre-dates this change (it was merely capped at 8 before). Cancelling/debouncing stale `FetchFiles` is a known follow-up, deliberately out of scope here.
