import { describe, test, expect } from "bun:test";
import { categorizeFiles } from "../src/lib/file-categorizer";
import type { FileChange } from "../src/lib/types";

function file(path: string, additions = 10, deletions = 5): FileChange {
  return { path, additions, deletions };
}

describe("categorizeFiles", () => {
  describe("built-in heuristics", () => {
    test("categorizes lockfiles as generated", () => {
      const result = categorizeFiles([
        file("package-lock.json"),
        file("bun.lock"),
        file("yarn.lock"),
        file("pnpm-lock.json"),
      ]);
      for (const f of result.files) {
        expect(f.category).toBe("generated");
      }
    });

    test("categorizes snapshots as generated", () => {
      const result = categorizeFiles([file("src/__snapshots__/app.test.snap")]);
      expect(result.files[0]!.category).toBe("generated");
    });

    test("categorizes *.generated.* as generated", () => {
      const result = categorizeFiles([file("src/schema.generated.ts")]);
      expect(result.files[0]!.category).toBe("generated");
    });

    test("categorizes files under generated/ directories as generated", () => {
      const result = categorizeFiles([
        file("frontend/src/api/backend/generated/interfaces.ts"),
        file("frontend/src/api/backend/generated/responses.ts"),
      ]);
      for (const f of result.files) {
        expect(f.category).toBe("generated");
      }
    });

    test("categorizes EF Core Designer.cs and ModelSnapshot.cs as generated", () => {
      const result = categorizeFiles([
        file("backend/Persistence/Migrations/20260321_AddFoo.Designer.cs"),
        file("backend/Persistence/Migrations/MyDbContextModelSnapshot.cs"),
      ]);
      for (const f of result.files) {
        expect(f.category).toBe("generated");
      }
    });

    test("categorizes test files as test", () => {
      const result = categorizeFiles([
        file("src/lib/foo.test.ts"),
        file("src/lib/bar.spec.tsx"),
        file("tests/unit/baz.ts"),
        file("src/__tests__/qux.ts"),
      ]);
      for (const f of result.files) {
        expect(f.category).toBe("test");
      }
    });

    test("categorizes .NET test files as test", () => {
      const result = categorizeFiles([
        file("backend/Immybot.Backend.UnitTests/ContextTests/TenantExtensionTests/BasicTests.cs"),
        file("backend/Foo.IntegrationTests/SomeTests.cs"),
        file("backend/Foo.Tests/Bar.cs"),
        file("backend/Project/Module/SomeTest.cs"),
      ]);
      for (const f of result.files) {
        expect(f.category).toBe("test");
      }
    });

    test("categorizes docs files as docs", () => {
      const result = categorizeFiles([
        file("README.md"),
        file("docs/guide.md"),
        file("CHANGELOG.md"),
        file("LICENSE"),
      ]);
      for (const f of result.files) {
        expect(f.category).toBe("docs");
      }
    });

    test("categorizes config files as config", () => {
      const result = categorizeFiles([
        file(".github/workflows/ci.yml"),
        file("tsconfig.json"),
        file("biome.json"),
        file("bunfig.toml"),
        file(".prettierrc.json"),
      ]);
      for (const f of result.files) {
        expect(f.category).toBe("config");
      }
    });

    test("defaults unknown files to code", () => {
      const result = categorizeFiles([
        file("src/lib/foo.ts"),
        file("src/components/Bar.tsx"),
        file("index.js"),
      ]);
      for (const f of result.files) {
        expect(f.category).toBe("code");
      }
    });
  });

  describe("aggregate breakdown", () => {
    test("computes correct stats per category", () => {
      const result = categorizeFiles([
        file("src/app.ts", 100, 20),
        file("src/app.test.ts", 50, 10),
        file("bun.lock", 500, 200),
      ]);
      expect(result.breakdown.code).toEqual({ additions: 100, deletions: 20, files: 1 });
      expect(result.breakdown.test).toEqual({ additions: 50, deletions: 10, files: 1 });
      expect(result.breakdown.generated).toEqual({
        additions: 500,
        deletions: 200,
        files: 1,
      });
      expect(result.breakdown.docs).toEqual({ additions: 0, deletions: 0, files: 0 });
      expect(result.breakdown.config).toEqual({ additions: 0, deletions: 0, files: 0 });
      expect(result.breakdown.total).toEqual({ additions: 650, deletions: 230, files: 3 });
    });

    test("returns zeroes for empty file list", () => {
      const result = categorizeFiles([]);
      expect(result.files).toEqual([]);
      expect(result.breakdown.total).toEqual({ additions: 0, deletions: 0, files: 0 });
      expect(result.breakdown.code).toEqual({ additions: 0, deletions: 0, files: 0 });
    });
  });

  describe("user rules", () => {
    test("user rules take precedence over built-in heuristics", () => {
      const result = categorizeFiles(
        [file("src/app.test.ts")],
        [{ pattern: "**/*.test.*", category: "code" }],
      );
      // Built-in would categorize as "test", but user rule overrides to "code"
      expect(result.files[0]!.category).toBe("code");
    });

    test("first matching user rule wins", () => {
      const result = categorizeFiles(
        [file("src/app.ts")],
        [
          { pattern: "src/**", category: "test" },
          { pattern: "**/*.ts", category: "docs" },
        ],
      );
      expect(result.files[0]!.category).toBe("test");
    });

    test("falls through to built-in heuristics when no user rule matches", () => {
      const result = categorizeFiles([file("bun.lock")], [{ pattern: "src/**", category: "test" }]);
      expect(result.files[0]!.category).toBe("generated");
    });
  });
});
