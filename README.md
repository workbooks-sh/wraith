# wraith

**One analyzer for Rust + TypeScript/JavaScript.** Dead code, dupes,
cycles, complexity, boundaries, safe auto-fix — single binary, JSON
output, pre-commit-friendly exit codes.

Most code-health tools pick a language and a niche: `cargo-udeps` for
unused deps, `cargo-machete` for the same, `dpdm` for JS cycles,
`madge` for graphs, a linter for complexity, a custom script for
boundaries. wraith unifies that surface behind one CLI with one
finding schema, so a workspace pass returns one report regardless of
how many languages live in the tree.

TS/JS analysis delegates to [`fallow`](https://github.com/fallow-rs/fallow)
under the hood; findings come back wrapped in the same schema.

## Subcommands

| command | what it does |
|---|---|
| `wraith dead-code` | pub items with no references in the workspace |
| `wraith unused-deps` | deps in `Cargo.toml` with no `use` / path reference |
| `wraith circular-deps` | crate-level + module-level cycles (Tarjan SCC) |
| `wraith dupes` | token-shingled fn-body clone detection |
| `wraith health` | cyclomatic + cognitive complexity hotspots |
| `wraith boundaries` | module-import allow/deny rules |
| `wraith fix` | safe auto-remove of dead pub items + unused deps (dry-run by default; `--apply` writes) |
| `wraith audit` | dead-code + unused-deps scoped to git-changed files (pre-commit gate) |
| `wraith init` | scaffold `.wraithrc.json`; optional `--ci=github\|gitlab` |
| `wraith hooks install` | git pre-commit + Claude Code hook |
| `wraith migrate --from clippy\|deny` | translate other tools' configs |
| `wraith watch` | re-run on file save; emits jsonl with `batch-end` markers |
| `wraith refactor extract-fn` | extract a contiguous line range from an enclosing fn into a new fn |
| `wraith report` | run all detectors and emit a markdown summary (paste into README / PR) |
| `wraith deps duplicates` | crates resolved at multiple versions in `Cargo.lock`, with caller attribution |
| `wraith deps audit` | known security advisories (shells out to optional `cargo-audit`) |
| `wraith deps unused-features` | flag `features = […]` entries with no usage |
| `wraith deps size` | binary-size attribution per crate (shells out to optional `cargo-bloat`) |

Exit codes: `0` no findings, `1` findings, `2` internal error, `64`
missing optional binary.

## Install

```bash
cargo install wraith-cli
wraith --help
```

For TS/JS analysis, also install `fallow` and put it on PATH.

## Run

```bash
wraith --root . dead-code
wraith --root . unused-deps --format json
wraith audit --exit-zero
wraith circular-deps
wraith dupes
wraith health
wraith fix --apply         # writes
wraith watch               # streams jsonl on save
wraith report              # full markdown summary
```

## Design

- **Parser (Rust):** `syn` 2.x — portable, no rustc internals.
- **Workspace discovery:** `cargo_metadata`.
- **Graph:** name-based symbol + reference graph; cycles via `petgraph`.
- **TS/JS:** dispatched to a `fallow` binary on PATH; findings wrapped
  into the unified `Finding` schema.

```
wraith/
├── crates/
│   ├── wraith-core/    library — workspace + parse + graph + analyze +
│   │                              circular, dupes, health, boundaries, fix
│   └── wraith-cli/     binary — `wraith` + hooks, migrate, watch, fallow shim
└── examples/
    └── small-test-crate/  fixture used by tests
```

## Finding schema

All commands emit Finding records carrying a `schema_version`,
`severity` (`info` / `warning` / `error`), and a `kind` tagged union:
`dead-code`, `unused-dep`, `circular-dep`, `duplicate`, `complexity`,
`boundary-violation`, `external` (wrapper around fallow output).

## Config — `.wraithrc.json`

```json
{
  "ignore": ["target", "node_modules", ".git"],
  "allow_dead": [],
  "allow_unused_deps": [],
  "treat_pub_crate_as_internal": true,
  "duplicates": { "min_tokens": 40, "similarity_threshold": 0.85 },
  "complexity": { "cyclomatic": 15, "cognitive": 25 },
  "boundaries": [
    { "from": "guarded_crate", "allow": ["public_api"], "deny": ["internal"] }
  ]
}
```

## Resolution limits

The resolver is name-based — intentionally not type-aware. Trade-offs
are false-negatives, not false-positives:

- A `pub fn foo` collides with any other `foo` in scope; if either is
  referenced, both look alive.
- Macro-generated items aren't seen.
- Items inside `mod tests` are skipped wholesale.
- `cfg(...)` blocks other than `cfg(test)` are walked unconditionally.

`fix --apply` will not remove anything the resolver isn't certain about.

## AI agent skill

A `wraith` skill ships in [`skills/wraith/`](skills/wraith/) — a
decision tree that maps intents ("I need to find code", "this fn is
too complex") to wraith commands, plus deep dives on navigation,
refactoring, diff reports, and monorepo usage. Drop it into Claude
Code with:

```bash
ln -sfn "$(pwd)/skills/wraith" ~/.claude/skills/wraith
```

## Tests

```bash
cargo test --workspace
```

Integration tests spawn synthetic Cargo workspaces in `tempdir`s,
invoke the `wraith` binary, and assert on stdout. Plus unit tests in
core crates and protocol smoke tests for `wraith-lsp` and `wraith-mcp`.
