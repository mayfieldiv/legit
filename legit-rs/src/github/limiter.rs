//! HTTP concurrency limiter shared across the whole GitHub transport. Caps the
//! number of simultaneously in-flight requests and exposes a live snapshot
//! (in-flight + waiting) plus a change-subscription so the runtime can turn
//! ticks into `Msg::NetworkStatsChanged`.
//!
//! Unlike a plain semaphore, the limiter holds pending requests as **data** in
//! an explicit queue and a pump grants slots from it (see ADR 0003). That lets a
//! request be re-ranked while it is still waiting — the whole point of focus
//! promotion below.
//!
//! Two lanes share one hard ceiling. A `total` cap bounds all in-flight
//! requests; a smaller `background` sub-cap bounds speculative, list-wide work
//! (the open-PR listing and the enrichment fan-out). A request's **effective
//! lane** is computed by the pump, not baked in at dispatch:
//!
//! - `Interactive` if it asked for the interactive lane outright (the detail
//!   body, the selected PR's files) **or** it is background work for the PR the
//!   user is currently focused on (the detail PR, else the selected list PR).
//! - `Background` otherwise.
//!
//! The pump grants interactive-effective waiters up to `total`, and
//! background-effective waiters only while the sub-cap has room. Because
//! background in-flight can never exceed its sub-cap, at least
//! `total - background` slots are always free for interactive work, so it is
//! guaranteed that many immediately and borrows up to the full `total` when
//! background is idle. Borrowing is asymmetric: interactive reaches into
//! background's idle slots, background never the reverse.
//!
//! **Focus promotion.** `set_focus` re-ranks the queue: a pending request for
//! the newly-focused PR becomes interactive-effective, so one the background
//! sub-cap was holding back is granted at once (using a borrowed slot). Moving
//! focus away re-ranks it back with no extra bookkeeping. Only *pending*
//! requests promote — an in-flight one already holds its slot.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard},
};

use tokio::sync::{oneshot, watch};

use crate::github::rest::PrKey;

/// The lane a request asks for at dispatch. The pump may treat a `Background`
/// request as interactive-effective while its PR is focused; it never
/// downgrades an `Interactive` one. Only `Background` carries a PR key — that
/// makes "only background work is promotable" structural rather than a rule the
/// pump has to remember.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lane {
    /// A fetch the user is actively waiting on regardless of focus (detail body,
    /// selected files). Always interactive-effective.
    Interactive,
    /// Speculative, list-wide work (open-PR listing, enrichment fan-out).
    /// `pr` is the PR it belongs to — interactive-effective while that PR is
    /// focused — or `None` for repo-wide work (the listing, the batched
    /// review-status query), which can never be focused.
    Background { pr: Option<PrKey> },
}

/// Live view of the transport's HTTP concurrency.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetworkStats {
    /// Requests currently executing (a slot is committed).
    pub in_flight: usize,
    /// Requests queued but not yet granted a slot.
    pub waiting: usize,
}

/// A request parked in the queue until the pump grants it. `tx` delivers the
/// `Permit` once a slot is committed to it.
struct Waiter {
    /// Monotonic insertion id. `push_back` keeps the queue ordered by it, so the
    /// pump scans in FIFO order without re-reading it; the id is used only by
    /// `AcquireGuard::drop` to locate and remove a cancelled waiter.
    id: u64,
    lane: Lane,
    tx: oneshot::Sender<Permit>,
}

/// All mutable limiter state, behind one mutex. Critical sections are short and
/// fully synchronous — no `.await` is ever held across the lock.
struct Inner {
    total_max: usize,
    background_max: usize,
    total_in_flight: usize,
    background_in_flight: usize,
    /// The PR the user is focused on, whose pending requests rank as interactive.
    focused: Option<PrKey>,
    next_id: u64,
    queue: VecDeque<Waiter>,
}

impl Inner {
    fn snapshot(&self) -> NetworkStats {
        NetworkStats {
            in_flight: self.total_in_flight,
            waiting: self.queue.len(),
        }
    }

    /// A waiter is interactive-effective when it asked for the interactive lane
    /// outright, or when its PR is the focused one.
    fn is_interactive(&self, waiter: &Waiter) -> bool {
        match &waiter.lane {
            Lane::Interactive => true,
            Lane::Background { pr } => pr
                .as_ref()
                .is_some_and(|pr| self.focused.as_ref() == Some(pr)),
        }
    }

    /// Index of the next waiter to grant. The queue is FIFO by insertion id
    /// (`push_back` of a monotonic id; removals keep order), so the first
    /// interactive-effective waiter is already the oldest one and nothing later
    /// can beat it. Absent any interactive waiter, the first background-effective
    /// waiter wins, and only while the background sub-cap has room. `None` when
    /// nothing can be granted now.
    fn pick_next(&self) -> Option<usize> {
        let mut background = None;
        for (index, waiter) in self.queue.iter().enumerate() {
            if self.is_interactive(waiter) {
                return Some(index);
            }
            if background.is_none() && self.background_in_flight < self.background_max {
                background = Some(index);
            }
        }
        background
    }
}

/// A `priority-queue`-backed limiter. Every HTTP request `acquire`s a `Permit`
/// first; the permit reports as `in_flight` until dropped. See the module docs
/// for the two-lane + focus-promotion design.
pub struct NetworkLimiter {
    inner: Mutex<Inner>,
    stats_tx: watch::Sender<NetworkStats>,
}

/// RAII guard for one committed slot. On drop it frees the slot, pumps the queue
/// (a freed slot may unblock a waiter), and republishes the snapshot.
pub struct Permit {
    limiter: Arc<NetworkLimiter>,
    /// Whether this permit counts against the background sub-cap.
    background: bool,
}

impl Drop for Permit {
    fn drop(&mut self) {
        let mut inner = self.limiter.inner.lock().unwrap();
        inner.total_in_flight -= 1;
        if self.background {
            inner.background_in_flight -= 1;
        }
        self.limiter.pump_and_publish(inner);
    }
}

/// Removes a still-queued waiter if the `acquire` future is dropped before its
/// grant arrives, so a cancelled acquire neither leaks a `waiting` count nor is
/// later handed a slot nobody holds. Disarmed once the permit is in hand.
struct AcquireGuard<'a> {
    limiter: &'a Arc<NetworkLimiter>,
    id: u64,
    armed: bool,
}

impl AcquireGuard<'_> {
    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for AcquireGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut inner = self.limiter.inner.lock().unwrap();
        // If the waiter is gone it was already granted; the buffered permit in
        // the (now-dropped) receiver releases its own slot via `Permit::drop`.
        if let Some(pos) = inner.queue.iter().position(|w| w.id == self.id) {
            inner.queue.remove(pos);
            let stats = inner.snapshot();
            drop(inner);
            self.limiter.publish(stats);
        }
    }
}

impl NetworkLimiter {
    /// `total_max` is the hard ceiling on all in-flight requests; `background_max`
    /// is the sub-cap for background-effective requests (must not exceed `total_max`).
    pub fn new(total_max: usize, background_max: usize) -> Arc<Self> {
        debug_assert!(
            background_max <= total_max,
            "background sub-cap ({background_max}) cannot exceed the total cap ({total_max})"
        );
        let (stats_tx, _) = watch::channel(NetworkStats::default());
        Arc::new(Self {
            inner: Mutex::new(Inner {
                total_max,
                background_max,
                total_in_flight: 0,
                background_in_flight: 0,
                focused: None,
                next_id: 0,
                queue: VecDeque::new(),
            }),
            stats_tx,
        })
    }

    /// Queue a request in `lane`, blocking until the pump grants it a slot. The
    /// caller is counted as `waiting` until then and `in_flight` once the
    /// returned `Permit` is held; dropping the permit frees the slot.
    pub async fn acquire(self: &Arc<Self>, lane: Lane) -> Permit {
        let (tx, rx) = oneshot::channel();
        let id = {
            let mut inner = self.inner.lock().unwrap();
            let id = inner.next_id;
            inner.next_id += 1;
            inner.queue.push_back(Waiter { id, lane, tx });
            // The pump may grant this very waiter immediately; the permit is
            // buffered in `rx` and picked up by the await below.
            self.pump_and_publish(inner);
            id
        };
        let guard = AcquireGuard {
            limiter: self,
            id,
            armed: true,
        };
        let permit = rx
            .await
            .expect("limiter never drops a queued waiter without granting it a permit");
        guard.disarm();
        permit
    }

    /// Set the PR the user is focused on (the open detail PR, else the selected
    /// list PR). Pending requests for it are re-ranked into the interactive lane;
    /// any the background sub-cap was holding back are granted at once. A no-op
    /// when focus is unchanged.
    pub fn set_focus(self: &Arc<Self>, focused: Option<PrKey>) {
        let mut inner = self.inner.lock().unwrap();
        if inner.focused == focused {
            return;
        }
        inner.focused = focused;
        self.pump_and_publish(inner);
    }

    /// A point-in-time snapshot of the counts. Production code observes changes
    /// through `subscribe`; this direct read exists for tests asserting state.
    #[cfg(test)]
    pub fn snapshot(&self) -> NetworkStats {
        self.inner.lock().unwrap().snapshot()
    }

    /// Receiver that yields a fresh `NetworkStats` every time the counts change.
    pub fn subscribe(&self) -> watch::Receiver<NetworkStats> {
        self.stats_tx.subscribe()
    }

    /// Grant as many waiters as capacity allows, then publish the snapshot. The
    /// grants are *delivered after the lock is released* so a send to a cancelled
    /// receiver (which drops the permit and re-enters the limiter to free its
    /// slot) can't deadlock against the lock we hold here.
    fn pump_and_publish(self: &Arc<Self>, mut inner: MutexGuard<'_, Inner>) {
        let grants = self.drain_grants(&mut inner);
        let stats = inner.snapshot();
        drop(inner);
        for (tx, permit) in grants {
            // `Err` means the receiver was cancelled; dropping the returned permit
            // here frees its slot via `Permit::drop` (no lock is held now).
            let _ = tx.send(permit);
        }
        self.publish(stats);
    }

    /// Pull grantable waiters out of the queue, committing a slot to each. Caller
    /// delivers the returned permits after unlocking.
    fn drain_grants(self: &Arc<Self>, inner: &mut Inner) -> Vec<(oneshot::Sender<Permit>, Permit)> {
        let mut grants = Vec::new();
        while inner.total_in_flight < inner.total_max {
            let Some(index) = inner.pick_next() else {
                break;
            };
            let waiter = inner
                .queue
                .remove(index)
                .expect("pick_next returned a valid queue index");
            let background = !inner.is_interactive(&waiter);
            inner.total_in_flight += 1;
            if background {
                inner.background_in_flight += 1;
            }
            grants.push((
                waiter.tx,
                Permit {
                    limiter: Arc::clone(self),
                    background,
                },
            ));
        }
        grants
    }

    fn publish(&self, stats: NetworkStats) {
        // A closed receiver set is not an error — the runtime may not have a
        // subscriber yet (or ever, in tests).
        let _ = self.stats_tx.send(stats);
    }
}

#[cfg(test)]
mod tests {
    use super::{Lane, NetworkLimiter, NetworkStats};
    use crate::github::rest::PrKey;
    use std::sync::Arc;

    fn key(number: u64) -> PrKey {
        PrKey {
            repo_slug: "owner/repo".to_owned(),
            number,
        }
    }

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

        let permit = limiter.acquire(Lane::Background { pr: None }).await;
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
        let held = limiter.acquire(Lane::Background { pr: None }).await;

        // A second acquire can't get a slot; it parks in the queue as `waiting`.
        let blocked = Arc::clone(&limiter);
        let pending =
            tokio::spawn(async move { blocked.acquire(Lane::Background { pr: None }).await });

        spin_until(&limiter, |s| s.waiting == 1).await;
        assert_eq!(
            limiter.snapshot(),
            NetworkStats {
                in_flight: 1,
                waiting: 1
            }
        );

        // Free the slot; the parked acquire is granted and flips waiting→in-flight.
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
        // Fill the only slot so the next acquire must queue as `waiting`.
        let held = limiter.acquire(Lane::Background { pr: None }).await;

        let blocked = Arc::clone(&limiter);
        let pending =
            tokio::spawn(async move { blocked.acquire(Lane::Background { pr: None }).await });
        spin_until(&limiter, |s| s.waiting == 1).await;

        // Cancel the queued acquire: its future is dropped before the grant. The
        // guard must drop the dead waiter so `waiting` returns to zero (otherwise
        // this loop never terminates).
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
        // lane can fill all 4 total slots — it draws against `total` alone.
        let limiter = NetworkLimiter::new(4, 2);
        let mut held = Vec::new();
        for _ in 0..4 {
            held.push(limiter.acquire(Lane::Interactive).await);
        }
        assert_eq!(limiter.snapshot().in_flight, 4);

        // The 5th interactive request hits the total cap and queues.
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

        // Freeing one slot lets the queued request through.
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
            limiter.acquire(Lane::Background { pr: Some(key(1)) }).await,
            limiter.acquire(Lane::Background { pr: Some(key(2)) }).await,
        ];
        assert_eq!(limiter.snapshot().in_flight, 2);

        // A 3rd background request queues even though 2 total slots are free —
        // background can never exceed its sub-cap.
        let blocked = Arc::clone(&limiter);
        let pending =
            tokio::spawn(
                async move { blocked.acquire(Lane::Background { pr: Some(key(3)) }).await },
            );
        spin_until(&limiter, |s| s.waiting == 1).await;
        assert_eq!(
            limiter.snapshot(),
            NetworkStats {
                in_flight: 2,
                waiting: 1
            }
        );

        // The interactive lane still takes both remaining total slots at once —
        // it isn't blocked by the saturated background sub-cap.
        let i1 = limiter.acquire(Lane::Interactive).await;
        let i2 = limiter.acquire(Lane::Interactive).await;
        assert_eq!(limiter.snapshot().in_flight, 4);

        // Releasing a background slot lets the queued 3rd background through.
        drop(bg);
        let resumed = pending.await.expect("pending acquire task");
        drop(resumed);
        drop(i1);
        drop(i2);
    }

    #[tokio::test]
    async fn focus_promotes_a_pending_background_request() {
        // total 2, background sub-cap 1: one slot is reserved for interactive work.
        let limiter = NetworkLimiter::new(2, 1);
        // Fill the sole background slot.
        let held = limiter.acquire(Lane::Background { pr: Some(key(1)) }).await;

        // A second background request for PR #2 can't run: the sub-cap is full,
        // so it queues even though one total slot is free.
        let blocked = Arc::clone(&limiter);
        let pending =
            tokio::spawn(
                async move { blocked.acquire(Lane::Background { pr: Some(key(2)) }).await },
            );
        spin_until(&limiter, |s| s.waiting == 1).await;
        assert_eq!(
            limiter.snapshot(),
            NetworkStats {
                in_flight: 1,
                waiting: 1
            }
        );

        // Focus PR #2: its queued request is promoted to interactive-effective,
        // which ignores the background sub-cap and grabs the free total slot.
        limiter.set_focus(Some(key(2)));
        let resumed = pending.await.expect("pending acquire task");
        assert_eq!(
            limiter.snapshot(),
            NetworkStats {
                in_flight: 2,
                waiting: 0
            }
        );
        drop(resumed);
        drop(held);
    }
}
