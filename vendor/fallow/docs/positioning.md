# Fallow: Positioning and Copy Guide

This document is the canonical copy guide for all fallow repos (fallow, fallow-docs, fallow-skills, fallow-cov-protocol, fallow-cloud). The new brand is codebase intelligence for TypeScript and JavaScript, split across a free static layer and a paid runtime layer. Everything authored for README heroes, package descriptions, marketplace listings, crates.io metadata, docs, marketing, dashboard copy, and launch posts must route through the decisions captured here. If a surface cannot fit the longer form, fall back to the tagline plus the two-layer one-liner.

## North Star

**Who:** TypeScript and JavaScript teams of any size, from solo developers to large monorepos.

**What:** Codebase intelligence for TypeScript and JavaScript, delivered in two layers. The free static layer finds what is connected: unused files, exports, types, dependencies, circular imports, duplication, complexity hotspots, architecture boundary violations, and feature-flag usage, with integrations for CI, editors, and MCP. The paid runtime layer, Fallow Runtime, adds execution evidence from production: hot and cold paths, runtime-backed review, runtime-weighted health, stale-flag evidence, trends, and alerts.

**Why now:** Two drifts are compounding at once. AI-accelerated code generation is creating structural drift faster than humans can review it, so teams need cross-file structural analysis that runs on every commit, not once a quarter. At the same time, teams still ship code with no execution evidence, so they guess which paths are hot, which are cold, and which flags are dead. That is execution drift, and the traditional coverage tools that could answer it were built for tests, not production. Fallow closes both gaps in one pipeline.

**Why fallow:** The only tool that combines cross-file static analysis with production runtime evidence in a single Rust-native pipeline. Sub-second static analysis on typical projects, optional runtime coverage collection when teams are ready to upgrade, and a single report that speaks both languages. Zero configuration on the static layer, explicit licensed onboarding on the runtime layer.

## Tagline

**Codebase intelligence for TypeScript and JavaScript**

Usage: README hero, GitHub repo header, docs landing, marketplace listings. "JavaScript" is understood to include TypeScript in 2026 tooling discourse.

## Subtitle

**Static analysis finds what is connected. Runtime intelligence finds what actually runs. Both land in the same report.**

Usage: directly below the tagline in README heroes, docs landing, and the marketing hero. Communicates the two-layer model in one line without naming the commercial product.

## Stack Positioning

**Primary form:** Linters check files. TypeScript checks types. Fallow checks the codebase.

Usage: docs landing page, blog posts, conference talks, README explainer sections, marketing site. Positions fallow as the third pillar alongside oxlint/Biome and tsc.

**Secondary form (still acceptable):** Linters enforce style. Formatters enforce consistency. Fallow enforces relevance.

Usage: long-form essays, explainer sections where the emphasis is on what fallow enforces rather than what it checks. Do not use as the primary stack-positioning line on hero surfaces.

Full comparison page: [Fallow vs linters](https://docs.fallow.tools/explanations/fallow-vs-linters).

## Two-layer product model

Fallow ships as two layers on a single pipeline.

**Free static layer.** Open source under MIT. Rust-native binary with an npm wrapper, a VS Code extension, an LSP, an MCP server, a GitHub Action, and a CLI. Finds unused code, duplication, circular dependencies, complexity hotspots, architecture boundaries, feature-flag usage, and more. Zero configuration on typical projects. Sub-second on most codebases. This is the layer we describe as "static codebase intelligence" in long-form copy.

**Paid runtime layer (Fallow Runtime).** Commercial add-on. Ingests production V8 coverage, normalizes it, and joins it back to the static graph. Surfaces hot paths, cold code, runtime-backed review, runtime-weighted health, stale-flag evidence, trends, and alerts. Licensed via a signed JWT, offline-verified by the CLI. The public engine term in docs and CLI output is "runtime coverage"; the commercial product name is "Fallow Runtime."

**One-sentence claim:** Static analysis is free and open source. Runtime intelligence is the paid team layer.

Usage: landing pages, marketplace descriptions long enough to include it, the "Why fallow" section in docs, and the pricing page. Include it verbatim on any surface that has room.

## Wedge and outcomes

Fallow earns its keep through concrete outcomes. The primary wedge is always the first one.

- **Delete and refactor with confidence (primary wedge).** Static finds what nothing imports. Runtime confirms what nothing hits in production. Ship the delete PR without fear.
- **Review hot-path changes.** In PR review, surface whether the file being changed is on a hot path or cold code. Reviewers weight attention accordingly.
- **Prioritize refactors.** Runtime-weighted health ranks complexity hotspots by how often they actually run, not by raw cyclomatic score.
- **Retire stale flags.** Static finds flag references. Runtime proves a branch has not been hit in weeks. Remove the flag, collapse the branch, delete the old path.

## One-Liners (per surface)

| Surface | Copy |
|---------|------|
| npm description (`fallow`) | Codebase intelligence for TypeScript and JavaScript. Finds unused code, duplication, circular dependencies, complexity hotspots, and architecture drift. Optional runtime intelligence layer (Fallow Runtime) adds production execution evidence. Rust-native, sub-second, 95 framework plugins. |
| GitHub repo description (`fallow-rs/fallow`) | Codebase intelligence for TypeScript and JavaScript. Static analysis finds what is connected; Fallow Runtime finds what actually runs. Rust-native, sub-second, 95 framework plugins. |
| fallow-skills GitHub description | Agent skills for fallow codebase intelligence. Teaches AI agents how to use static analysis and, where licensed, Fallow Runtime evidence to delete and refactor with confidence. |
| fallow-cov-protocol description | Wire protocol for Fallow Runtime: typed schemas for production V8 coverage ingest, normalization, and replay. |
| docs.fallow.tools description | Documentation for fallow, the codebase intelligence platform for TypeScript and JavaScript. Static analysis is free and open source. Runtime intelligence is the paid team layer. |
| fallow.tools hero | Codebase intelligence for TypeScript and JavaScript. |
| fallow.tools subhead | Static analysis finds what is connected. Runtime intelligence finds what actually runs. Both land in the same report. |
| fallow.cloud (dashboard) tagline | Fallow Runtime: production execution evidence for your codebase. |
| MCP server description | MCP server for fallow codebase intelligence. Exposes static analysis, and where licensed, Fallow Runtime evidence, as typed tools to AI agents. |
| VS Code marketplace description | Codebase intelligence for TypeScript and JavaScript. Real-time diagnostics for unused code, duplication, complexity hotspots, and architecture drift, with optional runtime evidence via Fallow Runtime. |
| GitHub Action description | Codebase intelligence for TypeScript and JavaScript in CI. Runs fallow on pull requests; summarizes unused code, duplication, complexity, boundaries; optionally joins Fallow Runtime evidence to the report. |
| crates.io short description (`fallow-cli`) | CLI for fallow, Rust-native codebase intelligence for TypeScript and JavaScript. |

All entries are em-dash-free.

## Naming hierarchy

Use these terms consistently. The brand comes first, the category phrase explains what fallow is, the product descriptors distinguish the free and paid layers, and the engine term stays technical.

- **Brand:** Fallow
- **Category phrase:** Codebase intelligence for TypeScript and JavaScript
- **Free product descriptor:** Static codebase intelligence (the open-source layer)
- **Paid product name:** Fallow Runtime
- **Technical engine term (docs + CLI):** runtime coverage (the collection mechanism that powers Fallow Runtime)
- **Outcome language:** cold code, hot paths, runtime-backed review, stale flags, runtime-weighted health

## What not to say

Do not ship these phrases. They either muddy the category, undersell the product, or set false expectations.

- "developer intelligence platform" (generic, reads like an enterprise pitch)
- "better knip" (private comparison only, never on marketing surfaces)
- "runtime coverage" as the hero headline (engine term, not category; keep it in docs and CLI)
- "dead code tool" as the identity (too narrow, loses the runtime layer entirely)
- "AI-first" as the category (fallow is deterministic; AI is an audience, not an ingredient)
- "zero config" adjacent to Fallow Runtime onboarding copy (the static layer is zero-config, the runtime layer is licensed and explicit)

## Consistency checks

Use this checklist on every new piece of copy before shipping.

- [ ] No em-dashes (U+2014). Use commas, periods, colons, parentheses, or ` -- ` instead.
- [ ] The free static layer and the paid runtime layer are clearly separated. A reader should never confuse which is which.
- [ ] On any surface with room, include the sentence: "Static analysis is free and open source. Runtime intelligence is the paid team layer."
- [ ] The category phrase "codebase intelligence" appears before any outcome language (delete, refactor, hot paths, stale flags).
- [ ] "Fallow Runtime" is used for the commercial product; "runtime coverage" is used for the engine in docs and CLI output.
- [ ] No phrase from the "What not to say" list has slipped in.
- [ ] Primary stack line is "Linters check files. TypeScript checks types. Fallow checks the codebase." The secondary form only appears in long-form prose.
