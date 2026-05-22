---
name: wraith
description: Use when you need to analyze, navigate, clean up, or refactor a Rust or TypeScript/JavaScript codebase. Wraith is the editing engine — you decide what to do, wraith does the multi-file syntactic legwork without burning your token context on read+edit cycles.
metadata:
  tags: wraith, rust, refactoring, codebase-analysis, dead-code, complexity, dupes
---

# Wraith Development Skill

Wraith is a Rust-native codebase analyzer + agent-driven refactoring tool. It runs against a Cargo workspace and exposes everything an AI agent needs to diagnose, navigate, and clean up code without manual multi-file edits.

The principle: **you are the brain, wraith is the editing engine.**

## Core mental model

When you need to do anything to a Rust (or TS/JS) codebase, there's almost certainly a `wraith` subcommand that:
- gives you the structured data you need (no grep / no whole-file Reads), OR
- performs the syntactic edits for you (no manual multi-file editing)

Default to invoking wraith. Manual edits are the fallback when wraith can't do it.

## Decision tree

**"I want to know what's wrong with this codebase."**
→ `wraith report` — full markdown summary of every detector. Then drill into specific findings.

**"I want to see what a recent change/PR/session actually improved."**
→ `wraith report --since=<ref>` — diff report. Resolved vs introduced findings, LOC delta, complexity delta. Perfect for PR descriptions.

**"I need to find / understand code without burning tokens reading files."**
→ See [rules/navigate.md](./rules/navigate.md) — `ctx`, `summarize`, `ls`, `graph callers/callees/blast-radius`.

**"I want to clean up dead code or unused dependencies."**
→ `wraith fix --apply` — safe auto-fix. Guards against destroying `Cargo.toml [package]`. Only touches `[dependencies]` / `[dev-dependencies]` / `[build-dependencies]`.

**"I want to dedupe near-identical functions."**
→ See [rules/refactor.md](./rules/refactor.md) — `dedupe-cluster` (byte-identical) or `diff-cluster` + `extract-shared` (similar with differences).

**"This function is too complex; help me break it up."**
→ `wraith health --fn <full::path> --suggest-extractions --format json` → pick a suggestion → `wraith refactor extract-fn <file>:<start>..<end> --name <new>`.

**"I need to move/rename/inline/split a function."**
→ `wraith refactor move-fn` / `rename` / `inline` / `split-fn`. All are multi-file-aware and update call sites.

**"I want to tighten visibility (pub → pub(crate) → private)."**
→ `wraith visibility` — walks the reference graph, suggests the tightest visibility per pub item. `--apply` to rewrite.

**"I want to manage Cargo deps."**
→ `wraith deps duplicates` (same crate at multiple versions), `deps audit` (RustSec CVEs), `deps unused-features`, `deps size`.

**"Are there circular dependencies?"**
→ `wraith circular-deps` — Tarjan SCC over module + crate graphs. Post wb-5lgj.20+.24 fixes, this is reliable.

## How wraith makes you faster

| Without wraith | With wraith |
|---|---|
| `grep -rn fn run_` then read 10 files | `wraith ls "run_*" --kind=fn --format=json` (one structured response) |
| Read the whole file to understand a fn | `wraith ctx <symbol> --no-body` (def + imports + top callers + top callees) |
| Read 200 lines to find extraction candidates | `wraith health --fn X --suggest-extractions --format=json` (ranked list of feasible extractions, each with the exact `extract-fn` command ready to run) |
| Manually identify dupes by reading similar code | `wraith dupes` (transitive clustering, similarity-scored) |
| Multi-file rename via sed + careful re-checks | `wraith refactor rename <symbol> --to=<new>` |
| Multi-file fn move + update all imports | `wraith refactor move-fn <src:fn> --to=<dst-mod>` |
| Write a PR description listing what you fixed | `wraith report --since=<base-ref>` |

## Full command reference

| Command | What it does |
|---|---|
| `wraith report [--since=<ref>]` | All-detector summary (or before/after diff) |
| `wraith audit [--exit-zero]` | dead-code + unused-deps on git-changed files (pre-commit) |
| `wraith dead-code` | Pub items with no references |
| `wraith unused-deps` | Cargo.toml deps with no `use` |
| `wraith circular-deps` | Module + crate cycles via Tarjan SCC |
| `wraith dupes [--pairs]` | Transitive duplicate clusters (or raw pairs) |
| `wraith health [--fn X] [--show-branches \| --suggest-extractions]` | Complexity hotspots, branch trees, extraction candidates |
| `wraith boundaries` | Module-import allow/deny rules |
| `wraith visibility [--apply]` | Suggest pub → pub(crate) tightening |
| `wraith deps {duplicates,audit,unused-features,size}` | Crate logistics |
| `wraith graph {callers,callees,blast-radius,crate-deps,reverse-deps}` | Reference graph queries |
| `wraith ctx <symbol>` | Smallest useful context window (def + callers + callees) |
| `wraith summarize <file>` | Per-file structured summary |
| `wraith ls [pattern] --kind=<kind>` | Symbol listing |
| `wraith fix [--apply]` | Safe auto-fix (dead pub items + unused deps); Cargo.toml-guarded |
| `wraith refactor extract-fn <file>:<start>..<end> --name <new>` | Function extraction (v2: handles return/?/await/self./match-arm) |
| `wraith refactor dedupe-cluster <id>` | Collapse byte-identical cluster onto canonical |
| `wraith refactor diff-cluster <id>` | Structured cluster divergence (read-only) |
| `wraith refactor extract-shared <id> --signature=... --param-mapping=...` | Agent-driven unification of similar-but-not-identical clusters |
| `wraith refactor move-fn <src:fn> --to=<dst-mod>` | Workspace-wide fn relocation |
| `wraith refactor rename <symbol> --to=<new>` | Workspace-wide symbol rename |
| `wraith refactor inline <src:fn>` | Replace call sites with body |
| `wraith refactor split-fn <src:fn> --at-line=N --names=a,b` | Split a fn at a line |
| `wraith init [--ci=github\|gitlab]` | Scaffold `.wraithrc.json` |
| `wraith hooks install` | git pre-commit + Claude Code hook |
| `wraith migrate --from clippy\|deny` | Translate clippy.toml / deny.toml |
| `wraith watch` | File-save jsonl stream |

All commands support `--format json | jsonl | human` (default human). Exit codes: `0` no findings, `1` findings, `2` internal error, `64` missing optional binary or invalid input.

## The agent-driven refactor loop

This is the killer workflow. Use this for any "the codebase needs cleanup" task:

```
1. wraith report                    → identify worst hotspots
2. wraith dupes                     → see what dedup opportunities exist
3. For each cluster (byte-identical):
     wraith refactor dedupe-cluster <id>
4. For each cluster (similar):
     wraith refactor diff-cluster <id>    → see divergences
     wraith refactor extract-shared <id> --signature=... --param-mapping=...
5. For each high-complexity fn:
     wraith health --fn X --suggest-extractions --format=json
     wraith refactor extract-fn <file>:<range> --name <new>
6. wraith visibility --apply        → tighten over-pub
7. wraith fix --apply               → final dead-code/unused-dep sweep
8. wraith report --since=<start-ref> → narrate the wins
```

Every step is a tool invocation. You never `grep`, you never read whole files for cleanup, you never write a multi-file edit by hand.

## When NOT to reach for wraith

- **Adding a feature** — wraith is a maintenance tool, not a feature-development one. Write the code first; clean up with wraith later.
- **Architectural redesigns** — wraith can identify cycles but the strategy to break them is yours. Same with picking abstractions.
- **TS/JS heavy work** — wraith dispatches `.ts`/`.js` to vendored `fallow` via subprocess. Less optimized than the Rust path. For TS/JS-only projects, use `fallow` directly.

## Related deep dives

- [rules/navigate.md](./rules/navigate.md) — token-economy queries (`ctx`, `summarize`, `ls`, `graph`)
- [rules/refactor.md](./rules/refactor.md) — the refactor primitives in depth
- [rules/diff-reports.md](./rules/diff-reports.md) — using `--since` for PRs and session wins
- [rules/monorepo.md](./rules/monorepo.md) — running wraith across multiple Cargo workspaces in a monorepo

## Performance notes

- Wraith caches the workspace graph in `.wraithrc.cache`. Add it to `.gitignore`. Warm runs are 3-5x faster than cold.
- Each `wraith` invocation runs one analysis pass. If you chain many commands, they share the cache.
- The `diff` report creates a temporary git worktree at `.claude/wraith-diff/<sha>/`. This is gitignored at the monorepo root. Re-runs against the same `<ref>` reuse the worktree (and its warm cache).

## Resolution limits

Wraith uses `syn` (not rustc internals) — name-based resolution, intentionally not type-aware. Trade-offs are false-negatives, not false-positives:

- A `pub fn foo` collides with any other `foo` in scope; if either is referenced, both look alive.
- Macro-generated items aren't seen.
- Items inside `mod tests` are skipped wholesale.
- `cfg(...)` blocks other than `cfg(test)` are walked unconditionally.

When in doubt about a finding, verify it manually before acting on `--apply`.
