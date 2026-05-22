# ADR-008: fallow-core is internal; embedders use fallow-cli::programmatic

**Status:** Accepted
**Date:** 2026-05-11

## Context

`fallow-core` is a workspace crate that contains the analysis orchestration, detection logic, duplicates clone detector, plugin registry, and cross-reference layer. Most of its module surface (`fallow_core::analyze::*`, `fallow_core::duplicates::*`, `fallow_core::discover::*`, `fallow_core::extract::*` via re-export, `fallow_core::graph::*` via re-export) is declared `pub` because fallow-cli, fallow-lsp, the napi bindings, and the in-tree benchmarks all need access. Those internal consumers compile against the workspace path dependencies, so they are unaffected by version-stability decisions.

Across the 2026 Q1 and Q2 release cadence, two functions in particular (`fallow_core::analyze::find_duplicate_exports` and `fallow_core::analyze::find_unused_exports`) had their signatures change in patch releases as detection capabilities expanded. The workspace consumers update lockstep with the change, but external crates that pinned `fallow-core` from crates.io broke twice on these signature shifts. Each break required a follow-up release and direct contact with the affected consumer.

The pending follow-ups in fallow-rs/fallow#322 (task 1: hoist `compile_ignore_matchers` to `ResolvedConfig`; task 4: a config-writer module that downstream `add-to-config` actions consume) will both touch `pub fn` surface on `fallow-core::analyze`. Without a written policy on what fallow-core exposes as a stable contract, every internal refactor must either preserve a signature that no external consumer is intentionally relying on, or risk another silent break.

This ADR records the policy now so the #322 follow-ups (and any future refactor) can change internal signatures freely.

## Decision

`fallow-core` is an internal implementation crate of the fallow workspace. Its public items are public only so that other crates in the same workspace can compile against it. They are NOT a stable library API and may change in any release, including patch releases.

External embedders consume the curated programmatic surface at `fallow_cli::programmatic`:

- `detect_dead_code`, `detect_circular_dependencies`, `detect_boundary_violations`, `detect_duplication`, `compute_complexity`, `compute_health` (the user-facing one-shot analyses; each returns `Result<serde_json::Value, ProgrammaticError>`)
- `ProgrammaticError { message, exit_code, code, help, context }` (structured errors preserving the CLI's exit-code ladder: 0 ok, 2 generic, 7 network, ...). The internal `type ProgrammaticResult<T> = Result<T, ProgrammaticError>` alias is private; external callers spell out the `Result<T, ProgrammaticError>` shape directly.
- `AnalysisOptions`, `DeadCodeOptions`, `DuplicationOptions`, `ComplexityOptions` (the input option structs)

Each function in `fallow_cli::programmatic` returns a `serde_json::Value` whose shape matches the CLI's `--format json` contract (`schema_version`, `summary`, relative paths, injected `actions`, optional `_meta` under `--explain`). The same crate is what the napi bindings (`crates/napi`) re-export to Node consumers, so the embedder API is also the API agents already integrate against via `@fallow-rs/napi`.

The decision is recorded as policy here; no code change ships with this ADR.

A follow-up release will execute the policy in two steps, in this order:

1. Add `#[deprecated]` attributes to the `pub fn` items under `fallow_core::analyze::*` with notes pointing at `fallow_cli::programmatic::*`. Silence the deprecation warnings inside the workspace via `#[allow(deprecated)]` at the call sites in `fallow-cli` and `fallow-lsp`. Keep the crate publishable through this release so any silent external consumer surfaces by upgrading and seeing a compile warning.
2. After one minor release of grace, flip `publish = false` on `fallow-core` in `Cargo.toml` and remove the deprecation attributes (they no longer serve a purpose once the crate cannot be consumed externally). Workspace `path = ...` dependencies continue to compile unchanged.

CHANGELOG entries call out both steps explicitly so external consumers have a documented migration path.

## Alternatives considered

### Option A: `#[doc(hidden)]` on `fallow_core::analyze::*` items

The items remain `pub` so fallow-cli can call them, but disappear from rustdoc and signal "do not depend on this." This is what `serde_derive` does.

- Pros: cheapest change; no `pub(crate)` cascade through hundreds of internal callers; preserves all current call paths verbatim
- Cons: does not enforce anything at the type level; a determined consumer still imports the function and depends on it; "hidden" is a documentation gesture, not a binding contract

### Option B: `pub(crate)` + a narrow `fallow_core::api` facade

The current `analyze::*` items become `pub(crate)` (forcing the workspace to consolidate access through one entry point), and a new `api` module re-exports a stable subset with semver-tracked signatures.

- Pros: strongest type-level enforcement; forces the workspace to design and document what fallow-core's "stable surface" actually is
- Cons: requires hundreds of `pub(crate)` flips across the crate, plus matching re-exports in the facade; the honest answer to "what IS the stable API of fallow-core?" today is "nothing yet, the curated embedder surface lives in fallow-cli::programmatic"; multi-week refactor to solve a problem the existing programmatic facade already addresses

### Option C: `publish = false` on fallow-core (selected)

Mark the crate as un-publishable. Workspace consumers continue to compile against `path = "../core"`. External consumers cannot pull `fallow-core` from crates.io at all and must go through `fallow-cli::programmatic` (which is what the napi bindings already do).

- Pros: matches the existing architecture (`fallow_cli::programmatic` is already the embedder surface with structured errors, exit-code parity, and quiet-mode semantics); no `pub(crate)` cascade; clear policy users can rely on; signature changes inside `fallow-core` become free
- Cons: external consumers who DID depend on `fallow-core` (the small group surfaced by the two prior breaks) must migrate to `fallow-cli::programmatic`; the deprecation cycle in step 1 above mitigates this by giving them one release of warnings before the publish flip

## Consequences

**Positive**

- `fallow-core::analyze::*` signatures (and any other internal item) can change freely in any release; the workspace continues to compile because internal consumers update lockstep
- The #322 task 1 hoist of `compile_ignore_matchers` to `ResolvedConfig` no longer needs to worry about external breakage on `find_duplicate_exports`'s parameter list
- The embedder API gets a single discoverable entry point (`fallow_cli::programmatic`), with structured errors, JSON shape parity with the CLI, and exit-code semantics already in place
- The napi bindings (`@fallow-rs/napi`) and any AI-agent integrations that consume the JSON contract are unaffected; they already route through the programmatic surface

**Negative**

- External consumers who imported `fallow-core` directly (small group, surfaced by the two prior signature-change incidents) must migrate to `fallow_cli::programmatic` once the deprecation cycle completes. The deprecation message names the replacement function for each removed entry point
- The "stable embedder surface" is now defined by `fallow_cli::programmatic`'s exported items. Adding or removing items there becomes a versioned decision in its own right, which the workspace must take seriously (this is upside, not just downside)
- New contributors must be told once that fallow-core is internal. This ADR is the canonical reference; CLAUDE.md and the crate-level rustdoc on `fallow-core` will link to it once steps 1 and 2 of the rollout ship
