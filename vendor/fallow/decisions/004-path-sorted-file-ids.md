# ADR-004: Path-sorted FileIds

**Status:** Accepted
**Date:** 2024-06-01

## Context

Every discovered file gets a `FileId(u32)` used as an index throughout the pipeline: edge arrays, cross-reference tables, result sets. The question is whether FileIds should be assigned in discovery order (filesystem walk order) or sorted by path.

Filesystem walk order varies across platforms (macOS HFS+ returns alphabetically, Linux ext4 returns by inode, Windows NTFS returns alphabetically but with case differences). This means the same project analyzed on different OSes would produce different FileIds, making cross-platform caching, test snapshots, and debugging harder.

## Decision

Sort discovered files by path before assigning FileIds. `FileId(0)` is always the lexicographically first path, `FileId(1)` the second, and so on.

```rust
/// Compact file identifier.
///
/// A newtype wrapper around `u32` used as a stable index into file arrays.
/// FileIds are path-sorted (not insertion order) for stable cross-run identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);
const _: () = assert!(std::mem::size_of::<FileId>() == 4);
```

## Alternatives considered

### Insertion order (filesystem walk order)

Assign FileIds as files are discovered.

- Pros: no sorting step, simpler
- Cons: non-deterministic across platforms, cache invalidation when unrelated files are added/removed (all subsequent IDs shift differently depending on walk order)

### Path hashing

Use a hash of the path as the FileId.

- Pros: stable identity independent of other files, no sorting needed
- Cons: hash collisions require handling, can't use as direct array indices (sparse), 32-bit hash has non-trivial collision probability at 50k+ files

## Consequences

**Positive:**
- Identical FileIds across platforms for the same project state
- Adding a file only shifts IDs for files lexicographically after it (predictable cache invalidation)
- Debug output is reproducible: FileId values correspond to alphabetical position
- Compile-time size assertion (4 bytes) keeps the type cache-friendly as an array index

**Negative:**
- Requires sorting all discovered files before the pipeline can proceed (negligible cost: sorting 10k paths takes microseconds)
- FileIds change when files are renamed (acceptable since the file content changed too)
