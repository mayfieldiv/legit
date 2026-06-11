//! Shared builders for the GitHub domain objects the detail-view tests
//! construct over and over (`detail_items`, `detail_layout`/view snapshots,
//! and the `update` reducer tests). One canonical shape per object; tests
//! needing a variation (bot author, custom path) use struct-update syntax on
//! the result.

use chrono::{DateTime, TimeZone, Utc};

use crate::github::types::{FullReviewThread, IssueComment, ReviewComment};

/// A fixed timestamp safely in the past relative to every test's `now`.
pub fn fixed_created_at() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap()
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
