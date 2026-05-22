# ADR-007: Boundary zone subtree-relative `root`

**Status:** Accepted
**Date:** 2026-04-26

## Context

Architecture boundary zones (`crates/config/src/config/boundaries.rs`) match files via project-root-relative glob patterns. This works for single-package layouts (`src/components/**`, `src/db/**`) but collapses on monorepos where every package has the same internal directory layout.

Concretely, with two packages each owning a `src/` tree, the natural way to express "ui inside the app package, domain inside the core package" is:

```jsonc
{ "name": "ui",     "patterns": ["packages/app/src/**"] }
{ "name": "domain", "patterns": ["packages/core/src/**"] }
```

This works, but it forces every zone definition to repeat the package prefix. Worse, when a third package joins (`packages/billing/`), the user must add a third zone for what is, semantically, the same "ui" or "domain" layer just in a different package. Configuration scales with the cross product of (zones, packages) instead of zones plus packages.

The `BoundaryZone.root` field has been serde-accepted and inert for two releases, with `BoundaryConfig::resolve` emitting a `FALLOW-BOUNDARY-ROOT-RESERVED` warning so users could not silently rely on it. This ADR records the decision for the now-shipping behavior.

## Decision

`BoundaryZone.root: Option<String>` becomes a subtree scope. When set, the zone's `patterns` are resolved relative to that directory at classification time:

1. The path being classified must start with the zone's `root` prefix; if not, the zone is skipped.
2. The prefix is stripped, and the remaining path is matched against the zone's compiled glob matchers.

Implementation lives in `ResolvedBoundaryConfig::classify_zone`. Zones without a `root` keep their existing project-root-relative behavior at zero cost (a single `Option::is_none()` branch per candidate path).

A normalization step at resolve time canonicalizes user input: backslashes become forward slashes, leading `./` is stripped, and a trailing `/` is appended when missing. Empty / `"."` / `"./"` collapse to no-root semantics. The normalized form is what `classify_zone` compares against.

A redundant-prefix validation runs before classification. For each zone with `root: X/`, every pattern is checked against the normalized prefix; any pattern starting with `X/` is reported via `tracing::error!` tagged `FALLOW-BOUNDARY-ROOT-REDUNDANT-PREFIX`. This catches users who double-prefixed during the warning ramp ("we wrote `packages/app/src/**` because root was inert; now we need to drop the prefix").

## Alternatives considered

### Prefix-rewriting at resolve time

Concatenate `root` onto each pattern at resolve time, producing flat globs the existing classifier consumes unchanged.

- Pros: zero changes to `classify_zone`; one code path everywhere
- Cons: corrupts the stored pattern strings; debugging output (`fallow list --boundaries`, error messages) shows synthesized patterns the user never wrote; later normalization (e.g. de-duplicating zones) becomes structurally messy because two distinct user-written patterns can collapse to the same effective glob

### `rootMode` opt-in field

Add `rootMode: "filter" | "prefix" | "ignore"` to `BoundaryZone` to let users pick semantics.

- Pros: users could keep historic prefix-style patterns alive without migration
- Cons: permanent expansion of the public config API to solve a one-time migration moment; a configuration knob users must learn for no ongoing benefit; ships with three semantic modes when only one is correct going forward

### Filter at classification time (selected)

Filter `relative_path` against `root`; strip and glob-match the remainder.

- Pros: zero-cost for zones without root (single Option check); patterns stored verbatim so debug output and `fallow list` stay clean; normalization happens at one well-defined boundary; the redundant-prefix validator reuses the same normalized form
- Cons: classification path now branches on `zone.root.as_deref()` for every zone; in a project with N zones and M files this is N*M extra comparisons. In practice both numbers are small (typically <20 zones, files already cached after first classification) and the branch is predictable

## Consequences

**Positive**

- Monorepos with per-package layouts can be expressed compactly: one zone per architectural layer, repeated only once via `root` per package
- Existing single-package configs unaffected; `root` is opt-in and zero-cost when unset
- Pattern strings retain their as-authored form, keeping `fallow list --boundaries` and tracing output debuggable
- The `FALLOW-BOUNDARY-ROOT-REDUNDANT-PREFIX` validation explicitly catches the migration footgun the panel flagged ("we doubled-prefixed because root was inert")

**Negative**

- Users who configured `root` during the warning ramp expecting prefix semantics need to drop the package prefix from their patterns, or hit the redundant-prefix error. The error message names the offending zone, pattern, and root explicitly, so the migration step is mechanical
- Adds a normalization function that must stay in sync with any future path-normalization changes elsewhere in fallow (forward slashes only, trailing slash semantics)
- Classification cost scales linearly with zone count rather than being constant; for projects with hundreds of zones this could matter, though no current users approach that scale
