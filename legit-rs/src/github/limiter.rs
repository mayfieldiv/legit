//! HTTP concurrency limiter shared across the whole GitHub transport. Caps the
//! number of simultaneously in-flight requests and exposes a live snapshot
//! (in-flight + waiting) plus a change-subscription so the runtime can turn
//! ticks into `Msg::NetworkStatsChanged`.
//!
//! Requests run in one of two lanes (see ADR 0003). A `total` semaphore is the
//! hard ceiling on all in-flight requests; a smaller `background` sub-cap
//! bounds speculative, list-wide work (the open-PR listing and the enrichment
//! fan-out). `Interactive` requests — the ones the user is actively waiting on,
//! the detail body and the selected PR's files — draw only against `total`.
//! Because `Background` can never hold more than its sub-cap, at least
//! `total - background` slots are always free for the interactive lane, so it
//! is guaranteed that many immediately and borrows up to the full `total` when
//! background is idle. Borrowing is asymmetric: interactive reaches into
//! background's idle slots, but background stays hard-capped and cannot use
//! interactive's headroom.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};

/// Which lane a request takes through the limiter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    /// A fetch the user is actively waiting on (detail body, selected files).
    /// Draws straight from the `total` pool, so it never queues behind the
    /// background sub-cap.
    Interactive,
    /// Speculative, list-wide work (open-PR listing, enrichment fan-out). Holds
    /// a `background` sub-cap permit in addition to a `total` slot.
    Background,
}

/// Live view of the transport's HTTP concurrency.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetworkStats {
    /// Requests currently executing (a permit is held).
    pub in_flight: usize,
    /// Requests blocked on the semaphore (awaiting a permit).
    pub waiting: usize,
}

/// A `Semaphore`-backed limiter. Every HTTP request acquires a `Permit` first;
/// the permit reports as `in_flight` until dropped. Requests that can't get a
/// slot immediately count as `waiting`. See the module docs for the two-lane
/// (`total` + `background` sub-cap) design.
pub struct NetworkLimiter {
    total: Arc<Semaphore>,
    background: Arc<Semaphore>,
    in_flight: AtomicUsize,
    waiting: AtomicUsize,
    stats_tx: watch::Sender<NetworkStats>,
}

/// RAII guard for one in-flight slot. Holds the `total` permit — and, for a
/// `Background` request, the `background` sub-cap permit too — and on drop
/// decrements the in-flight count and republishes the snapshot.
pub struct Permit {
    _total: OwnedSemaphorePermit,
    _background: Option<OwnedSemaphorePermit>,
    limiter: Arc<NetworkLimiter>,
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.limiter.in_flight.fetch_sub(1, Ordering::SeqCst);
        self.limiter.publish();
    }
}

/// Decrements `waiting` (and republishes) on drop. Guards the gap between
/// registering as `waiting` and obtaining the semaphore permit so a cancelled
/// `acquire` future — dropped while parked on the `.await` — can't leak the
/// count and leave `NetworkStats.waiting` stuck above zero.
struct WaitingGuard<'a> {
    limiter: &'a Arc<NetworkLimiter>,
}

impl Drop for WaitingGuard<'_> {
    fn drop(&mut self) {
        self.limiter.waiting.fetch_sub(1, Ordering::SeqCst);
        self.limiter.publish();
    }
}

impl NetworkLimiter {
    /// `total_max` is the hard ceiling on all in-flight requests; `background_max`
    /// is the sub-cap for the `Background` lane (which must not exceed `total_max`).
    pub fn new(total_max: usize, background_max: usize) -> Arc<Self> {
        debug_assert!(
            background_max <= total_max,
            "background sub-cap ({background_max}) cannot exceed the total cap ({total_max})"
        );
        let (stats_tx, _) = watch::channel(NetworkStats::default());
        Arc::new(Self {
            total: Arc::new(Semaphore::new(total_max)),
            background: Arc::new(Semaphore::new(background_max)),
            in_flight: AtomicUsize::new(0),
            waiting: AtomicUsize::new(0),
            stats_tx,
        })
    }

    /// Acquire one slot in `lane`, blocking when at capacity. The caller is
    /// counted as `waiting` while blocked and `in_flight` once the returned
    /// `Permit` is held; dropping the permit frees the slot(s).
    pub async fn acquire(self: &Arc<Self>, lane: Lane) -> Permit {
        self.waiting.fetch_add(1, Ordering::SeqCst);
        self.publish();
        let (total, background) = {
            // Hold the decrement in a drop guard so cancelling this future while
            // parked on either semaphore still releases the `waiting` count. The
            // guard drops at the end of this block — once the permits are in hand.
            let _waiting = WaitingGuard { limiter: self };
            // Background takes its sub-cap permit FIRST, so it never occupies one
            // of the shared `total` slots while still waiting to run — that
            // headroom stays reserved for the interactive lane. Interactive skips
            // the sub-cap and draws straight from `total`. The two semaphores are
            // always acquired in this order (sub-cap then total), so there is no
            // circular wait and no deadlock.
            let background = match lane {
                Lane::Background => Some(
                    Arc::clone(&self.background)
                        .acquire_owned()
                        .await
                        .expect("background sub-cap semaphore is never closed"),
                ),
                Lane::Interactive => None,
            };
            let total = Arc::clone(&self.total)
                .acquire_owned()
                .await
                .expect("network limiter semaphore is never closed");
            // Past the awaits, so no more cancellation points. Bump in_flight
            // before the guard drops so its republish reflects the final
            // {in_flight, waiting} state in a single tick.
            self.in_flight.fetch_add(1, Ordering::SeqCst);
            (total, background)
        };
        Permit {
            _total: total,
            _background: background,
            limiter: Arc::clone(self),
        }
    }

    pub fn snapshot(&self) -> NetworkStats {
        NetworkStats {
            in_flight: self.in_flight.load(Ordering::SeqCst),
            waiting: self.waiting.load(Ordering::SeqCst),
        }
    }

    /// Receiver that yields a fresh `NetworkStats` every time the counts change.
    pub fn subscribe(&self) -> watch::Receiver<NetworkStats> {
        self.stats_tx.subscribe()
    }

    fn publish(&self) {
        // A closed receiver set is not an error — the runtime may not have a
        // subscriber yet (or ever, in tests).
        let _ = self.stats_tx.send(self.snapshot());
    }
}

#[cfg(test)]
mod tests {
    use super::{Lane, NetworkLimiter, NetworkStats};
    use std::sync::Arc;

    /// Spin the single-threaded runtime until `cond` holds. `#[tokio::test]` is
    /// single-threaded, so yielding hands a spawned task the runtime far enough
    /// to register itself.
    async fn spin_until(limiter: &Arc<NetworkLimiter>, cond: impl Fn(NetworkStats) -> bool) {
        while !cond(limiter.snapshot()) {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn new_limiter_reports_zero() {
        let limiter = NetworkLimiter::new(16, 8);
        assert_eq!(
            limiter.snapshot(),
            NetworkStats {
                in_flight: 0,
                waiting: 0
            }
        );
    }

    #[tokio::test]
    async fn acquiring_a_permit_marks_one_in_flight_until_dropped() {
        let limiter = NetworkLimiter::new(16, 8);

        let permit = limiter.acquire(Lane::Background).await;
        assert_eq!(
            limiter.snapshot(),
            NetworkStats {
                in_flight: 1,
                waiting: 0
            }
        );

        drop(permit);
        assert_eq!(
            limiter.snapshot(),
            NetworkStats {
                in_flight: 0,
                waiting: 0
            }
        );
    }

    #[tokio::test]
    async fn second_acquire_at_capacity_counts_as_waiting() {
        let limiter = NetworkLimiter::new(1, 1);
        let held = limiter.acquire(Lane::Background).await;

        // A second acquire can't get a slot; it parks on the semaphore and must
        // register as `waiting`.
        let blocked = Arc::clone(&limiter);
        let pending = tokio::spawn(async move { blocked.acquire(Lane::Background).await });

        spin_until(&limiter, |s| s.waiting == 1).await;
        assert_eq!(
            limiter.snapshot(),
            NetworkStats {
                in_flight: 1,
                waiting: 1
            }
        );

        // Free the slot; the parked acquire now proceeds and flips waiting→in-flight.
        drop(held);
        let resumed = pending.await.expect("pending acquire task");
        assert_eq!(
            limiter.snapshot(),
            NetworkStats {
                in_flight: 1,
                waiting: 0
            }
        );
        drop(resumed);
    }

    #[tokio::test]
    async fn cancelled_acquire_does_not_leak_waiting() {
        let limiter = NetworkLimiter::new(1, 1);
        // Fill the only slot so the next acquire must park as `waiting`.
        let held = limiter.acquire(Lane::Background).await;

        let blocked = Arc::clone(&limiter);
        let pending = tokio::spawn(async move { blocked.acquire(Lane::Background).await });
        spin_until(&limiter, |s| s.waiting == 1).await;

        // Cancel the parked acquire: its future is dropped mid-await. Without a
        // drop guard the `waiting` increment would leak forever (this loop would
        // never terminate); with it, the count returns to zero.
        pending.abort();
        let _ = pending.await;
        spin_until(&limiter, |s| s.waiting == 0).await;
        assert_eq!(
            limiter.snapshot(),
            NetworkStats {
                in_flight: 1,
                waiting: 0
            }
        );
        drop(held);
    }

    #[tokio::test]
    async fn interactive_borrows_idle_background_slots() {
        // Background sub-cap is 2, but with no background work the interactive
        // lane can fill all 4 total slots — it draws only against `total`.
        let limiter = NetworkLimiter::new(4, 2);
        let mut held = Vec::new();
        for _ in 0..4 {
            held.push(limiter.acquire(Lane::Interactive).await);
        }
        assert_eq!(limiter.snapshot().in_flight, 4);

        // The 5th interactive request hits the total cap and parks as waiting.
        let blocked = Arc::clone(&limiter);
        let pending = tokio::spawn(async move { blocked.acquire(Lane::Interactive).await });
        spin_until(&limiter, |s| s.waiting == 1).await;
        assert_eq!(
            limiter.snapshot(),
            NetworkStats {
                in_flight: 4,
                waiting: 1
            }
        );

        // Freeing one interactive slot lets the parked request through.
        held.pop();
        let resumed = pending.await.expect("pending acquire task");
        assert_eq!(limiter.snapshot().in_flight, 4);
        drop(resumed);
        drop(held);
    }

    #[tokio::test]
    async fn background_capped_at_subcap_while_interactive_uses_the_rest() {
        // total 4, background sub-cap 2.
        let limiter = NetworkLimiter::new(4, 2);
        let bg = vec![
            limiter.acquire(Lane::Background).await,
            limiter.acquire(Lane::Background).await,
        ];
        assert_eq!(limiter.snapshot().in_flight, 2);

        // A 3rd background request parks on the sub-cap even though 2 total slots
        // are free — background can never exceed its sub-cap.
        let blocked = Arc::clone(&limiter);
        let pending = tokio::spawn(async move { blocked.acquire(Lane::Background).await });
        spin_until(&limiter, |s| s.waiting == 1).await;
        assert_eq!(
            limiter.snapshot(),
            NetworkStats {
                in_flight: 2,
                waiting: 1
            }
        );

        // The interactive lane can still take both remaining total slots
        // immediately — it isn't blocked by the saturated background sub-cap.
        let i1 = limiter.acquire(Lane::Interactive).await;
        let i2 = limiter.acquire(Lane::Interactive).await;
        assert_eq!(limiter.snapshot().in_flight, 4);

        // Releasing a background slot lets the parked 3rd background through.
        drop(bg);
        let resumed = pending.await.expect("pending acquire task");
        drop(resumed);
        drop(i1);
        drop(i2);
    }
}
