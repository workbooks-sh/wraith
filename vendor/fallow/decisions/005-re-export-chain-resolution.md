# ADR-005: Re-export chain resolution

**Status:** Accepted
**Date:** 2024-06-01

## Context

JavaScript/TypeScript projects commonly use barrel files (`index.ts`) that re-export from internal modules:

```ts
// lib/index.ts
export { formatDate } from './utils';
export { Button } from './components/Button';
```

If `formatDate` is imported via `lib/index.ts` by consumers, the original export in `./utils.ts` must not be reported as unused. Without re-export chain resolution, every export behind a barrel file would be a false positive.

Multi-level chains are common in large codebases: `app.ts` imports from `lib/index.ts`, which re-exports from `lib/utils/index.ts`, which re-exports from `lib/utils/date.ts`. The chain can be arbitrarily deep.

## Decision

Use iterative propagation through re-export edges with cycle detection. After building the initial module graph, a separate pass walks re-export edges and propagates usage information from consumers back through barrel files to the original export site.

The algorithm:

1. Build the module graph with import edges and re-export edges as separate edge types
2. Iteratively propagate: for each re-export edge, if the re-exported name is consumed downstream, mark the original export as used
3. Repeat until no new propagations occur (fixed-point iteration)
4. Detect cycles in re-export chains to prevent infinite loops (e.g., `a/index.ts` re-exports from `b/index.ts` which re-exports from `a/index.ts`)

`export *` (namespace re-exports) propagate all exports from the source module, resolved transitively through multiple levels.

## Alternatives considered

### Recursive resolution at query time

Resolve chains lazily when checking if an export is used.

- Pros: no upfront propagation cost, simple for shallow chains
- Cons: repeated traversal for each export, O(exports * chain_depth) worst case, harder to handle cycles correctly, memoization complexity

### Graph rewriting (inline re-exports)

Rewrite the graph so that `import { X } from 'barrel'` becomes a direct edge to the original file.

- Pros: subsequent analysis sees a simple graph, no chain awareness needed
- Cons: loses the barrel file structure (needed for reporting), complex rewriting logic for `export *`, harder to maintain edge metadata (line numbers, type-only flags)

## Consequences

**Positive:**
- Zero false positives from barrel files (the most common source of false positives in dead code tools)
- Fixed-point iteration handles arbitrary chain depth without recursion limits
- Re-export edges are preserved in the graph for debugging (`--trace` shows the full chain)
- Cycle detection prevents hangs on pathological re-export patterns

**Negative:**
- Additional pass over the graph adds to analysis time (typically <5% of total runtime)
- `export *` propagation is conservative: if any export from the source module is consumed via the barrel, all are considered reachable through that path
