//! Unit tests for the file categoriser. Mirror the behaviour of the TS
//! `tests/file-categorizer.test.ts`: glob matching against user `fileRules`,
//! the built-in extension / path heuristics, and breakdown totals. Pure and
//! synchronous — no tokio.

use super::{Breakdown, CategoryStats, FileCategory, FileChange, categorize};
use crate::config::FileRule;

// ── helpers ───────────────────────────────────────────────────────────────────

fn file(path: &str, additions: u64, deletions: u64) -> FileChange {
    FileChange {
        path: path.to_owned(),
        additions,
        deletions,
    }
}

fn rule(pattern: &str, category: &str) -> FileRule {
    FileRule {
        pattern: pattern.to_owned(),
        category: category.to_owned(),
    }
}

/// Resolve the category a single path lands in, with no user rules.
fn category_of(path: &str) -> FileCategory {
    let result = categorize(&[file(path, 0, 0)], &[]);
    result.files[0].category
}

// ── tracer bullet ──────────────────────────────────────────────────────────────

#[test]
fn unmatched_path_defaults_to_code() {
    assert_eq!(category_of("src/main.rs"), FileCategory::Code);
}
