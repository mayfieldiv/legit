import type {
  FileChange,
  FileCategory,
  FileChangeWithCategory,
  CategoryStats,
  StatsByFileCategory,
  FileCategorization,
} from "./types";
import type { FileRule } from "./config";

const BUILT_IN_RULES: Array<{ glob: Bun.Glob; category: FileCategory }> = [
  // generated
  { glob: new Bun.Glob("**/*.lock"), category: "generated" },
  { glob: new Bun.Glob("**/*-lock.json"), category: "generated" },
  { glob: new Bun.Glob("**/*.snap"), category: "generated" },
  { glob: new Bun.Glob("**/*.generated.*"), category: "generated" },
  { glob: new Bun.Glob("**/generated/**"), category: "generated" },
  { glob: new Bun.Glob("**/*.Designer.cs"), category: "generated" },
  { glob: new Bun.Glob("**/*ModelSnapshot.cs"), category: "generated" },
  // test
  { glob: new Bun.Glob("**/test/**"), category: "test" },
  { glob: new Bun.Glob("**/tests/**"), category: "test" },
  { glob: new Bun.Glob("**/__tests__/**"), category: "test" },
  { glob: new Bun.Glob("**/*.test.*"), category: "test" },
  { glob: new Bun.Glob("**/*.spec.*"), category: "test" },
  // .NET test conventions: project dirs like `Foo.UnitTests`, file names like `FooTests.cs`
  { glob: new Bun.Glob("**/*Tests/**"), category: "test" },
  { glob: new Bun.Glob("**/*Tests.cs"), category: "test" },
  { glob: new Bun.Glob("**/*Test.cs"), category: "test" },
  // docs
  { glob: new Bun.Glob("**/*.md"), category: "docs" },
  { glob: new Bun.Glob("docs/**"), category: "docs" },
  { glob: new Bun.Glob("README*"), category: "docs" },
  { glob: new Bun.Glob("LICENSE*"), category: "docs" },
  { glob: new Bun.Glob("CHANGELOG*"), category: "docs" },
  // config
  { glob: new Bun.Glob(".github/**"), category: "config" },
  { glob: new Bun.Glob("**/*.yml"), category: "config" },
  { glob: new Bun.Glob("**/*.yaml"), category: "config" },
  { glob: new Bun.Glob("**/*.toml"), category: "config" },
  { glob: new Bun.Glob("**/*.ini"), category: "config" },
  { glob: new Bun.Glob(".eslintrc*"), category: "config" },
  { glob: new Bun.Glob(".prettierrc*"), category: "config" },
  { glob: new Bun.Glob("**/tsconfig*.json"), category: "config" },
  { glob: new Bun.Glob("biome.json"), category: "config" },
  { glob: new Bun.Glob("bunfig.toml"), category: "config" },
];

const ZERO_STATS: CategoryStats = { additions: 0, deletions: 0, files: 0 };
const ALL_CATEGORIES: FileCategory[] = ["code", "test", "generated", "docs", "config"];

function emptyBreakdown(): StatsByFileCategory {
  const breakdown = { total: { ...ZERO_STATS } } as StatsByFileCategory;
  for (const cat of ALL_CATEGORIES) {
    breakdown[cat] = { ...ZERO_STATS };
  }
  return breakdown;
}

function matchCategory(path: string, userRules?: FileRule[]): FileCategory {
  if (userRules) {
    for (const rule of userRules) {
      if (new Bun.Glob(rule.pattern).match(path)) {
        return rule.category;
      }
    }
  }
  for (const rule of BUILT_IN_RULES) {
    if (rule.glob.match(path)) {
      return rule.category;
    }
  }
  return "code";
}

export function categorizeFiles(files: FileChange[], userRules?: FileRule[]): FileCategorization {
  const breakdown = emptyBreakdown();
  const categorized: FileChangeWithCategory[] = files.map((f) => {
    const category = matchCategory(f.path, userRules);
    breakdown[category].additions += f.additions;
    breakdown[category].deletions += f.deletions;
    breakdown[category].files += 1;
    breakdown.total.additions += f.additions;
    breakdown.total.deletions += f.deletions;
    breakdown.total.files += 1;
    return { ...f, category };
  });

  return { files: categorized, breakdown };
}
