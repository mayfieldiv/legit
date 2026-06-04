//! Pure file categoriser. Port of the TS `src/lib/file-categorizer.ts`: assign
//! every changed file a `FileCategory` from glob rules (user rules first, then
//! built-ins, defaulting to `Code`), and roll up per-category additions /
//! deletions / file counts (the `total` row is derived — see `Breakdown`). No
//! IO, no async — inputs are passed explicitly so the engine is unit-tested
//! synchronously, mirroring the `blocker` and `format` modules.
//!
//! The whole public surface (`categorize` + its result types) is consumed by
//! the summary panel: `update` calls `categorize` on `Msg::FilesArrived` and
//! the panel renders the resulting breakdown.

use std::sync::LazyLock;

use globset::{GlobBuilder, GlobMatcher};

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

impl FileCategory {
    /// The five categories in display order. The order is also the discriminant
    /// order, so `category as usize` indexes `Breakdown::rows`; the two must stay
    /// in sync (both are this list, spelled once here).
    pub const ALL: [FileCategory; 5] = [
        FileCategory::Code,
        FileCategory::Test,
        FileCategory::Generated,
        FileCategory::Docs,
        FileCategory::Config,
    ];

    /// Parse a config `fileRules` category string into a `FileCategory`. Returns
    /// `None` for anything outside the TS `FileCategory` union so an unknown
    /// string makes the rule a no-op rather than inventing a category.
    fn parse(s: &str) -> Option<FileCategory> {
        match s {
            "code" => Some(FileCategory::Code),
            "test" => Some(FileCategory::Test),
            "generated" => Some(FileCategory::Generated),
            "docs" => Some(FileCategory::Docs),
            "config" => Some(FileCategory::Config),
            _ => None,
        }
    }

    /// The category's lowercase label, as the config `fileRules` strings and the
    /// summary panel's breakdown rows spell it.
    pub fn as_str(self) -> &'static str {
        match self {
            FileCategory::Code => "code",
            FileCategory::Test => "test",
            FileCategory::Generated => "generated",
            FileCategory::Docs => "docs",
            FileCategory::Config => "config",
        }
    }
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
/// derived `total`). Mirrors the TS `CategoryStats`.
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

/// Per-category stats. Mirrors the TS `StatsByFileCategory` (a
/// `Record<FileCategory, CategoryStats>` plus `total`), but deliberately
/// diverges: the TS stores `total` alongside the rows, whereas here `total()`
/// derives it on demand so the rows and the total cannot drift apart. `rows` is
/// indexed by `FileCategory as usize` (the variant order is stable), so the
/// per-category fields never have to be enumerated by name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Breakdown {
    rows: [CategoryStats; 5],
}

// ── Algorithm ─────────────────────────────────────────────────────────────────

/// Categorise `files` against `user_rules` (then the built-in rules), returning
/// the tagged files and their breakdown. Mirrors the TS `categorizeFiles` +
/// `computeBreakdown`.
///
/// User rules are compiled once per call into the same `CompiledRule` shape as
/// the built-ins, then reused across every file. The TS does the equivalent
/// per-file with `Bun.Glob`, whose construction is cheap; `globset`'s glob
/// compilation is not, so compiling once keeps the synchronous `update` reducer
/// cheap on large diffs rather than porting the per-file rebuild.
pub fn categorize(files: &[FileChange], user_rules: &[FileRule]) -> FileCategorization {
    let user_compiled = compile_user_rules(user_rules);
    let mut breakdown = Breakdown::default();
    let categorized = files
        .iter()
        .map(|f| {
            let category = match_category(&f.path, &user_compiled);
            breakdown.row(category).accumulate(f);
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

impl Breakdown {
    /// The five per-category rows in display order, paired with their category.
    /// The summary panel iterates this rather than reaching into `rows` by index,
    /// so the render and the storage can't drift out of order. The `total` is
    /// read separately via `total`.
    pub fn category_rows(&self) -> [(FileCategory, CategoryStats); 5] {
        FileCategory::ALL.map(|category| (category, self.rows[category as usize]))
    }

    /// The total across all categories, summed on demand from the rows so it can
    /// never drift from them. (The TS stores `total` alongside the rows; here it
    /// is derived — see the `Breakdown` doc.)
    pub fn total(&self) -> CategoryStats {
        self.rows
            .iter()
            .fold(CategoryStats::default(), |acc, row| acc.add(*row))
    }

    /// The per-category row for `category`. Tests assert against a single
    /// category by name; the view reads every row through `category_rows`.
    #[cfg(test)]
    pub fn stats(&self, category: FileCategory) -> CategoryStats {
        self.rows[category as usize]
    }

    /// Mutable reference to the per-category row for `category`.
    fn row(&mut self, category: FileCategory) -> &mut CategoryStats {
        &mut self.rows[category as usize]
    }
}

impl CategoryStats {
    /// Fold one file's additions / deletions into this row and bump its count.
    fn accumulate(&mut self, file: &FileChange) {
        self.additions += file.additions;
        self.deletions += file.deletions;
        self.files += 1;
    }

    /// Sum two stats field-wise. Used by `Breakdown::total` to fold the rows.
    fn add(self, other: CategoryStats) -> CategoryStats {
        CategoryStats {
            additions: self.additions + other.additions,
            deletions: self.deletions + other.deletions,
            files: self.files + other.files,
        }
    }
}

/// Resolve the category for one path: the first matching user rule wins, then
/// the first matching built-in rule, defaulting to `Code`. User rules are
/// chained ahead of the built-ins so first-match-wins preserves user precedence
/// in a single pass. Mirrors the TS `matchCategory`.
fn match_category(path: &str, user_compiled: &[CompiledRule]) -> FileCategory {
    user_compiled
        .iter()
        .chain(BUILT_IN_RULES.iter())
        .find(|rule| rule.glob.is_match(path))
        .map_or(FileCategory::Code, |rule| rule.category)
}

/// Compile the user `fileRules` into `CompiledRule`s, once per `categorize`
/// call. A rule with an unparseable category or an invalid glob can't decide
/// anything, so it's dropped here rather than treated as a match — the TS never
/// reaches this state because its `category` is typed, but config here is
/// untyped JSON. Dropping it preserves the previous per-file skip semantics:
/// such a rule simply never matches.
fn compile_user_rules(user_rules: &[FileRule]) -> Vec<CompiledRule> {
    user_rules
        .iter()
        .filter_map(|rule| {
            let (Some(category), Ok(glob)) = (
                FileCategory::parse(&rule.category),
                build_glob(&rule.pattern),
            ) else {
                return None;
            };
            Some(CompiledRule { glob, category })
        })
        .collect()
}

// ── Rules ─────────────────────────────────────────────────────────────────

/// A compiled glob pattern and the category it assigns. Serves both the
/// built-in rules and the per-call compiled user rules.
struct CompiledRule {
    glob: GlobMatcher,
    category: FileCategory,
}

/// Compile a glob with Bun.Glob semantics: `*` and `?` stop at path separators
/// (`literal_separator`), `**` still crosses them, and `\` escapes specials —
/// matching the TS `Bun.Glob` the patterns were written against. Fallible so a
/// malformed user pattern degrades gracefully rather than panicking.
fn build_glob(pattern: &str) -> Result<GlobMatcher, globset::Error> {
    Ok(GlobBuilder::new(pattern)
        .literal_separator(true)
        .backslash_escape(true)
        .build()?
        .compile_matcher())
}

/// Compile a built-in pattern, which is known-valid at authoring time.
fn compile(pattern: &str) -> GlobMatcher {
    build_glob(pattern).expect("built-in glob pattern is valid")
}

/// Built-in pattern rules, in priority order. First match wins. Mirrors the TS
/// `BUILT_IN_RULES`.
static BUILT_IN_RULES: LazyLock<Vec<CompiledRule>> = LazyLock::new(|| {
    use FileCategory::*;
    [
        // generated
        ("**/*.lock", Generated),
        ("**/*-lock.json", Generated),
        ("**/*.snap", Generated),
        ("**/*.generated.*", Generated),
        ("**/generated/**", Generated),
        ("**/*.Designer.cs", Generated),
        ("**/*ModelSnapshot.cs", Generated),
        // test
        ("**/test/**", Test),
        ("**/tests/**", Test),
        ("**/__tests__/**", Test),
        ("**/*.test.*", Test),
        ("**/*.spec.*", Test),
        // .NET test conventions: project dirs like `Foo.UnitTests`, file names
        // like `FooTests.cs`.
        ("**/*Tests/**", Test),
        ("**/*Tests.cs", Test),
        ("**/*Test.cs", Test),
        // docs
        ("**/*.md", Docs),
        ("docs/**", Docs),
        ("README*", Docs),
        ("LICENSE*", Docs),
        ("CHANGELOG*", Docs),
        // config
        (".github/**", Config),
        ("**/*.yml", Config),
        ("**/*.yaml", Config),
        ("**/*.toml", Config),
        ("**/*.ini", Config),
        (".eslintrc*", Config),
        (".prettierrc*", Config),
        ("**/tsconfig*.json", Config),
        ("biome.json", Config),
        ("bunfig.toml", Config),
    ]
    .into_iter()
    .map(|(pattern, category)| CompiledRule {
        glob: compile(pattern),
        category,
    })
    .collect()
});

#[cfg(test)]
mod tests;
