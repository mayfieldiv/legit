//! Per-PR refresh Priority Queue.
//!
//! Orders queued refreshes by a smart-status-derived priority (lower number =
//! more urgent), FIFO within a tier. A `BinaryHeap` provides the ordering; a
//! `HashMap` keyed by `PrKey` is the authoritative phase/dedupe record, so a
//! re-enqueue upgrades an existing entry's priority (and unions its
//! include-files flag) rather than double-queuing the PR. Heap entries left
//! stale by such an upgrade — or by a PR being taken or completed — are dropped
//! lazily on `take_next` (the standard lazy-deletion pattern for a heap that
//! can't update a key in place).
//!
//! Pure: no IO, no async. `update` drives it — `enqueue` on `r`/`R`,
//! `take_next` while pumping up to the active cap, `complete` on
//! `Msg::RefreshComplete` — and the runtime turns each taken `QueueItem` into a
//! `Cmd::RefreshPr`. The transport's `Semaphore` still bounds HTTP concurrency
//! underneath; this queue bounds how many *PRs* refresh at once so the priority
//! ordering is observable rather than collapsed by dispatching everything at
//! once.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use crate::github::rest::PrKey;

/// Where one PR sits in the refresh lifecycle. Drives the list-row indicator
/// (`Model::refresh_phase_for`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshPhase {
    /// Waiting in the queue for an active slot to free up.
    Queued,
    /// Taken from the queue; its `Cmd::RefreshPr` fan-out is in flight.
    Refreshing,
}

/// A PR taken off the queue, handed back to `update` to turn into a refresh
/// command. `priority` is carried for completeness/inspection; the dispatcher
/// only needs `key` + `include_files`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueItem {
    pub key: PrKey,
    pub priority: u8,
    pub include_files: bool,
}

/// The authoritative state the queue keeps for each non-idle PR. One entry per
/// PR (the dedupe invariant): re-enqueuing rewrites this rather than adding a
/// second.
#[derive(Clone, Debug)]
struct EntryState {
    phase: RefreshPhase,
    priority: u8,
    include_files: bool,
    /// Insertion sequence of the PR's *live* heap entry. `take_next` accepts a
    /// popped heap entry only when its `seq` matches this, so a priority
    /// upgrade (which pushes a fresh heap entry with a new seq) invalidates the
    /// PR's previous heap entry without having to find and remove it.
    seq: u64,
}

/// One ordering element in the heap. Holds just enough to rank and validate;
/// the canonical state lives in `RefreshQueue::entries`.
#[derive(Clone, Debug, Eq, PartialEq)]
struct HeapEntry {
    key: PrKey,
    priority: u8,
    seq: u64,
}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // `BinaryHeap` is a max-heap, so invert both keys: the smallest
        // priority — and, within a tier, the smallest seq (FIFO) — compares
        // greatest and is popped first.
        other
            .priority
            .cmp(&self.priority)
            .then_with(|| other.seq.cmp(&self.seq))
    }
}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, Default)]
pub struct RefreshQueue {
    heap: BinaryHeap<HeapEntry>,
    entries: HashMap<PrKey, EntryState>,
    next_seq: u64,
    /// PRs currently `Refreshing` (the active count the pump bounds). Derivable
    /// from `entries` but tracked directly so the pump's hot check is O(1).
    active: usize,
    /// PRs completed since the queue was last fully idle — the count the
    /// drain-complete success message reports. Reset by `take_completed_run`.
    completed_in_run: usize,
}

impl RefreshQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueue — or upgrade — a refresh for `key`.
    ///
    /// - A PR already `Refreshing` is left alone: its in-flight run must settle
    ///   (and `complete` clear it) before it can re-queue, mirroring the TS
    ///   refresh-queue's "refreshing is a no-op" rule.
    /// - A PR already `Queued` keeps the stronger (lower) priority and the
    ///   union of the include-files flags. An enqueue that changes neither is a
    ///   no-op, so a repeated identical press doesn't churn the heap.
    /// - An idle PR is queued fresh.
    pub fn enqueue(&mut self, key: PrKey, priority: u8, include_files: bool) {
        let (next_priority, next_include) = match self.entries.get(&key) {
            Some(state) if state.phase == RefreshPhase::Refreshing => return,
            Some(state) => {
                let priority = state.priority.min(priority);
                let include_files = state.include_files || include_files;
                if priority == state.priority && include_files == state.include_files {
                    return;
                }
                (priority, include_files)
            }
            None => (priority, include_files),
        };
        let seq = self.next_seq;
        self.next_seq += 1;
        self.entries.insert(
            key.clone(),
            EntryState {
                phase: RefreshPhase::Queued,
                priority: next_priority,
                include_files: next_include,
                seq,
            },
        );
        self.heap.push(HeapEntry {
            key,
            priority: next_priority,
            seq,
        });
    }

    /// Pop the most-urgent queued PR, mark it `Refreshing`, and return it. Skips
    /// stale heap entries (superseded by an upgrade, or whose PR was already
    /// taken/completed). `None` when nothing is queued.
    pub fn take_next(&mut self) -> Option<QueueItem> {
        while let Some(entry) = self.heap.pop() {
            let state = match self.entries.get_mut(&entry.key) {
                Some(state) if state.phase == RefreshPhase::Queued && state.seq == entry.seq => {
                    state
                }
                // Stale heap entry: a newer enqueue replaced it, or the PR is
                // already refreshing/done. Drop it and look further down.
                _ => continue,
            };
            state.phase = RefreshPhase::Refreshing;
            self.active += 1;
            return Some(QueueItem {
                key: entry.key,
                priority: state.priority,
                include_files: state.include_files,
            });
        }
        None
    }

    /// Mark a PR's refresh finished, freeing its active slot and counting it
    /// toward the current drain. A no-op for a PR that isn't `Refreshing`, so a
    /// duplicate or stray `RefreshComplete` can't corrupt the counts.
    pub fn complete(&mut self, key: &PrKey) {
        if self.entries.get(key).map(|state| state.phase) == Some(RefreshPhase::Refreshing) {
            self.entries.remove(key);
            self.active = self.active.saturating_sub(1);
            self.completed_in_run += 1;
        }
    }

    /// The refresh phase of `key`, or `None` when the PR is idle. The list view
    /// reads this to draw the per-row indicator.
    pub fn phase_for(&self, key: &PrKey) -> Option<RefreshPhase> {
        self.entries.get(key).map(|state| state.phase)
    }

    /// Number of PRs currently `Refreshing`. The pump dispatches more while this
    /// is below the active cap.
    pub fn active_len(&self) -> usize {
        self.active
    }

    /// True when nothing is queued or refreshing — the queue has fully drained.
    pub fn is_idle(&self) -> bool {
        self.entries.is_empty()
    }

    /// Take and reset the count of PRs completed since the queue was last idle.
    /// Read once per drain (when `is_idle` first becomes true) to size the
    /// success message.
    pub fn take_completed_run(&mut self) -> usize {
        std::mem::take(&mut self.completed_in_run)
    }
}

#[cfg(test)]
mod tests {
    use super::{RefreshPhase, RefreshQueue};
    use crate::github::rest::PrKey;

    fn key(number: u64) -> PrKey {
        PrKey {
            repo_slug: "owner/repo".to_owned(),
            number,
        }
    }

    /// Drain the queue to a vec of PR numbers in pop order, marking each
    /// complete so the next pop isn't blocked by an active-slot cap (this
    /// helper ignores the cap; the pump in `update` enforces it).
    fn drain(queue: &mut RefreshQueue) -> Vec<u64> {
        let mut order = Vec::new();
        while let Some(item) = queue.take_next() {
            order.push(item.key.number);
            queue.complete(&item.key);
        }
        order
    }

    #[test]
    fn dequeues_in_priority_order() {
        let mut queue = RefreshQueue::new();
        queue.enqueue(key(1), 3, false);
        queue.enqueue(key(2), 1, false);
        queue.enqueue(key(3), 2, false);

        assert_eq!(drain(&mut queue), [2, 3, 1]);
    }

    #[test]
    fn ties_break_fifo_by_insertion_order() {
        let mut queue = RefreshQueue::new();
        queue.enqueue(key(10), 2, false);
        queue.enqueue(key(11), 2, false);
        queue.enqueue(key(12), 2, false);

        assert_eq!(
            drain(&mut queue),
            [10, 11, 12],
            "same-priority entries pop in the order they were enqueued"
        );
    }

    #[test]
    fn identical_re_enqueue_is_deduped() {
        let mut queue = RefreshQueue::new();
        queue.enqueue(key(1), 2, false);
        queue.enqueue(key(1), 2, false);
        queue.enqueue(key(1), 2, false);

        assert_eq!(
            drain(&mut queue),
            [1],
            "the same PR at the same priority queues exactly once"
        );
    }

    #[test]
    fn re_enqueue_upgrades_to_the_stronger_priority() {
        let mut queue = RefreshQueue::new();
        queue.enqueue(key(1), 4, false);
        queue.enqueue(key(2), 1, false);
        // PR #1 re-queued at a stronger (lower) priority should now lead.
        queue.enqueue(key(1), 0, false);

        assert_eq!(drain(&mut queue), [1, 2]);
    }

    #[test]
    fn re_enqueue_never_weakens_priority() {
        let mut queue = RefreshQueue::new();
        queue.enqueue(key(1), 1, false);
        queue.enqueue(key(2), 2, false);
        // A weaker (higher) re-enqueue of #1 must not push it behind #2.
        queue.enqueue(key(1), 4, false);

        assert_eq!(drain(&mut queue), [1, 2]);
    }

    #[test]
    fn re_enqueue_unions_include_files() {
        let mut queue = RefreshQueue::new();
        queue.enqueue(key(1), 2, false);
        queue.enqueue(key(1), 2, true);

        let item = queue.take_next().expect("one queued item");
        assert_eq!(item.key, key(1));
        assert!(
            item.include_files,
            "include-files unions across re-enqueues"
        );
    }

    #[test]
    fn take_next_marks_refreshing_and_complete_clears() {
        let mut queue = RefreshQueue::new();
        queue.enqueue(key(1), 0, true);
        assert_eq!(queue.phase_for(&key(1)), Some(RefreshPhase::Queued));

        let item = queue.take_next().expect("queued");
        assert_eq!(queue.phase_for(&key(1)), Some(RefreshPhase::Refreshing));
        assert_eq!(queue.active_len(), 1);
        assert!(!queue.is_idle());

        queue.complete(&item.key);
        assert_eq!(queue.phase_for(&key(1)), None);
        assert_eq!(queue.active_len(), 0);
        assert!(queue.is_idle());
    }

    #[test]
    fn refreshing_pr_is_not_re_enqueued() {
        let mut queue = RefreshQueue::new();
        queue.enqueue(key(1), 2, false);
        queue.take_next().expect("queued");

        // While #1 is refreshing, re-queuing it (even at priority 0) is a no-op.
        queue.enqueue(key(1), 0, true);
        assert_eq!(queue.phase_for(&key(1)), Some(RefreshPhase::Refreshing));
        assert!(
            queue.take_next().is_none(),
            "nothing new is queued while the PR is in flight"
        );
    }

    #[test]
    fn completed_run_counts_then_resets() {
        let mut queue = RefreshQueue::new();
        queue.enqueue(key(1), 1, false);
        queue.enqueue(key(2), 1, false);
        let a = queue.take_next().unwrap();
        let b = queue.take_next().unwrap();
        queue.complete(&a.key);
        queue.complete(&b.key);

        assert!(queue.is_idle());
        assert_eq!(queue.take_completed_run(), 2);
        assert_eq!(
            queue.take_completed_run(),
            0,
            "the run count resets after it is read"
        );
    }

    #[test]
    fn complete_on_a_non_refreshing_pr_is_a_noop() {
        let mut queue = RefreshQueue::new();
        queue.enqueue(key(1), 1, false);

        // #1 is only Queued, not Refreshing — completing it changes nothing.
        queue.complete(&key(1));
        assert_eq!(queue.phase_for(&key(1)), Some(RefreshPhase::Queued));
        assert_eq!(queue.active_len(), 0);
        assert_eq!(queue.take_completed_run(), 0);
    }
}
