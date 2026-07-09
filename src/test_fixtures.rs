//! Shared builders for the GitHub domain objects the detail-view tests
//! construct over and over (`detail_items`, `detail_layout`/view snapshots,
//! and the `update` reducer tests) — one canonical shape per object; tests
//! needing a variation (bot author, custom path) use struct-update syntax on
//! the result — plus the [`bounded`] hang guard for tests that drive real
//! subprocesses.

use std::{sync::mpsc, thread, time::Duration};

use chrono::{DateTime, TimeZone, Utc};

use crate::github::types::{CheckRun, FullReviewThread, IssueComment, ReviewComment};

/// Run `f` on a worker thread, returning `None` if it doesn't finish within
/// `limit`, so a regression that reintroduces a hang fails the calling test
/// instead of wedging the suite. On that timeout the worker leaks by design —
/// it is stuck by definition, and joining it would trade the fast failure for
/// the very hang being guarded against. A panic in `f` is resumed on the
/// caller rather than misreported as a hang.
pub fn bounded<T: Send + 'static>(
    limit: Duration,
    f: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let _ = tx.send(f());
    });
    match rx.recv_timeout(limit) {
        Ok(value) => Some(value),
        Err(mpsc::RecvTimeoutError::Timeout) => None,
        // The sender dropped without sending: `f` panicked.
        Err(mpsc::RecvTimeoutError::Disconnected) => match handle.join() {
            Err(panic) => std::panic::resume_unwind(panic),
            Ok(()) => unreachable!("worker exited without sending or panicking"),
        },
    }
}

/// A fixed timestamp safely in the past relative to every test's `now`.
pub fn fixed_created_at() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap()
}

/// An untimed check run (no start/end, so no Check Duration). Vary the
/// `workflow_name` or timestamps via struct-update where a test needs it.
pub fn check(name: &str, status: &str, conclusion: Option<&str>) -> CheckRun {
    CheckRun {
        name: name.to_owned(),
        workflow_name: None,
        status: status.to_owned(),
        conclusion: conclusion.map(str::to_owned),
        started_at: None,
        completed_at: None,
    }
}

/// A completed check carrying both endpoints, so it has a Check Duration of
/// `seconds`. The wall-clock start is arbitrary; only the span matters.
pub fn timed_check(name: &str, conclusion: &str, seconds: i64) -> CheckRun {
    let started = fixed_created_at();
    CheckRun {
        name: name.to_owned(),
        workflow_name: None,
        status: "completed".to_owned(),
        conclusion: Some(conclusion.to_owned()),
        started_at: Some(started),
        completed_at: Some(started + chrono::Duration::seconds(seconds)),
    }
}

/// A human review comment with the canonical fixture URL
/// `https://example.test/r/<id>` (asserted verbatim by the `o`-key tests).
pub fn review_comment(id: &str, author: &str, body: &str) -> ReviewComment {
    ReviewComment {
        id: id.to_owned(),
        author: author.to_owned(),
        body: body.to_owned(),
        created_at: fixed_created_at(),
        url: format!("https://example.test/r/{id}"),
        is_bot: false,
    }
}

/// An unresolved-or-resolved thread at `src/lib.rs:12`; override `path`/`line`
/// via struct-update where the location matters.
pub fn thread(id: &str, is_resolved: bool, comments: Vec<ReviewComment>) -> FullReviewThread {
    FullReviewThread {
        id: id.to_owned(),
        is_resolved,
        path: "src/lib.rs".to_owned(),
        line: Some(12),
        comments,
    }
}

/// A human issue comment with the canonical fixture URL
/// `https://example.test/c/<id>`.
pub fn issue_comment(id: u64, author: &str, body: &str) -> IssueComment {
    IssueComment {
        id,
        author: author.to_owned(),
        body: body.to_owned(),
        created_at: fixed_created_at(),
        url: format!("https://example.test/c/{id}"),
        is_bot: false,
    }
}
