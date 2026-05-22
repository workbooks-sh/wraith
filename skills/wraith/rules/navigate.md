# Wraith: navigation primitives (token-economy)

These four subcommands exist purely to save you from grep + Read of whole files. Each one returns structured data scoped to exactly what you need to act on.

## `wraith ctx <symbol> [--no-body] [--format=json] [--limit=N]`

Smallest useful "context window" for a single symbol. Returns:

- Symbol's full definition (signature + body; just signature with `--no-body`)
- File's `use` imports
- Top-N signatures of direct callers (default 5)
- Top-N signatures of what this symbol calls
- File path + line range

**Use when**: "I need to understand what `wavelet::run_image` is" or "What calls `apply_chain_cpu`?"

**Don't use when**: you need the literal full file body to make a multi-line edit — that's the existing Read tool.

JSON shape:
```json
{
  "symbol": "wavelet::run_image",
  "file": "src/bin/wavelet.rs",
  "line_start": 2451, "line_end": 2900,
  "signature": "pub async fn run_image(...) -> Result<()>",
  "body": "...",          // omitted if --no-body
  "imports": ["use anyhow::Result;", ...],
  "callers": [{ "symbol": "wavelet::main", "signature": "...", "file": "...", "line": ... }],
  "callees": [{ "symbol": "wavelet::backends::veo::generate", ...}]
}
```

## `wraith summarize <file> [--include-bodies] [--format=json]`

Per-file structured summary. Lets you decide whether to actually read a file:

- Pub interface (fns/types/consts/traits) with signatures
- Imports (`use` roots)
- Per-fn complexity (cyclomatic + cognitive)
- Module dependencies (what other modules this file references)
- LOC count

**Use when**: "Should I bother reading `src/bin/wavelet.rs`?" → summarize first. If the pub interface tells you what you need, skip the Read.

## `wraith ls [pattern] [--kind=fn|struct|enum|trait|type|const|static|mod] [--format=json]`

Symbol listing with file:line + kind + visibility. Replaces `grep -rn fn foo`:

```bash
wraith ls "run_*" --kind=fn --format=json
```

Returns every fn whose qualified name matches `run_*`. Faster than grep, structured output, kind-filtered.

**Use when**: "Find all my `run_*` handlers", "List all pub traits in the workspace", "What types live in `backends::`?"

## `wraith graph <subcommand>`

Five read-only views over the reference graph:

- `crate-deps [--format=json|dot|md]` — workspace crate-level dep graph
- `callers <symbol> [--transitive]` — direct or transitive callers
- `callees <symbol> [--transitive]` — what this symbol calls
- `blast-radius <symbol> [--depth=N]` — "if I change this, what's affected"
- `reverse-deps <module-or-crate>` — what depends on this

**Use when**: refactoring a fn, deciding whether a rename is safe, understanding impact scope.

Sample:
```bash
wraith graph blast-radius wavelet::backends::util::pick_image_ext_from_mime --depth=2
```
→ ranked list of every symbol within 2 hops that depends on `pick_image_ext_from_mime`.

## The pattern

Whenever your instinct is to:
- `grep` a name → try `wraith ls` or `wraith graph callers`
- `cat` a file to learn its shape → try `wraith summarize`
- Read a fn + its callers → try `wraith ctx`

If wraith doesn't have what you need, fall back to grep / Read. But check first — the structured output is faster and uses less context.
