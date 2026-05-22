# wraith — Rust + TS/JS codebase analyzer

Unified analyzer for Rust + TypeScript/JavaScript codebases. One binary,
one workspace pass, JSON / JSONL / human output, pre-commit-friendly
exit codes. TS/JS analysis delegates to [`fallow`](https://github.com/fallow-rs/fallow),
vendored at `vendor/fallow/`.

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
| `wraith init` | scaffold `.wraithrc.json`; optional `--ci=github|gitlab` |
| `wraith hooks install` | git pre-commit + Claude Code hook |
| `wraith migrate --from clippy|deny` | translate other tools' configs |
| `wraith watch` | re-run on file save; emits jsonl with `batch-end` markers |
| `wraith refactor extract-fn` | extract a contiguous line range from an enclosing fn into a new fn |
| `wraith report` | run all detectors and emit a markdown summary (paste into README / PR) |
| `wraith deps duplicates` | crates resolved at multiple versions in `Cargo.lock`, with caller attribution |
| `wraith deps audit` | known security advisories (shells out to optional `cargo-audit`) |
| `wraith deps unused-features` | flag `features = […]` entries with no usage (v1 heuristic; emits `verify-manually`) |
| `wraith deps size` | binary-size attribution per crate (shells out to optional `cargo-bloat`) |

Exit codes: `0` no findings, `1` findings, `2` internal error, `64` missing optional binary.

The `deps audit` / `deps size` subcommands require third-party tooling on PATH:
`cargo install cargo-audit` and `cargo install cargo-bloat`. They exit `64` with
an install hint when missing — no vendored shim.

## Architecture

```
wraith/
├── crates/
│   ├── wraith-core/    library — workspace + parse + graph + analyze + audit + new modules:
│   │                              circular, dupes, health, boundaries, fix
│   └── wraith-cli/     binary — `wraith` + hooks, migrate, watch, fallow shim
├── vendor/
│   └── fallow/         git subtree (fallow-rs/fallow main, MIT) for TS/JS analysis
└── examples/
    └── small-test-crate/  fixture used by tests
```

- Parser (Rust): `syn` 2.x — portable, no rustc internals.
- Workspace discovery: `cargo_metadata`.
- Graph: name-based symbol+reference graph; cycles via `petgraph`.
- TS/JS: dispatched to a `fallow` binary on PATH; findings wrapped into
  the unified `Finding` schema with `source: "fallow"`.

## Finding schema (v1)

All commands emit Finding records carrying a `schema_version` (currently
1), `severity` (`info` / `warning` / `error`), and a `kind` tagged union
covering: `dead-code`, `unused-dep`, `circular-dep`, `duplicate`,
`complexity`, `boundary-violation`, and `external` (the wrapper around
fallow output).

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

Name-based resolver — intentionally not type-aware. Trade-offs are
false-negatives, not false-positives:

- A `pub fn foo` collides with any other `foo` in scope; if either is
  referenced, both look alive.
- Macro-generated items aren't seen.
- Items inside `mod tests` are skipped wholesale.
- `cfg(...)` blocks other than `cfg(test)` are walked unconditionally.

A future MIR-backed mode (tied to a pinned nightly) could close the gap.

## Vendored fallow

`vendor/fallow/` is a `git subtree` of `github.com/fallow-rs/fallow`
main, squashed. It is excluded from this workspace via
`[workspace] exclude` because:

- fallow's workspace uses `resolver = "3"` (Rust 2024 edition) and pins
  the toolchain to 1.95; folding it into wraith's `resolver = "2"`
  workspace would require dep-version alignment on ~20 oxc crates plus
  a stable-1.95 host.

For now, `wraith-cli` shells out to a `fallow` binary if one is on
PATH and merges its JSON findings via `wraith_core::report::FindingKind::External`.
When the host toolchain bumps to 1.95+ and we move to a workspace-per-
language layout, the shim in `crates/wraith-cli/src/fallow.rs` should
be replaced by direct calls into `fallow-core` / `fallow-extract`.

Pull updates:

```bash
git subtree pull --prefix=packages/wraith/vendor/fallow fallow main --squash
```

## Run

```bash
cd packages/wraith
cargo build
./target/debug/wraith --help
./target/debug/wraith --root path/to/your/cargo-workspace dead-code
./target/debug/wraith --root . unused-deps --format json
./target/debug/wraith audit --exit-zero
./target/debug/wraith circular-deps
./target/debug/wraith dupes
./target/debug/wraith health
./target/debug/wraith fix --apply         # writes
./target/debug/wraith watch               # streams jsonl on save
```

## Claude Code skill

A `wraith` skill ships with the repo at [`skills/wraith/`](skills/wraith/).
It teaches AI agents (and humans) when to reach for which wraith
subcommand and how to compose them into refactor workflows.

Install for your Claude Code:

```bash
ln -sfn "$(pwd)/skills/wraith" ~/.claude/skills/wraith
```

The skill is also a useful read on its own — `SKILL.md` is a decision
tree mapping intents ("I need to find code", "this fn is too complex")
to wraith commands, and `rules/` has deep dives on navigation,
refactoring, diff reports, and monorepo usage.

## Wraith on wavelet — real-world demo

Wraith was developed against [`packages/wavelet`](../wavelet) — the
workbooks motion-graphics renderer (~71k LOC, 242 source files, one
large Rust crate). The session that built out wraith's full surface
also ran wraith against wavelet to validate it. Here's what
`wraith report --since=<pre-session-commit>` produced at the end:

### Before / after diff

```
$ cd packages/wavelet
$ wraith report --since=9da581812
```

| metric | before (`9da581812`) | after (HEAD) | delta |
|---|---:|---:|---:|
| Source files | 242 | 242 | +0 |
| Lines of code | 71,542 | 71,376 | **−166** |
| Total findings | 87 | 72 | **−15** |
| Findings resolved | — | — | **28** |
| Findings introduced | — | — | 13 |
| Findings unchanged | — | — | 59 |

Plus the raw `git diff` (path-scoped): **20 files changed, +245 / −412 lines**.

### What was resolved

| detector | resolved |
|---|---:|
| `dead-code` | 13 (run_batch fn + voices catalog + agent error codes + 1 unused-render-fn) |
| `dupes (clusters)` | 13 (audio MIME quartet, image MIME quintuple, `read_region` pair) |
| `unused-deps` | 1 (`animato` — declared but never imported) |
| `health` | 1 (`run_turn` cyclo 30 → under threshold via extract-into-helpers) |

### What this validates about wraith

| claim | proof on wavelet |
|---|---|
| Zero false positives on `dead-code` | All 14 auto-removed items kept the build green |
| `Cargo.toml [package]` never touched by `fix --apply` | Pre/post diff confirms section byte-identical (regression test `wb-5lgj.21`) |
| Resolver handles real crate-level patterns | wavelet's 71k LOC produces only **2 real cycles** after the leaf-name disambiguation in `wb-5lgj.24` — before that fix, the same crate produced a phantom 148-node SCC |
| Dupes detector finds structural clones, not just text-identical | `assert_color_band_mean` cluster (3 fns at 0.86–0.94 sim) is a real shader-assert boilerplate pattern; `looks_like_image` and `looks_like_video` were flagged as byte-identical but refused safely (scope-aware: they reference different `SUPPORTED_EXTS`) — caught the false positive `wb-5lgj.37` from real usage |
| Diff reports actually narrate sessions | The table above was produced by `wraith report --since=<ref>`. No manual aggregation. |

### Wraith found bugs in itself (caught via self-use during this session)

| bug | severity | how it surfaced | status |
|---|---|---|---|
| `circular-deps` 148-node phantom SCC (synthetic "crate" root) | P2 | wraith ran against wavelet | fixed in `wb-5lgj.20` |
| `fix --apply` destroys `Cargo.toml [package]` | **P1** | applying to wavelet broke the build | fixed in `wb-5lgj.21` |
| Resolver leaf-name false positives (`.new()` matching across modules) | P2 | second wave of circular-deps FPs | fixed in `wb-5lgj.24` |
| `dedupe-cluster` scope-blind on module-local consts | **P1** | wavelet `looks_like_image`/`looks_like_video` | fixed in `wb-5lgj.37` |
| `dedupe-cluster` doesn't elevate visibility of canonical | P2 | wavelet `extract_grounded_text` pair (canonical was private) | fixed in `wb-5lgj.38` |
| `suggest-extractions` emits ranges `extract-fn` v1 refuses | P2 | wavelet `run_image` (match-arm-bodies) | fixed in `wb-5lgj.39` |

Six real bugs caught by eating our own dog food on a real codebase, all fixed in the same session.

## Wraith on the monorepo

Wraith analyzes one Cargo workspace at a time. Across this monorepo:

| workspace | crates | source files | LOC | findings | top finding |
|---|---:|---:|---:|---:|---|
| `packages/wavelet/` | 1 | 242 | 71,376 | 72 | `run_image` cyclo=145 |
| `packages/wraith/` | 4 | 37 | 17,997 | 54 | self-found: 8 dead-code, 4 unused-deps, 14 dupes, 28 complexity |
| `packages/orchestrator-core/` | 1 | 11 | 1,965 | 2 | trivially clean — 1 dead-code, 1 dupe |

Run wraith on any workspace by `cd`-ing into it, or with `--root`:

```bash
wraith --root packages/wraith report
wraith --root packages/wavelet report --since=main~10
```

See [`skills/wraith/rules/monorepo.md`](skills/wraith/rules/monorepo.md) for patterns.

## What gets restructured

When wraith identifies and the agent acts on its suggestions, the **directory shape** changes. From this session's wavelet work:

| pattern | before | after |
|---|---|---|
| MIME extension helpers | 8 local `pick_ext` / `mime_to_ext` / `guess_mime` fns scattered across `backends/{google,fal,elevenlabs}/` | 2 shared helpers (`pick_image_ext_from_mime`, `pick_audio_ext_from_mime`) in `backends/util.rs` |
| Region parsing | 2 near-identical `read_region` / `read_region_yaml` fns in `validators/shader.rs` | 1 shared `region_from_floats` helper + 2 thin format adapters |
| Agent turn loop | 1 god-function `run_turn` at 200 lines, cyclo=30 | 1 thin coordinator (65 lines) + 4 single-concern helpers (`initialize_plan`, `initialize_system_prompt`, `check_step_termination`, `handle_text_response`, `dispatch_one_call`) |
| Dead code | 14 unused pub items scattered (voices catalog, agent error codes, unused fn `run_batch`, etc.) | gone |

Wavelet's largest remaining files (the next refactor targets wraith identifies):

```
8007 src/bin/wavelet.rs          ← god file; wraith health surfaces run_image cyclo=145 and 9 other handlers >50 cyclo
1607 src/css_filter.rs           ← wraith hijack_filters_in_html @ cyclo=49 cog=126
1141 src/render_offline.rs
1150 src/agent/tools/plan_tools.rs
 863 src/variants.rs
```

`bin/wavelet.rs` at 8000 lines is the obvious next break-up target. Wraith's
`refactor move-fn` makes the work straightforward: each handler relocates
to `src/bin/handlers/<topic>.rs` with all callers + `use` paths updated
automatically.

## Tests

```bash
cargo test --workspace
```

**95+ tests passing** across the wraith workspace — integration tests spawn
synthetic Cargo workspaces in `tempfile::tempdir()`s, invoke the `wraith`
binary, and assert on stdout. Plus unit tests in core crates and protocol
smoke tests for `wraith-lsp` and `wraith-mcp`.

Test count growth tracked the feature growth:
- MVP: 7 tests
- After `.4`-`.19` epic: 23 tests
- After all `.20`-`.39` agent-driven refactor tickets: 95 tests

## Beads tickets

- Epic: `wb-5lgj`
- **Closed (39)**: `.1`-`.36` MVP + LSP/MCP/extract-fn-v1 + clustering + visibility + cache + graph queries + token-economy + dedupe-cluster + diff-cluster/extract-shared. `.27` extract-fn v2, `.31` move-fn/rename/inline/split-fn, `.37` scope-aware dedupe, `.38` visibility-elevation, `.39` suggest-extractions feasibility filter.
- All P1 bugs caught from self-use have been fixed.
