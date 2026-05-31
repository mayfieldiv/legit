//! HTTP concurrency limiter shared across the whole GitHub transport. Caps the
//! number of simultaneously in-flight requests and exposes a live snapshot
//! (in-flight + waiting) plus a change-subscription so the runtime can turn
//! ticks into `Msg::NetworkStatsChanged`. Port of the TS `withConcurrencyLimit`
//! in `src/lib/concurrency.ts`.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};

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
/// slot immediately count as `waiting`.
pub struct NetworkLimiter {
    semaphore: Arc<Semaphore>,
    in_flight: AtomicUsize,
    waiting: AtomicUsize,
    stats_tx: watch::Sender<NetworkStats>,
}

/// RAII guard for one in-flight slot. Holds the semaphore permit and, on drop,
/// decrements the in-flight count and republishes the snapshot.
pub struct Permit {
    _permit: OwnedSemaphorePermit,
    limiter: Arc<NetworkLimiter>,
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.limiter.in_flight.fetch_sub(1, Ordering::SeqCst);
        self.limiter.publish();
    }
}

impl NetworkLimiter {
    pub fn new(max_concurrent: usize) -> Arc<Self> {
        let (stats_tx, _) = watch::channel(NetworkStats::default());
        Arc::new(Self {
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            in_flight: AtomicUsize::new(0),
            waiting: AtomicUsize::new(0),
            stats_tx,
        })
    }

    /// Acquire one slot, blocking on the semaphore when at capacity. The caller
    /// is counted as `waiting` while blocked and `in_flight` once the returned
    /// `Permit` is held; dropping the permit frees the slot.
    pub async fn acquire(self: &Arc<Self>) -> Permit {
        self.waiting.fetch_add(1, Ordering::SeqCst);
        self.publish();
        let permit = Arc::clone(&self.semaphore)
            .acquire_owned()
            .await
            .expect("network limiter semaphore is never closed");
        self.waiting.fetch_sub(1, Ordering::SeqCst);
        self.in_flight.fetch_add(1, Ordering::SeqCst);
        self.publish();
        Permit {
            _permit: permit,
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
    use super::{NetworkLimiter, NetworkStats};
    use std::sync::Arc;

    #[tokio::test]
    async fn new_limiter_reports_zero() {
        let limiter = NetworkLimiter::new(8);
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
        let limiter = NetworkLimiter::new(8);

        let permit = limiter.acquire().await;
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
        let limiter = NetworkLimiter::new(1);
        let held = limiter.acquire().await;

        // A second acquire can't get a slot; it parks on the semaphore and must
        // register as `waiting`.
        let blocked = Arc::clone(&limiter);
        let pending = tokio::spawn(async move { blocked.acquire().await });

        // Let the spawned task run far enough to register itself as waiting.
        // `#[tokio::test]` is single-threaded, so a yield hands it the runtime.
        while limiter.snapshot().waiting == 0 {
            tokio::task::yield_now().await;
        }
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
}
