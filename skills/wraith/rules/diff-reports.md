# Wraith: diff reports (`--since`)

`wraith report --since=<ref>` is the killer feature for two use cases:

1. **PR descriptions** — "here's what this PR improved (or regressed)"
2. **Session wins** — "here's what wraith + this cleanup session accomplished"

## How it works

Given a git revision, wraith:

1. Captures HEAD findings (live working tree)
2. Creates a temporary worktree at `<ref>` (under `.claude/wraith-diff/<sha>/`, gitignored)
3. Captures baseline findings there
4. Diffs the two finding sets via normalized identity (kind + relative file path + symbol)
5. Pulls `git diff --shortstat` for the LOC delta
6. Renders a markdown narrative

The worktree is reused across re-runs against the same ref, so the warm `.wraithrc.cache` makes the second invocation 3-5x faster.

## What the report tells you

**Top of report — `## At a glance` table:**

| metric | before | after | delta |
|---|---:|---:|---:|
| Source files | 242 | 242 | +0 |
| Lines of code | 71542 | 71376 | **-166** |
| Total findings | 87 | 72 | **-15** |
| Findings resolved | — | — | **28** |
| Findings introduced | — | — | 13 |
| Findings unchanged | — | — | 59 |

Plus the raw `git diff` (path-scoped): `20 files changed, +245 / -412 lines`.

**Sections:**

- `## Findings resolved` — what's fixed since base (broken down by detector + collapsible list)
- `## Findings introduced` — what's new at HEAD (catches regressions)
- `## Net effect` — narrative summary: "Net win: X resolved, Y introduced (net -Z)"

## When to use it

**Always at the end of a cleanup session.** Run against the session's start commit and paste the result into the PR description. It tells the reviewer: "wraith identified X issues; I fixed Y; here are the receipts."

**Before merging a refactor.** Run against the target branch's tip to catch regressions you might have introduced.

**For monthly project-health snapshots.** Run against the last month's commit and post in #engineering — concrete signal on whether the codebase is improving or regressing.

## Limitations

- **Finding identity is path + symbol + kind.** If you rename a fn during the session, the diff sees it as `resolved` + `introduced` (two events) rather than `renamed`. Not a bug, but worth knowing.
- **Same-file dupe clusters** can shift around as you delete cluster members. A 3-member cluster losing 1 becomes a 2-member cluster — the diff might count this as "1 resolved + 1 introduced" rather than "shrunk."
- **The base snapshot uses the SAME wraith binary as HEAD.** So changes in wraith's detector behavior between commits aren't reflected — both sides see the same detectors. That's usually what you want.

## Examples

**End-of-session win narrative:**
```bash
# Assuming session started at SHA abc1234:
wraith report --since=abc1234 > session-wins.md
```

**Pre-merge regression check:**
```bash
git fetch origin main
wraith report --since=origin/main
```

**Compare two refs (not from HEAD):**

Currently `--since` always compares against HEAD. For arbitrary `<ref1>..<ref2>` comparison, check out ref2 first then run `--since=<ref1>`.

**The wavelet demo run:**
```bash
cd packages/wavelet
wraith report --since=9da581812   # pre-session commit
```

…yielded:
- −166 LOC
- 28 findings resolved (dead-code, unused-deps, dupes consolidated)
- 13 introduced (mostly post-refactor surface area that surfaced new dupes)
- Net −15 findings

That's the kind of concrete win-statement that makes a refactor PR easy to merge.
