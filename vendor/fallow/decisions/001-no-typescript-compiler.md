# ADR-001: No TypeScript compiler

**Status:** Accepted
**Date:** 2024-06-01

## Context

Fallow needs to analyze TypeScript/JavaScript codebases to find unused code, circular dependencies, and complexity hotspots. The central question: should fallow invoke the TypeScript compiler (tsc) for full type resolution, or rely on syntactic analysis only?

Tools like knip (pre-v6) used TypeScript's compiler API for import resolution and type information. This provides accurate type-level analysis but comes with significant costs: startup time (tsc bootstraps the entire program), memory consumption (full type-checker state), and tight coupling to TypeScript versions.

## Decision

Use the Oxc parser ecosystem (oxc_parser + oxc_semantic + oxc_resolver) for purely syntactic analysis. No type resolution, no tsc dependency.

Oxc provides:

- **oxc_parser**: fast, error-tolerant parsing of TS/JS/JSX/TSX
- **oxc_semantic**: scope-aware binding analysis (which symbols are actually used within a file)
- **oxc_resolver**: Node.js-compatible module resolution (tsconfig paths, package.json exports, etc.)

This combination covers the analysis fallow needs: import/export extraction, scope-aware dead binding detection, and cross-file module resolution. Type-level information (e.g., "this import is only used in a type position") is approximated through syntactic heuristics (`import type`, TSQualifiedName patterns) rather than full type inference.

## Alternatives considered

### TypeScript compiler API (tsc)

Full type resolution via `ts.createProgram()`.

- Pros: exact type information, re-export resolution through types, accurate `typeof` handling
- Cons: 2-5s startup overhead, 500MB+ memory for large projects, version coupling (must track TypeScript releases), single-threaded, cannot be embedded in a Rust binary without a Node.js subprocess

### SWC parser

Rust-based parser with its own AST.

- Pros: fast, Rust-native
- Cons: no semantic analysis equivalent to oxc_semantic, less active ecosystem for analysis tooling, different AST design philosophy

### Tree-sitter

Incremental parsing with a C-based runtime.

- Pros: error-tolerant, incremental, language-agnostic
- Cons: grammar-level only (no scope analysis), would need a separate scope resolution layer, TypeScript grammar has known edge cases

## Consequences

**Positive:**
- Sub-second analysis on most projects (vs. seconds with tsc)
- Parallel file parsing via rayon (oxc_parser is stateless per-file)
- No Node.js dependency: fallow ships as a single static binary
- Memory usage proportional to file count, not type-checker state
- Knip v6 subsequently made the same decision, validating the approach

**Negative:**
- Cannot detect usage through purely type-level re-exports (e.g., `export type { Foo } from './bar'` where `Foo` is used via a type alias chain). Mitigated by `import type` / `export type` syntax detection
- Template literal type patterns are not resolved
- Some edge cases require syntactic heuristics that may produce false negatives (erring on the side of not reporting, which is safer than false positives)
