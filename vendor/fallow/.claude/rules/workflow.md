# Feature workflow chain

How `/fallow-implement`, `/panel-review`, `/user-panel`, and `/fallow-review` thread together for non-trivial features, and why each one forks or stays in main session context.

## The chain

For any non-trivial feature, bug fix, or design change, the order is:

1. `/fallow-implement <description>` produces a plan, writes it to `.plans/<slug>.md`, pauses for user confirmation, then carries out research, implementation, verification, docs, and benchmarks across 8 phases.
2. Optional: `/panel-review` or a `user-panel` Task agent, invoked between Phase 1 (plan) and Phase 2 (implementation) when the feature has config surface, UX decisions, schema additions, MCP tool descriptions, or naming-load-bearing knobs. Reads the plan from `.plans/<slug>.md`. Surfaces feedback in main context.
3. Implementation continues. `/fallow-implement` proceeds through Phases 2 to 7, optionally adjusted based on panel feedback.
4. `/fallow-review` at the end, pre-ship review that validates code quality, output formats, filters, docs, companion repos, schemas, and integration points. Produces a verdict.

For trivial fixes (typos, dependency bumps, doc-only changes), skip the chain. Edit directly and run `cargo fmt --all` plus `cargo clippy`.

## Fork policy per skill

Different skills have different relationships to the surrounding session context. The right `context:` frontmatter setting depends on whether the next step in the chain needs the skill's reasoning trace.

`/fallow-implement` stays in main (`context:` unset). The skill orchestrates 8 phases, and downstream skills (`/panel-review`, `/user-panel`) read from its planning trace plus the `.plans/<slug>.md` artefact. Forking would prevent the chain from seeing the planning rationale. The plan artefact is what survives compaction and parallel-agent races; the in-context trace is what the user reads during the conversation.

`/panel-review` stays in main (`context:` unset). The value of a panel review is the per-persona rationale, not just the consensus verdict. The implementation phase needs to see why each panelist objected or approved, otherwise the implementer cannot make the right tradeoffs. A summary return-artefact loses too much signal.

The `user-panel` Task agent forks implicitly (the Task tool itself produces a summary back to main). Use this when the rationale is captured in a structured per-persona output and the main session does not need the full multi-thousand-token deliberation. Spawned via the `user-panel` agent defined in `.claude/agents/user-panel.md`.

`/fallow-review` forks (`context: fork`). The skill reviews the diff on disk and produces a verdict plus concerns. Nothing in the chain consumes its trace; the verdict is the artefact. Forking keeps main context clean of the multi-phase review trace (scope detection, code quality, output format audit, filter parity, integration points, benchmark validation, report).

`/panel-review-loop` stays in main (`context:` unset). Iterative validation loop across benchmark projects. Used standalone, not part of the feature chain. The main session is where the user steers the loop.

## The `.plans/` artefact

`.plans/` is a gitignored directory at the fallow repo root. `/fallow-implement` writes its Phase 1 plan to `.plans/<feature-slug>.md` before pausing for user confirmation.

The plan file is the load-bearing artefact across the chain:

- Survives `/compact` and session restarts.
- Survives `context: fork` boundaries.
- Re-readable by parallel agents and worktree sessions.
- Re-readable by `/panel-review`, `/user-panel`, and `/fallow-review` without inheriting the planning trace.

Plans are transient working artefacts. Concepts that promote to load-bearing structural choices move to `decisions/` as ADRs. Plans that ship can be deleted; the diff and the ADR are the durable records.

Slug convention: kebab-case derived from the feature title or issue number. Examples: `unused-catalog-entries-lsp.md`, `issue-403-rebase-audit.md`, `phase2-capture-quality.md`.

## Branch points in the chain

`/fallow-implement` already has pause points where the user confirms direction. Two of those are decision-tree branches where `AskUserQuestion` is the right interaction tool:

- After Phase 1 plan is written: continue to Phase 2, invoke `/panel-review` first, revise the plan, or abandon.
- After Phase 1c impact assessment surfaces a behavioral ambiguity: pick the conservative path (fewer findings, no false positives) or the aggressive path (more findings, possible false positives).

For both branches, prefer `AskUserQuestion` over prose prompts. The "Other" option in `AskUserQuestion` covers the cases the structured choices miss.

## When to skip steps

- Plan-only mode: invoke `/fallow-implement` and stop after Phase 1 plan is written. Useful when the user wants to think before committing. The plan persists in `.plans/`.
- Panel-only mode: invoke `/panel-review` against an existing `.plans/<slug>.md` file or against uncommitted changes. Useful as a second opinion.
- Review-only mode: invoke `/fallow-review` against uncommitted changes or a PR. Useful for contributor PRs where implementation already happened elsewhere.

## Cross-references

- Domain vocabulary: `@CONTEXT.md`
- Team-assembly matrix per change type: `@.claude/rules/team-assembly.md`
- Per-crate rules: `@.claude/rules/cli-crate.md`, `@.claude/rules/core-crate.md`, `@.claude/rules/extract-crate.md`, `@.claude/rules/graph-crate.md`, `@.claude/rules/plugins.md`, `@.claude/rules/detection.md`
- Release workflow: `@.claude/rules/release-workflow.md`
