# ADR-003: FxHashMap over std HashMap

**Status:** Accepted
**Date:** 2024-06-01

## Context

Fallow uses hash maps extensively: module lookup by path, export-to-import cross-referencing, symbol resolution, plugin detection. Standard `HashMap` uses SipHash for DoS resistance, which is unnecessary for an offline analysis tool where all keys are trusted (file paths, export names).

Additionally, standard `HashMap` iteration order is randomized per-run, which makes test output non-deterministic and snapshot testing unreliable.

## Decision

Disallow `std::collections::HashMap` and `std::collections::HashSet` project-wide. Enforce via Clippy's `disallowed-types` configuration in `.clippy.toml`. Use `FxHashMap` / `FxHashSet` from the `rustc_hash` crate everywhere.

```toml
# .clippy.toml
disallowed-types = [
    { path = "std::collections::HashMap", reason = "Use rustc_hash::FxHashMap" },
    { path = "std::collections::HashSet", reason = "Use rustc_hash::FxHashSet" },
]
```

## Alternatives considered

### Standard HashMap with fixed seed

Use `HashMap` with `BuildHasherDefault<FxHasher>` for determinism while keeping the std API.

- Pros: familiar API, no new crate
- Cons: verbose type aliases, easy to accidentally use the default hasher, still pays for `RandomState` construction in any unqualified `HashMap::new()`

### IndexMap

Preserves insertion order, uses hash-based lookup.

- Pros: deterministic iteration, good for ordered output
- Cons: slower than FxHashMap for pure lookup workloads, larger memory footprint per entry, insertion order is not the same as sorted order (still need explicit sorting for path-ordered output)

### AHash

Alternative fast hasher used by hashbrown.

- Pros: potentially faster on some workloads, good distribution
- Cons: less battle-tested in the Rust compiler ecosystem, FxHash is the established choice in rustc/oxc tooling

## Consequences

**Positive:**
- Measurably faster hashing for short string keys (file paths, export names)
- No per-run seed randomization: same insertion order always produces the same iteration order, improving test stability
- Clippy enforcement means no accidental use of std HashMap anywhere in the codebase
- Consistent with the Oxc ecosystem (which also uses FxHashMap)

**Negative:**
- Contributors must remember to use `FxHashMap`/`FxHashSet` (Clippy catches violations at compile time)
- FxHash has poor distribution for certain key patterns (not relevant for string keys in our use case)
- Adds a crate dependency (`rustc_hash`), though it is small and widely used
