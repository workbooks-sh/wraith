# ADR-006: Hidden directory allowlist

**Status:** Accepted
**Date:** 2024-06-01

## Context

By convention, directories starting with `.` are hidden and typically contain tooling configuration (`.git`, `.cache`, `.next`, `.nuxt`) or build artifacts. Most hidden directories should be skipped during file discovery to avoid analyzing generated code, caches, or version control internals.

However, some hidden directories contain user-authored source files that are part of the project:

- `.storybook/` contains Storybook configuration and story wrapper files
- `.vitepress/` contains VitePress theme customizations and config
- `.well-known/` is a web standard directory for discovery files
- `.changeset/` contains changeset configuration
- `.github/` contains GitHub Actions workflows with script references

Skipping these would cause false positives (files reported as unused because their importers were never discovered).

## Decision

Maintain an explicit allowlist of hidden directories that are traversed during file discovery. All other dotdirs are skipped.

```rust
const ALLOWED_HIDDEN_DIRS: &[&str] = &[
    ".storybook",
    ".vitepress",
    ".well-known",
    ".changeset",
    ".github",
];
```

The allowlist is enforced by exhaustiveness tests: a count assertion, uniqueness check, dot-prefix validation, and no-trailing-slash check ensure the list stays well-formed.

Hidden files (`.eslintrc.js`, `.prettierrc.ts`) are always discovered regardless of the allowlist, since they are handled by the type filter (source extensions only) rather than directory filtering.

## Alternatives considered

### Skip all hidden directories

Simple rule: if it starts with `.`, skip it.

- Pros: simplest implementation, no maintenance burden
- Cons: misses `.storybook` and `.vitepress` source files, causing false positives. These are common enough that users would need per-project configuration to work around it

### Allow all hidden directories

Traverse everything, rely on `.gitignore` and explicit ignore patterns.

- Pros: no allowlist maintenance, users control exclusion
- Cons: `.git` (hundreds of MB of objects), `.next`/`.nuxt` (build caches with generated files), `.cache` would all be traversed, dramatically slowing discovery and polluting results with generated code

### User-configurable hidden directory list

Let users specify which hidden dirs to include via config.

- Pros: maximum flexibility
- Cons: every user must configure this, common directories like `.storybook` should just work. Config-based approach adds friction for the 95% case

## Consequences

**Positive:**
- Zero-config correctness for projects using Storybook, VitePress, Changesets, or GitHub workflows
- Fast discovery: skips `.git`, `.next`, `.nuxt`, `.cache`, and other large hidden directories
- Exhaustiveness tests prevent the list from silently growing stale or malformed

**Negative:**
- New framework-standard hidden directories require a fallow release to add to the allowlist
- Edge case: if a project uses a non-standard hidden directory for source files (e.g., `.custom-scripts/`), those files are not discovered. Workaround: use the `entry` config field or `dynamicallyLoaded` patterns
