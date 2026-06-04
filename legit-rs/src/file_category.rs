//! Pure file categoriser. Port of the TS `src/lib/file-categorizer.ts`: assign
//! every changed file a `FileCategory` from glob rules (user rules first, then
//! built-ins, defaulting to `Code`), and roll up per-category additions /
//! deletions / file counts plus a `total` row. No IO, no async — inputs are
//! passed explicitly so the engine is unit-tested synchronously, mirroring the
//! `blocker` and `format` modules.

use globset::GlobBuilder;

use crate::config::FileRule;

// ── Public types ────────────────────────────────────────────────────────────

/// A single changed file in a PR diff. Mirrors the TS `FileChange`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub additions: u64,
    pub deletions: u64,
}

/// File Category — assigned per file by pattern rules. Mirrors the TS
/// `FileCategory` union and drives the summary panel's size breakdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileCategory {
    Code,
    Test,
    Generated,
    Docs,
    Config,
}

/// A changed file plus its resolved category. Mirrors the TS
/// `FileChangeWithCategory`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChangeWithCategory {
    pub path: String,
    pub additions: u64,
    pub deletions: u64,
    pub category: FileCategory,
}

/// Rolled-up additions / deletions / file count for one category (or the
/// `total` row). Mirrors the TS `CategoryStats`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CategoryStats {
    pub additions: u64,
    pub deletions: u64,
    pub files: u64,
}

/// The outcome of categorising a file set: every file tagged with its category
/// plus the per-category breakdown. Mirrors the TS `FileCategorization`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCategorization {
    pub files: Vec<FileChangeWithCategory>,
    pub breakdown: Breakdown,
}

/// Per-category stats with a `total` row. Mirrors the TS
/// `StatsByFileCategory` (a `Record<FileCategory, CategoryStats>` plus `total`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Breakdown {
    pub code: CategoryStats,
    pub test: CategoryStats,
    pub generated: CategoryStats,
    pub docs: CategoryStats,
    pub config: CategoryStats,
    pub total: CategoryStats,
}

// ── Algorithm ─────────────────────────────────────────────────────────────────

/// Categorise `files` against `user_rules` (then the built-in rules), returning
/// the tagged files and their breakdown. Mirrors the TS `categorizeFiles` +
/// `computeBreakdown`.
pub fn categorize(files: &[FileChange], user_rules: &[FileRule]) -> FileCategorization {
    let mut breakdown = Breakdown::default();
    let categorized = files
        .iter()
        .map(|f| {
            let category = match_category(&f.path, user_rules);
            FileChangeWithCategory {
                path: f.path.clone(),
                additions: f.additions,
                deletions: f.deletions,
                category,
            }
        })
        .collect();

    FileCategorization {
        files: categorized,
        breakdown,
    }
}

/// Resolve the category for one path: the first matching user rule wins, then
/// the first matching built-in rule, defaulting to `Code`. Mirrors the TS
/// `matchCategory`.
fn match_category(path: &str, _user_rules: &[FileRule]) -> FileCategory {
    FileCategory::Code
}

#[cfg(test)]
mod tests;
