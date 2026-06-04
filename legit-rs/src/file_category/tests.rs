//! Unit tests for the file categoriser. Mirror the behaviour of the TS
//! `tests/file-categorizer.test.ts`: glob matching against user `fileRules`,
//! the built-in extension / path heuristics, and breakdown totals. Pure and
//! synchronous — no tokio.

use super::{CategoryStats, FileCategory, FileChange, categorize};
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
    assert_eq!(result.breakdown.stats(FileCategory::Code), expected);
    assert_eq!(result.breakdown.total, expected);
    assert_eq!(
        result.breakdown.stats(FileCategory::Test),
        CategoryStats::default()
    );
}

// ── built-in rules: generated ──────────────────────────────────────────────────

#[test]
fn lock_files_are_generated() {
    assert_eq!(category_of("bun.lock"), FileCategory::Generated);
    assert_eq!(category_of("deep/nested/bun.lock"), FileCategory::Generated);
    assert_eq!(category_of("package-lock.json"), FileCategory::Generated);
}

#[test]
fn generated_extension_and_dir_heuristics() {
    assert_eq!(
        category_of("src/__snapshots__/a.snap"),
        FileCategory::Generated
    );
    assert_eq!(category_of("src/api.generated.ts"), FileCategory::Generated);
    assert_eq!(
        category_of("src/generated/client.ts"),
        FileCategory::Generated
    );
    assert_eq!(
        category_of("Forms/MainForm.Designer.cs"),
        FileCategory::Generated
    );
    assert_eq!(
        category_of("Migrations/AppDbModelSnapshot.cs"),
        FileCategory::Generated
    );
}

// ── built-in rules: test ───────────────────────────────────────────────────────

#[test]
fn test_dirs_and_file_conventions() {
    assert_eq!(category_of("src/test/foo.rs"), FileCategory::Test);
    assert_eq!(category_of("pkg/tests/bar.rs"), FileCategory::Test);
    assert_eq!(category_of("src/__tests__/baz.ts"), FileCategory::Test);
    assert_eq!(category_of("src/foo.test.ts"), FileCategory::Test);
    assert_eq!(category_of("src/foo.spec.ts"), FileCategory::Test);
    // .NET conventions.
    assert_eq!(category_of("Foo.UnitTests/Bar.cs"), FileCategory::Test);
    assert_eq!(category_of("src/WidgetTests.cs"), FileCategory::Test);
    assert_eq!(category_of("src/WidgetTest.cs"), FileCategory::Test);
}

// ── built-in rules: docs ───────────────────────────────────────────────────────

#[test]
fn docs_extension_and_root_files() {
    assert_eq!(category_of("notes/design.md"), FileCategory::Docs);
    assert_eq!(category_of("docs/guide.txt"), FileCategory::Docs);
    assert_eq!(category_of("README"), FileCategory::Docs);
    assert_eq!(category_of("README.md"), FileCategory::Docs);
    assert_eq!(category_of("LICENSE"), FileCategory::Docs);
    assert_eq!(category_of("CHANGELOG.md"), FileCategory::Docs);
}

// ── built-in rules: config ─────────────────────────────────────────────────────

#[test]
fn config_extensions_and_named_files() {
    assert_eq!(
        category_of(".github/workflows/ci.yml"),
        FileCategory::Config
    );
    assert_eq!(category_of("k8s/deploy.yaml"), FileCategory::Config);
    assert_eq!(category_of("Cargo.toml"), FileCategory::Config);
    assert_eq!(category_of("setup.ini"), FileCategory::Config);
    assert_eq!(category_of(".eslintrc.json"), FileCategory::Config);
    assert_eq!(category_of(".prettierrc"), FileCategory::Config);
    assert_eq!(
        category_of("packages/web/tsconfig.build.json"),
        FileCategory::Config
    );
    assert_eq!(category_of("biome.json"), FileCategory::Config);
    assert_eq!(category_of("bunfig.toml"), FileCategory::Config);
}

// ── precedence ─────────────────────────────────────────────────────────────────

#[test]
fn earlier_built_in_rule_wins() {
    // `**/*.md` (docs) precedes `docs/**` but also `*.lock` (generated) precedes
    // everything: a generated-looking markdown still resolves by first match.
    // Here a `.md` under `docs/` matches the docs extension rule first, which is
    // still docs — but a `.snap` under `tests/` must resolve generated, not test,
    // because the generated block precedes the test block.
    assert_eq!(category_of("tests/render.snap"), FileCategory::Generated);
    // A markdown file inside a tests dir is docs only if a test rule does not
    // match first: `**/tests/**` (test) precedes `**/*.md` (docs), so it's test.
    assert_eq!(category_of("tests/README.md"), FileCategory::Test);
}

// ── user rules ─────────────────────────────────────────────────────────────────

#[test]
fn user_rule_overrides_built_in() {
    // `.md` is docs by built-in, but a user rule classifying everything under
    // `docs/` as code must win because user rules are checked first.
    let rules = [rule("docs/**", "code")];
    let result = categorize(&[file("docs/design.md", 1, 0)], &rules);
    assert_eq!(result.files[0].category, FileCategory::Code);
}

#[test]
fn first_matching_user_rule_wins() {
    // Two user rules match the same path; the earlier one decides the category.
    let rules = [rule("src/**", "generated"), rule("src/**/*.rs", "test")];
    let result = categorize(&[file("src/app/main.rs", 1, 0)], &rules);
    assert_eq!(result.files[0].category, FileCategory::Generated);
}

#[test]
fn user_rule_with_unknown_category_is_skipped() {
    // An unrecognised category can't classify anything, so the rule is ignored
    // and the path falls through to the built-in docs rule.
    let rules = [rule("**/*.md", "prose")];
    let result = categorize(&[file("notes.md", 1, 0)], &rules);
    assert_eq!(result.files[0].category, FileCategory::Docs);
}
