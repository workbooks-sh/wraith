# Wraith: monorepo usage

Wraith analyzes one Cargo workspace at a time. In a monorepo with multiple Rust crates / workspaces, you have two options:

## Option 1: per-workspace invocation (recommended)

Run wraith once per workspace, in each workspace's root:

```bash
cd packages/wavelet && wraith report
cd packages/wraith && wraith report
cd vendor/rvst && wraith report
```

Each run is isolated. Each gets its own `.wraithrc.cache` (gitignored at the package level). Each surfaces findings scoped to that workspace's symbol table.

**When to use**: most of the time. Workspaces are usually independent enough that mixing their findings adds noise rather than signal.

## Option 2: explicit `--root <path>`

If you want to invoke from a single shell without `cd`:

```bash
wraith --root packages/wavelet report
wraith --root packages/wraith report
```

Equivalent to option 1 but lets you script multi-package runs:

```bash
# Aggregate report.md per workspace:
for ws in packages/wavelet packages/wraith vendor/rvst; do
    name=$(basename "$ws")
    wraith --root "$ws" report > "reports/${name}.md"
done
```

## What wraith does NOT do (yet)

There's no `wraith --monorepo` mode that walks `**/Cargo.toml` and produces a unified report. Reasons:

- Cross-workspace finding identity is ambiguous (same fn name in two workspaces shouldn't be a dupe finding)
- Each workspace has its own `.wraithrc.json` — overriding which config wins is complex
- Boundary rules (`.wraithrc.json` `boundaries`) are per-workspace by design

If you need a monorepo-wide narrative, the recommended pattern is:

1. Run `wraith report` per workspace
2. Use a shell loop or Makefile to aggregate the outputs
3. Manually compose a top-level "monorepo health" summary

A future `wraith --monorepo` flag may automate this — file as a wraith ticket if/when it becomes a frequent pain point.

## Diff reports in a monorepo

`wraith report --since=<ref>` works fine per-workspace. The temp worktree it creates is under the monorepo root's `.claude/wraith-diff/<sha>/`, and the workspace-relative path is preserved:

```bash
cd packages/wavelet
wraith report --since=<ref>     # worktree at <monorepo>/.claude/wraith-diff/<sha>/packages/wavelet/
```

Just make sure the monorepo root has `.claude/wraith-diff/` in `.gitignore`.

## Subtree-mirrored crates

For crates that are `git subtree`s of upstream repos (like `vendor/rvst`, `vendor/colorwave`, `packages/wraith` itself, `packages/wavelet`):

- Run wraith inside the subtree's path normally — works identically to a non-vendored crate
- The `.wraithrc.cache` stays local; it's gitignored at the subtree's root
- When `git subtree push`-ing upstream, the cache and `.wraithrc.json` config travel with the crate (so the upstream repo gets wraith integration "for free")

## Multi-language monorepos

If your monorepo also has TypeScript/JavaScript:

- Wraith's vendored `fallow` subprocess handles TS/JS analysis automatically when you run against a workspace that contains `.ts`/`.js` files
- Findings come back tagged with `kind: External { source: "fallow" }` in the unified `Finding` schema
- Same severity buckets, same `--format json` output shape — agent-friendly across languages

## Practical patterns

**"What's the health of every Rust workspace in this monorepo?"**
```bash
    echo "=== $ws ==="
    wraith --root "$ws" report | head -20
    echo
done
```

**"Run wraith fix --apply across all workspaces (with care):"**
```bash
for ws in $(find . -name "Cargo.toml" -not -path "*/target/*" -not -path "*/wraith-diff/*" | xargs -n1 dirname); do
    # Each workspace gets its own analysis
    cd "$ws" && wraith fix --apply
done
```

(But always `--dry-run` first on any unfamiliar workspace.)
