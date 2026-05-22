import { describe, expect, it } from "vitest";
import { resolveFilePath } from "../src/treeView-utils.js";

describe("resolveFilePath", () => {
  it("returns empty strings when the input path is undefined", () => {
    // Regression for issue #323: stale type for UnlistedDependency caused the
    // tree view to pass undefined into path.* and crash the extension.
    expect(resolveFilePath(undefined, "/workspace")).toEqual({
      absolute: "",
      relative: "",
    });
  });

  it("returns empty strings when the input path is empty", () => {
    expect(resolveFilePath("", "/workspace")).toEqual({
      absolute: "",
      relative: "",
    });
  });

  it("resolves a relative path against the workspace root", () => {
    expect(resolveFilePath("src/foo.ts", "/workspace")).toEqual({
      absolute: "/workspace/src/foo.ts",
      relative: "src/foo.ts",
    });
  });

  it("keeps an absolute path absolute and computes a relative form", () => {
    expect(resolveFilePath("/workspace/src/foo.ts", "/workspace")).toEqual({
      absolute: "/workspace/src/foo.ts",
      relative: "src/foo.ts",
    });
  });

  it("falls back to the raw path when no workspace root is provided", () => {
    expect(resolveFilePath("src/foo.ts", undefined)).toEqual({
      absolute: "src/foo.ts",
      relative: "src/foo.ts",
    });
  });
});
