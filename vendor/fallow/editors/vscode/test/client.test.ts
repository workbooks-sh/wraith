import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("vscode", () => ({
  extensions: {
    getExtension: vi.fn(),
  },
  window: {
    showErrorMessage: vi.fn(),
    showWarningMessage: vi.fn(),
  },
}));

vi.mock("vscode-languageclient/node.js", () => ({
  LanguageClient: class {},
  TransportKind: {
    stdio: 0,
  },
}));

import { loadDiagnosticCategories } from "../src/client.js";
import {
  DIAGNOSTIC_CATEGORIES,
  getDiagnosticCategories,
  resetDiagnosticCategories,
  setDiagnosticCategories,
} from "../src/diagnosticFilter.js";

afterEach(() => {
  resetDiagnosticCategories();
});

const outputChannel = () => ({
  lines: [] as string[],
  appendLine(line: string) {
    this.lines.push(line);
  },
});

describe("loadDiagnosticCategories", () => {
  it("loads categories from fallow/issueTypes", async () => {
    const out = outputChannel();
    const client = {
      sendRequest: vi.fn(async () => [
        { code: "future-rule", label: "Future Rule" },
      ]),
    };

    await loadDiagnosticCategories(client as never, out as never);

    expect(client.sendRequest).toHaveBeenCalledWith("fallow/issueTypes");
    expect(getDiagnosticCategories()).toEqual([
      { code: "future-rule", label: "Future Rule" },
    ]);
    expect(out.lines.at(-1)).toBe(
      "Loaded 1 diagnostic categories from fallow-lsp."
    );
  });

  it("falls back to bundled categories when the request fails", async () => {
    setDiagnosticCategories([{ code: "stale-rule", label: "Stale Rule" }]);
    const out = outputChannel();
    const client = {
      sendRequest: vi.fn(async () => {
        throw new Error("method not found");
      }),
    };

    await loadDiagnosticCategories(client as never, out as never);

    expect(getDiagnosticCategories()).toBe(DIAGNOSTIC_CATEGORIES);
    expect(out.lines.at(-1)).toContain("using bundled diagnostic categories");
  });
});
