# Fallow for Zed

Zed extension for [`fallow-lsp`](https://github.com/fallow-rs/fallow), the language server behind Fallow's editor diagnostics.

## What works

- diagnostics for unused files, exports, types, dependencies, enum/class members, unresolved imports, unlisted deps, duplicate exports, circular dependencies, and duplication
- hover information
- quick-fix code actions
- code lens where Zed surfaces them

This extension is intentionally thin. It launches the existing `fallow-lsp` binary instead of re-implementing analysis logic inside the editor.

## Binary resolution

The extension looks for `fallow-lsp` in this order:

1. `lsp.fallow.binary.path`
2. local `node_modules/.bin/fallow-lsp` in the current worktree
3. `fallow-lsp` on `PATH`
4. a managed binary downloaded from the latest GitHub release and verified against Fallow's Ed25519 signing key

If you already install Fallow through npm or a package manager, you usually do not need to configure anything.

## Settings

If you customize `language_servers` for a language, keep `fallow` or `...` in the list so the extension still runs:

```json
{
  "languages": {
    "TypeScript": {
      "language_servers": ["fallow", "..."]
    },
    "JavaScript": {
      "language_servers": ["fallow", "..."]
    }
  }
}
```

To point Zed at a specific binary:

```json
{
  "lsp": {
    "fallow": {
      "binary": {
        "path": "/absolute/path/to/fallow-lsp",
        "arguments": []
      }
    }
  }
}
```

Fallow currently reads issue toggles from LSP initialization options:

```json
{
  "lsp": {
    "fallow": {
      "initialization_options": {
        "issueTypes": {
          "unused-files": true,
          "unused-exports": true,
          "unused-types": true,
          "unused-dependencies": true,
          "unused-dev-dependencies": true,
          "unused-optional-dependencies": true,
          "unused-enum-members": true,
          "unused-class-members": true,
          "unresolved-imports": true,
          "unlisted-dependencies": true,
          "duplicate-exports": true,
          "type-only-dependencies": true,
          "circular-dependencies": true,
          "stale-suppressions": true
        }
      }
    }
  }
}
```

## Development

1. Open Zed.
2. Run `zed: install dev extension`.
3. Select `editors/zed`.
4. Open a TypeScript or JavaScript project and confirm `fallow` is running in the language server UI.

If Zed opens the project in Restricted Mode, trust the worktree first. Restricted Mode blocks language servers entirely.

To preflight the actual packaged extension artifact locally, install the target once with `rustup target add wasm32-wasip2` and run `cargo build --target wasm32-wasip2 --manifest-path editors/zed/Cargo.toml`.

## Linux note

Zed's extension API exposes OS and CPU architecture, but not glibc vs musl. The managed download therefore uses the GNU Linux release asset. On musl/Nix-style setups, prefer `PATH` or `lsp.fallow.binary.path`.
