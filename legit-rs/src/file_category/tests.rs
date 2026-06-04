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

// ── breakdown totals ───────────────────────────────────────────────────────────

#[test]
fn breakdown_total_sums_all_files() {
    let files = [
        file("src/a.rs", 10, 2),
        file("src/b.rs", 5, 1),
        file("src/c.rs", 0, 7),
    ];
    let result = categorize(&files, &[]);

    // All three default to code, so the code row and the total row both equal
    // the input sums.
    let expected = CategoryStats {
        additions: 15,
        deletions: 10,
        files: 3,
    };
    assert_eq!(result.breakdown.code, expected);
    assert_eq!(result.breakdown.total, expected);
    assert_eq!(result.breakdown.test, CategoryStats::default());
}
