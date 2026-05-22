# Wraith: refactor primitives

All under `wraith refactor`. Each takes inputs from the agent (you) and does the multi-file syntactic legwork.

## Picking the right primitive

| You want to… | Use |
|---|---|
| Extract a code block into a new fn | `extract-fn` |
| Collapse byte-identical duplicate fns onto one canonical | `dedupe-cluster` |
| Unify similar-but-not-identical clones (e.g. literal differences only) | `diff-cluster` then `extract-shared` |
| Move a fn between modules / crates | `move-fn` |
| Rename a symbol workspace-wide | `rename` |
| Inline a fn's body at each call site, delete the fn | `inline` |
| Split one fn into two pipeline stages | `split-fn` |

## `extract-fn` (v2)

```bash
wraith refactor extract-fn <file>:<start>..<end> --name <new_fn> [--dry-run]
```

Extracts a contiguous range into a new function. Handles patterns v1 refused:
- early `return` → extracted fn returns `Option<T>`, callsite does `if let Some(v) = …() { return v; }`
- `?` operator → extracted fn returns `Result<T, E>`, callsite chains `?`
- `.await` → extracted fn becomes `async fn` (refuses if outer not async)
- `self.` / `&self` / `&mut self` → extracted as `Self::` assoc fn taking `&self` or `&mut self`
- match-arm-body → extracted, replaces the arm with a call

**Pattern**: drive `extract-fn` with `wraith health --fn X --suggest-extractions --format=json`. Wraith ranks candidates by self-containment score and pre-checks feasibility — every emitted suggestion comes with a ready-to-run `extract_fn_command`. You pick which to apply.

## `dedupe-cluster`

```bash
wraith refactor dedupe-cluster <cluster-id-or-symbol> [--canonical=<spec>] [--dry-run]
```

For clusters where members are **byte-identical**: pick one canonical, replace others with calls to it. Auto-elevates canonical's visibility to `pub(crate)` if needed (pass `--no-elevate` to disable).

**Safety**: refuses dedupe when members reference module-local constants with diverging definitions (e.g., `looks_like_image` and `looks_like_video` both reference `SUPPORTED_EXTS`, defined differently per file). Refusal message points you at `extract-shared` instead.

**Don't trust blindly**: `--dry-run` first, especially on shader/backend code where module-local consts are common.

## `diff-cluster` + `extract-shared`

When a cluster is similar-but-not-identical (similarity 0.85-0.99):

```bash
# 1. See what differs across members:
wraith refactor diff-cluster <cluster-id> --format=json

# 2. Decide a unified signature based on the divergences

# 3. Execute the unification:
wraith refactor extract-shared <cluster-id> \
  --signature='fn pick_ext(mime: &str) -> &str' \
  --param-mapping='{"member_0": {"arg0": "jpg"}, "member_1": {"arg0": "webp"}}' \
  --extract-to=crate::util
```

`diff-cluster` shows token-level divergences with inferred type hints. `extract-shared` takes your supplied signature + per-member parameter mapping and does the multi-file rewrite. Refuses if your mapping doesn't account for all divergences.

**This is the agent-driven workflow par excellence**: wraith analyses; you decide the abstraction; wraith executes.

## `move-fn`

```bash
wraith refactor move-fn <src-file:fn-name> --to=<dst-module-path> [--dry-run]
```

Deletes from src, creates dst module if missing, updates all workspace `use` imports + fully-qualified call sites, auto-elevates visibility. Use for relocating utility functions to a shared module (`backends::util`, etc.).

## `rename`

```bash
wraith refactor rename <symbol> --to=<new-name> [--dry-run]
```

Workspace-wide rename of one symbol. Updates definition + all references in one pass. Refuses collisions and trait methods (those need a more careful pass).

## `inline`

```bash
wraith refactor inline <src-file:fn-name> [--dry-run]
```

Replaces every call site with the fn body (param-to-arg substitution), deletes the original. Refuses generic fns, recursive fns, fns with non-tail return / break-to-outer / `?`.

Use sparingly — inlining can make a hot loop's body unreadable. Best applied to one-liner helpers that don't justify the indirection.

## `split-fn`

```bash
wraith refactor split-fn <src-file:fn-name> --at-line=<N> --names=<first>,<second> [--dry-run]
```

Splits at line N. First fn gets the captured args; second takes first's return values. Original callsite becomes `second(first(...))`. Refuses if N isn't at a statement boundary.

Use when a fn naturally has two phases — e.g. "validate input, then process" or "fetch, then render".

## Common workflow patterns

**The cluster cleanup pass:**
```
wraith dupes
# For each 1.00-similarity 2-member cluster:
wraith refactor dedupe-cluster <id> --dry-run
# Verify safety, then apply (remove --dry-run)
wraith refactor dedupe-cluster <id>
```

**The complexity reduction pass:**
```
wraith health
# Pick worst hotspot:
wraith --format json health --fn wavelet::run_image --suggest-extractions
# Pick top suggestion, execute:
wraith refactor extract-fn src/bin/wavelet.rs:2540..2587 --name handle_image_variants
```

**The shared-utility extraction pass:**
```
wraith dupes
# For a cluster with literal-only differences (e.g., MIME mapping):
wraith refactor diff-cluster <id>
# Read the divergence output, design a signature
wraith refactor extract-shared <id> --signature=... --param-mapping=... --extract-to=...
```

**The big-file restructure** (god-file → multiple files):
```
wraith summarize src/bin/wavelet.rs   # See the pub interface + sub-systems
# Pick a coherent subset of fns to extract:
wraith refactor move-fn src/bin/wavelet.rs:handle_image --to=src::bin::handlers::image
# Repeat for each coherent group
```

## Always `--dry-run` first

Every primitive supports `--dry-run`. Use it before `--apply` (or before the version-without-`--dry-run` that writes). Reviewing the planned edits is a few seconds; reverting an unwanted multi-file edit can be hours.
