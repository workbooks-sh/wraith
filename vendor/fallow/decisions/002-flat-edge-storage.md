# ADR-002: Flat edge storage

**Status:** Accepted
**Date:** 2024-06-01

## Context

The module graph is the central data structure in fallow. Every file is a node; every import statement creates an edge. For a large monorepo with 10,000+ files and 50,000+ edges, how these edges are stored determines cache performance during graph traversal, cycle detection, and dead code analysis.

## Decision

Store all edges in a single contiguous `Vec<Edge>` with range indices per node, rather than per-node adjacency lists.

Each `ModuleNode` stores a `Range<u32>` pointing into the shared edge vector. To iterate a node's outgoing edges, slice `edges[range.start..range.end]`. The `Edge` struct is kept small (32 bytes, enforced by a compile-time size assertion) to maximize cache line utilization.

```rust
pub(super) struct Edge {
    pub(super) source: FileId,
    pub(super) target: FileId,
    pub(super) symbols: Vec<ImportedSymbol>,
}
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<Edge>() == 32);
```

## Alternatives considered

### Per-node Vec adjacency lists

Each `ModuleNode` owns a `Vec<Edge>`.

- Pros: simpler to build incrementally, natural per-node ownership
- Cons: scattered heap allocations, poor cache locality during full-graph traversals (cycle detection, transitive usage), higher memory fragmentation

### Petgraph or similar graph library

Use an existing Rust graph library.

- Pros: well-tested, generic algorithms included
- Cons: generic abstractions add indirection, edge metadata (imported symbols) doesn't fit standard graph edge models, harder to optimize memory layout for our specific access patterns

### Arena-based allocation

Typed arena for edges with index-based references.

- Pros: good locality, no per-edge allocation
- Cons: more complex lifetime management, marginal benefit over a plain Vec with range indices

## Consequences

**Positive:**
- Cache-friendly sequential traversal: cycle detection and dead code analysis iterate edges linearly
- Predictable memory layout: one allocation for all edges, no fragmentation
- Compile-time size enforcement prevents accidental struct bloat from breaking performance
- Simple indexing: `edges[node.edge_range]` is zero-cost

**Negative:**
- Edges must be fully constructed before the graph is queryable (no incremental insertion after building)
- Reordering or removing edges requires rebuilding ranges (not needed in practice since the graph is built once per analysis run)
