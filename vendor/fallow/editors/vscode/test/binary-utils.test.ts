import * as path from "node:path";
import { describe, expect, it, vi, beforeEach } from "vitest";

let mockExistsSync: (p: string) => boolean = () => false;

vi.mock("node:fs", () => ({
  existsSync: (p: string) => mockExistsSync(p),
}));

vi.mock("vscode", () => ({
  workspace: {
    workspaceFolders: undefined as
      | Array<{ uri: { fsPath: string } }>
      | undefined,
  },
}));

import * as vscode from "vscode";
import { findLocalBinary, findBinaryInPath } from "../src/binary-utils.js";

describe("findLocalBinary", () => {
  beforeEach(() => {
    (vscode.workspace as any).workspaceFolders = undefined;
    mockExistsSync = () => false;
  });

  it("returns null when no workspace folders are open", () => {
    expect(findLocalBinary("fallow")).toBeNull();
  });

  it("returns null when node_modules/.bin binary does not exist", () => {
    (vscode.workspace as any).workspaceFolders = [
      { uri: { fsPath: "/workspace/project" } },
    ];
    mockExistsSync = () => false;

    expect(findLocalBinary("fallow")).toBeNull();
  });

  it("returns the path when node_modules/.bin binary exists", () => {
    (vscode.workspace as any).workspaceFolders = [
      { uri: { fsPath: "/workspace/project" } },
    ];
    mockExistsSync = () => true;

    const result = findLocalBinary("fallow");

    expect(result).toBe(
      path.join("/workspace/project", "node_modules", ".bin", "fallow")
    );
  });

  it("checks the first workspace folder only", () => {
    (vscode.workspace as any).workspaceFolders = [
      { uri: { fsPath: "/first" } },
      { uri: { fsPath: "/second" } },
    ];
    const calls: string[] = [];
    mockExistsSync = (p: string) => {
      calls.push(p);
      return false;
    };

    findLocalBinary("fallow");

    expect(calls).toHaveLength(1);
    expect(calls[0]).toBe(
      path.join("/first", "node_modules", ".bin", "fallow")
    );
  });
});

describe("findBinaryInPath", () => {
  beforeEach(() => {
    mockExistsSync = () => false;
  });

  it("returns null when binary is not in PATH", () => {
    expect(findBinaryInPath("fallow")).toBeNull();
  });

  it("returns the first matching path entry", () => {
    mockExistsSync = (p: string) => p.includes("/usr/local/bin");

    const originalPath = process.env["PATH"];
    process.env["PATH"] = `/usr/bin${path.delimiter}/usr/local/bin`;

    try {
      const result = findBinaryInPath("fallow");
      expect(result).toBe(path.join("/usr/local/bin", "fallow"));
    } finally {
      process.env["PATH"] = originalPath;
    }
  });
});
