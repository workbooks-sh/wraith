import { afterEach, describe, expect, it, vi } from "vitest";

const vscodeMocks = vi.hoisted(() => ({
  createQuickPick: vi.fn(),
}));

vi.mock("vscode", () => {
  type Listener<T> = (value: T) => void;
  class FakeEventEmitter<T> {
    private readonly listeners = new Set<Listener<T>>();
    public readonly event = (
      listener: Listener<T>
    ): { dispose: () => void } => {
      this.listeners.add(listener);
      return { dispose: () => this.listeners.delete(listener) };
    };
    public fire(value: T): void {
      for (const listener of this.listeners) {
        listener(value);
      }
    }
    public dispose(): void {
      this.listeners.clear();
    }
  }

  return {
    CodeActionKind: {
      QuickFix: {
        append: (value: string) => `quickfix.${value}`,
      },
    },
    EventEmitter: FakeEventEmitter,
    LanguageStatusSeverity: {
      Information: 0,
      Warning: 1,
    },
    MarkdownString: class {
      public isTrusted = false;
      public supportThemeIcons = false;
      public constructor(public readonly value: string) {}
    },
    ThemeIcon: class {
      public constructor(public readonly id: string) {}
    },
    Uri: {
      parse: (s: string) => ({ toString: () => s, scheme: "file" }),
    },
    languages: {
      createLanguageStatusItem: vi.fn((id: string, selector: unknown) => ({
        id,
        selector,
        name: undefined,
        severity: undefined,
        text: "",
        detail: undefined,
        command: undefined,
        dispose: vi.fn(),
      })),
    },
    window: {
      createQuickPick: vscodeMocks.createQuickPick,
      setStatusBarMessage: vi.fn(),
    },
    workspace: {
      onDidCloseTextDocument: vi.fn(),
    },
    commands: {
      executeCommand: vi.fn(),
      registerCommand: vi.fn(),
    },
    CodeAction: class {
      public command: unknown;
      public diagnostics: unknown;
      public constructor(
        public readonly title: string,
        public readonly kind: string
      ) {}
    },
  };
});

import {
  DiagnosticFilter,
  resetDiagnosticCategories,
  setDiagnosticCategories,
} from "../src/diagnosticFilter.js";
import { __testHelpers } from "../src/diagnosticMute.js";

const memento = () => ({
  get: <T>(): T | undefined => undefined,
  update: vi.fn(async () => {}),
  keys: () => [],
});

const quickPickThatAcceptsDefaults = () => {
  let accept: (() => void) | undefined;
  let hide: (() => void) | undefined;
  return {
    title: "",
    placeholder: "",
    canSelectMany: false,
    matchOnDetail: false,
    buttons: [],
    items: [],
    selectedItems: [],
    onDidTriggerButton: vi.fn(() => ({ dispose: vi.fn() })),
    onDidAccept: vi.fn((listener: () => void) => {
      accept = listener;
      return { dispose: vi.fn() };
    }),
    onDidHide: vi.fn((listener: () => void) => {
      hide = listener;
      return { dispose: vi.fn() };
    }),
    show: vi.fn(() => {
      accept?.();
    }),
    hide: vi.fn(() => {
      hide?.();
    }),
    dispose: vi.fn(),
  };
};

afterEach(() => {
  resetDiagnosticCategories();
});

describe("diagnostic mute language status", () => {
  it("is hidden until a mute is active, then hides again after clearing", () => {
    const filter = new DiagnosticFilter(memento() as never);
    const item = __testHelpers.createLanguageStatus(filter);

    expect(item.selector).toEqual([]);
    expect(item.command).toBeUndefined();

    filter.setCategoryMuted("code-duplication", true);
    expect(item.selector).toEqual([
      { scheme: "file", language: "javascript" },
      { scheme: "file", language: "javascriptreact" },
      { scheme: "file", language: "typescript" },
      { scheme: "file", language: "typescriptreact" },
      { scheme: "file", language: "vue" },
      { scheme: "file", language: "svelte" },
      { scheme: "file", language: "astro" },
      { scheme: "file", language: "mdx" },
      { scheme: "file", language: "json" },
    ]);
    expect(item.command).toMatchObject({
      command: "fallow.manageDiagnosticMutes",
    });

    filter.clearAllMutes();
    expect(item.selector).toEqual([]);
    expect(item.command).toBeUndefined();
  });

  it("keeps global mute separate when accepting the default manage picker state", async () => {
    const pick = quickPickThatAcceptsDefaults();
    vscodeMocks.createQuickPick.mockReturnValueOnce(pick);
    const filter = new DiagnosticFilter(memento() as never);
    filter.setMutedAll(true);

    await __testHelpers.showManageQuickPick(filter);

    expect(filter.isMutedAll()).toBe(true);
    expect(filter.mutedCategoriesSnapshot().size).toBe(0);
    expect(
      (pick.selectedItems as Array<{ code: string | null }>).some(
        (item) => item.code === null
      )
    ).toBe(true);
  });

  it("uses LSP-provided categories for labels and the manage picker", async () => {
    setDiagnosticCategories([
      { code: "future-rule", label: "Future Rule" },
      { code: "unused-export", label: "Unused Exports" },
    ]);
    const pick = quickPickThatAcceptsDefaults();
    vscodeMocks.createQuickPick.mockReturnValueOnce(pick);
    const filter = new DiagnosticFilter(memento() as never);

    expect(__testHelpers.labelFor("future-rule")).toBe("Future Rule");

    await __testHelpers.showManageQuickPick(filter);

    expect(
      (pick.items as Array<{ code: string | null; label: string }>).map(
        (item) => [item.code, item.label]
      )
    ).toEqual([
      [null, "$(eye-closed) All Fallow Findings"],
      ["future-rule", "Future Rule"],
      ["unused-export", "Unused Exports"],
    ]);
  });
});
