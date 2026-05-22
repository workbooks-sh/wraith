# wraith-lsp

LSP server surfacing [wraith](../..) findings as live editor diagnostics.

## Build + run

```bash
cargo build --bin wraith-lsp --release
./target/release/wraith-lsp --stdio
```

## What it publishes

On `textDocument/didOpen`, `didChange`, and `didSave` (debounced 500 ms), wraith-lsp re-runs the full wraith analysis (dead code, unused deps, circular deps, complexity hotspots, duplicates, boundary violations) for the workspace root containing the edited file and publishes findings as `textDocument/publishDiagnostics`.

Severity mapping:

| FindingKind          | LSP DiagnosticSeverity |
|----------------------|-----------------------|
| `DeadCode`           | `Warning` (+ `Unnecessary` tag) |
| `UnusedDep`          | `Warning` (+ `Unnecessary` tag) |
| `Complexity`         | `Information`         |
| `Duplicate`          | `Hint`                |
| `CircularDep`        | `Hint`                |
| `BoundaryViolation`  | `Error`               |
| `External` (fallow)  | mirrors `Finding.severity` |

## Code actions

For findings under the cursor:

- **DeadCode** → `wraith: remove dead item` (quickfix). Calls into `wraith_core::fix::plan` and emits a `wraith.applyFix` workspace command carrying the plan as an argument. Editors should bind that command to a script that invokes `wraith fix --apply`.
- **Complexity** → `wraith: extract function (not yet implemented)`. Stubbed; depends on the wb-5lgj.23 extract-function refactor agent. Surfaced as a `disabled` action so the UI affordance exists without firing a half-built refactor.

## VS Code client snippet

In your extension's `client.ts`:

```ts
import { LanguageClient, ServerOptions, TransportKind } from 'vscode-languageclient/node';

const serverOptions: ServerOptions = {
  command: 'wraith-lsp',
  args: ['--stdio'],
  transport: TransportKind.stdio,
};

const client = new LanguageClient(
  'wraith',
  'Wraith',
  serverOptions,
  { documentSelector: [{ language: 'rust' }] },
);

client.start();
```

## Status

Ships `wb-5lgj.12`. See `bd show wb-5lgj.12` for full context.
