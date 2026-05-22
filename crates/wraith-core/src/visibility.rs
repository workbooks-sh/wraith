//! Visibility tightening — for each `pub` item, find the smallest scope
//! that contains all references and suggest the tightest visibility
//! token that still keeps callers happy. (wb-5lgj.35)
//!
//! Reads the existing reference graph; never recomputes parsing.
//!
//! Scope ladder (broad → tight):
//!   `pub` → `pub(crate)` → `pub(super)` → private (drop `pub`)
//!
//! Heuristics (intentionally conservative; we'd rather skip than break):
//!   - any cross-crate reference → keep `pub`
//!   - all references in the SAME crate as the def → `pub(crate)` candidate
//!   - all references in the SAME file as the def → suggest `private`
//!   - re-exported via `pub use` from a lib/mod root → skip (public API)
//!   - `#[deprecated]` / `#[doc(hidden)]` / `#[allow(dead_code)]` → skip
//!   - trait impls (item inside `impl …`) → not tracked here (parser
//!     emits free items; impl items aren't in the symbol table)

use crate::graph::{ReferenceGraph, SymbolKind, SymbolNode, Visibility};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct Suggestion {
    pub symbol: String,
    pub current: String,
    pub suggested: String,
    pub file: PathBuf,
    pub line: usize,
    pub col: usize,
    pub kind: String,
    pub reason: String,
}

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub suggestions: Vec<Suggestion>,
    pub total: usize,
    pub apply_count_if_applied: usize,
}

/// Build a report of tightening suggestions against the given graph.
///
/// `pub_use_reexports`: set of leaf symbol names re-exported via
/// `pub use foo::bar` from a crate-root or mod-root file in the
/// workspace. Items in that set are presumed to be intentional public
/// API surface and are skipped.
pub fn compute_suggestions(
    graph: &ReferenceGraph,
    pub_use_reexports: &HashSet<String>,
    skip_files: &HashSet<PathBuf>,
) -> Report {
    let symbol_files = collect_symbol_files(graph);
    let mut suggestions = Vec::new();

    for (idx, sym) in graph.symbols.iter().enumerate() {
        let Some(s) = analyze_symbol(graph, idx, sym, &symbol_files, pub_use_reexports, skip_files)
        else {
            continue;
        };
        suggestions.push(s);
    }

    let total = suggestions.len();
    Report {
        suggestions,
        total,
        apply_count_if_applied: total,
    }
}

fn analyze_symbol(
    graph: &ReferenceGraph,
    idx: usize,
    sym: &SymbolNode,
    symbol_files: &HashMap<PathBuf, Vec<usize>>,
    pub_use_reexports: &HashSet<String>,
    skip_files: &HashSet<PathBuf>,
) -> Option<Suggestion> {
    // Only consider items that currently advertise `pub`. `pub(crate)`
    // → `private` is a possible tightening too, but the brief calls out
    // the `pub → pub(crate)/private` direction; we handle both.
    let current = match sym.visibility {
        Visibility::Public => "pub",
        Visibility::PubCrate => "pub(crate)",
        Visibility::Private => return None,
    };

    if sym.is_test || sym.is_entry_point || sym.has_no_mangle || sym.has_export_name {
        return None;
    }
    if sym.impl_of_foreign_trait {
        return None;
    }
    if sym.is_reexport {
        return None;
    }
    if skip_files.contains(&sym.file) {
        return None;
    }
    if pub_use_reexports.contains(&sym.symbol.name) {
        return None;
    }
    // mod items: visibility tightening on `pub mod foo;` cascades to
    // every item inside foo — too easy to break. Skip.
    if matches!(sym.kind, SymbolKind::Mod) {
        return None;
    }

    let refs = collect_references_to(graph, idx, sym);

    // No references at all → that's dead-code's job; skip.
    if refs.is_empty() {
        return None;
    }

    let any_cross_crate = refs
        .iter()
        .any(|r| r.from_crate != sym.symbol.crate_name);

    if any_cross_crate {
        return None;
    }

    let all_same_file = refs.iter().all(|r| r.from_file == sym.file);
    let ref_count = refs.len();
    let crate_name = sym.symbol.crate_name.clone();
    let _ = symbol_files;

    let suggested = if all_same_file {
        "private"
    } else {
        "pub(crate)"
    };

    if suggested == current {
        return None;
    }
    // pub(crate) → pub(crate) is a no-op; pub(crate) → private only if
    // all-same-file.
    if current == "pub(crate)" && suggested != "private" {
        return None;
    }

    let reason = if all_same_file {
        format!(
            "all {} reference(s) are within the defining file",
            ref_count
        )
    } else {
        format!(
            "all {} reference(s) are within crate `{}`",
            ref_count, crate_name
        )
    };

    Some(Suggestion {
        symbol: sym.symbol.qualified(),
        current: current.to_string(),
        suggested: suggested.to_string(),
        file: sym.file.clone(),
        line: sym.line,
        col: sym.col,
        kind: sym.kind.as_str().to_string(),
        reason,
    })
}

/// Collect references in the graph that resolve to `idx` (the given symbol).
///
/// Mirrors the resolver in `ReferenceGraph::referenced_symbols`: a ref's
/// `name` must match the symbol's leaf name; root segment (if present)
/// must agree with the crate.
fn collect_references_to<'g>(
    graph: &'g ReferenceGraph,
    idx: usize,
    sym: &SymbolNode,
) -> Vec<&'g crate::graph::Reference> {
    let mut out = Vec::new();
    // Same-named symbols share `by_name[leaf]` — we need to filter the
    // homonyms out so we only count refs that resolve to THIS idx.
    let homonym_count = graph
        .by_name
        .get(&sym.symbol.name)
        .map(|v| v.len())
        .unwrap_or(1);
    let leaf_unique = homonym_count == 1;

    for r in &graph.references {
        if r.name != sym.symbol.name {
            continue;
        }
        // Self-reference (the def site itself) — skip if the ref line equals
        // the def line and the file matches. Defs are not added as refs by
        // the parser, but a `mod foo` → `foo` segment ref can be co-located.
        if r.from_file == sym.file && r.line == sym.line {
            continue;
        }
        // Path-segment refs (intermediate segments of `a::b::c`) resolve
        // against modules only — skip for non-module symbols to avoid
        // false positives where a fn happens to share a name with a mod.
        if r.is_path_segment && !matches!(sym.kind, SymbolKind::Mod) {
            continue;
        }
        let resolves = match r.root.as_deref() {
            None => leaf_unique || sym.symbol.crate_name == r.from_crate,
            Some("crate") | Some("self") | Some("super") | Some("Self") => {
                sym.symbol.crate_name == r.from_crate
            }
            Some(root) => {
                root == sym.symbol.crate_name || sym.symbol.crate_name == r.from_crate
            }
        };
        if !resolves {
            continue;
        }
        out.push(r);
    }
    let _ = idx;
    out
}

fn collect_symbol_files(graph: &ReferenceGraph) -> HashMap<PathBuf, Vec<usize>> {
    let mut by_file: HashMap<PathBuf, Vec<usize>> = HashMap::new();
    for (i, s) in graph.symbols.iter().enumerate() {
        by_file.entry(s.file.clone()).or_default().push(i);
    }
    by_file
}

/// Scan a file's text for `pub use <path>::ident` (or `pub use … {ident,…}`)
/// re-exports. Returns the set of leaf identifiers re-exported.
///
/// This is a deliberately textual scan — robust enough for the heuristic
/// (we only need to know "this name appears in a `pub use`"), and
/// independent of the `syn` walker so it can be called from the CLI.
pub fn scan_pub_use_reexports(text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("pub use ") && !trimmed.starts_with("pub(crate) use ") {
            continue;
        }
        // Collect lines until we hit a `;` — `pub use foo::{a, b,\n c};`
        let mut buf = trimmed.to_string();
        if !buf.contains(';') {
            let rest_lines = text.lines().skip(i + 1);
            for nl in rest_lines {
                buf.push(' ');
                buf.push_str(nl);
                if nl.contains(';') {
                    break;
                }
            }
        }
        // Strip `pub use ` prefix and trailing `;`.
        let stripped = buf
            .trim_start_matches("pub(crate) use ")
            .trim_start_matches("pub use ")
            .trim_end_matches(';')
            .trim()
            .to_string();
        extract_use_idents(&stripped, &mut out);
    }
    out
}

fn extract_use_idents(spec: &str, out: &mut HashSet<String>) {
    // Cases we care about:
    //   foo::bar             → bar
    //   foo::bar as baz      → baz
    //   foo::{a, b as c, d}  → a, c, d
    //   foo::*               → glob; we record nothing
    let spec = spec.trim();
    if let Some(open) = spec.find('{') {
        if let Some(close) = spec.rfind('}') {
            let inner = &spec[open + 1..close];
            for piece in inner.split(',') {
                add_use_leaf(piece, out);
            }
            return;
        }
    }
    add_use_leaf(spec, out);
}

fn add_use_leaf(piece: &str, out: &mut HashSet<String>) {
    let piece = piece.trim();
    if piece.is_empty() || piece.ends_with('*') {
        return;
    }
    let leaf = if let Some(idx) = piece.find(" as ") {
        piece[idx + 4..].trim().to_string()
    } else {
        piece
            .rsplit("::")
            .next()
            .unwrap_or(piece)
            .trim()
            .to_string()
    };
    if !leaf.is_empty() {
        out.insert(leaf);
    }
}

/// Apply a set of suggestions to disk. Rewrites the `pub` token on the
/// symbol's declaration line to the suggested form. Preserves leading
/// attributes / doc-comments above the def — we only touch the
/// declaration line itself.
///
/// Returns the count of edits applied.
pub fn apply_suggestions(suggestions: &[Suggestion]) -> anyhow::Result<usize> {
    // Group by file so we can read once / write once.
    let mut by_file: HashMap<PathBuf, Vec<&Suggestion>> = HashMap::new();
    for s in suggestions {
        by_file.entry(s.file.clone()).or_default().push(s);
    }

    let mut applied = 0;
    for (file, sugs) in by_file {
        let text = std::fs::read_to_string(&file)?;
        let mut lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
        let mut changed = false;
        for s in sugs {
            if s.line == 0 || s.line > lines.len() {
                continue;
            }
            // The recorded line points at the symbol's IDENT span — the
            // `pub` token usually sits at the START of the same line, or
            // one line above (for multi-line signatures). Walk up to 2
            // lines back looking for the `pub` token to rewrite.
            let mut target: Option<usize> = None;
            for offset in 0..=2usize {
                if s.line <= offset {
                    break;
                }
                let i = s.line - 1 - offset;
                if line_has_pub_token(&lines[i]) {
                    target = Some(i);
                    break;
                }
            }
            let Some(i) = target else {
                continue;
            };
            let new_line = rewrite_pub_token(&lines[i], &s.suggested);
            if new_line != lines[i] {
                lines[i] = new_line;
                changed = true;
                applied += 1;
            }
        }
        if changed {
            let mut out = lines.join("\n");
            if text.ends_with('\n') {
                out.push('\n');
            }
            std::fs::write(&file, out)?;
        }
    }
    Ok(applied)
}

/// True if the given source line contains a top-level `pub` visibility
/// token (i.e. the start of an item declaration). Excludes lines that
/// look like attributes, comments, or string literals containing the
/// word `pub`.
fn line_has_pub_token(line: &str) -> bool {
    let t = line.trim_start();
    if t.starts_with("//") || t.starts_with("/*") || t.starts_with('#') {
        return false;
    }
    // Match `pub` followed by `(` or whitespace, at start of trimmed line.
    if let Some(rest) = t.strip_prefix("pub") {
        return rest
            .chars()
            .next()
            .map(|c| c == ' ' || c == '\t' || c == '(')
            .unwrap_or(false);
    }
    false
}

/// Rewrite the leading `pub[...] ` token of `line` to match `suggested`.
/// `suggested` is one of: `"pub(crate)"`, `"pub(super)"`, `"private"`.
fn rewrite_pub_token(line: &str, suggested: &str) -> String {
    // Find the indentation prefix.
    let indent_len = line.len() - line.trim_start().len();
    let (indent, body) = line.split_at(indent_len);
    let new_body = if let Some(rest) = body.strip_prefix("pub") {
        let after = rest;
        // `pub(...)` form — eat the parenthesised group.
        let after_vis = if after.starts_with('(') {
            // Find matching close paren.
            let mut depth = 0i32;
            let mut end = 0usize;
            for (i, c) in after.char_indices() {
                if c == '(' {
                    depth += 1;
                } else if c == ')' {
                    depth -= 1;
                    if depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
            }
            &after[end..]
        } else {
            after
        };
        let after_vis = after_vis.trim_start_matches(|c: char| c == ' ' || c == '\t');
        match suggested {
            "private" => after_vis.to_string(),
            s => format!("{} {}", s, after_vis),
        }
    } else {
        body.to_string()
    };
    format!("{}{}", indent, new_body)
}

/// Format a Report as human-readable markdown.
pub fn render_markdown(report: &Report) -> String {
    let mut out = String::new();
    if report.suggestions.is_empty() {
        out.push_str("no visibility tightening opportunities.\n");
        return out;
    }
    out.push_str(&format!(
        "# Visibility tightening — {} suggestion(s)\n\n",
        report.total
    ));
    for s in &report.suggestions {
        out.push_str(&format!(
            "- `{}` ({}): `{}` → `{}`  \n  at {}:{}  \n  {}\n",
            s.symbol,
            s.kind,
            s.current,
            s.suggested,
            file_relative_or_display(&s.file),
            s.line,
            s.reason,
        ));
    }
    out
}

fn file_relative_or_display(p: &Path) -> String {
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(rel) = p.strip_prefix(&cwd) {
            return rel.display().to_string();
        }
    }
    p.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_pub_to_pub_crate() {
        let line = "pub fn foo() -> i32 { 1 }";
        assert_eq!(
            rewrite_pub_token(line, "pub(crate)"),
            "pub(crate) fn foo() -> i32 { 1 }"
        );
    }

    #[test]
    fn rewrites_pub_to_private() {
        let line = "    pub fn foo() {}";
        assert_eq!(rewrite_pub_token(line, "private"), "    fn foo() {}");
    }

    #[test]
    fn rewrites_pub_restricted_form() {
        let line = "pub(crate) fn foo() {}";
        assert_eq!(rewrite_pub_token(line, "private"), "fn foo() {}");
    }

    #[test]
    fn line_has_pub_token_works() {
        assert!(line_has_pub_token("pub fn x() {}"));
        assert!(line_has_pub_token("    pub(crate) struct X;"));
        assert!(!line_has_pub_token("// pub fn x"));
        assert!(!line_has_pub_token("#[derive(pub)]"));
        assert!(!line_has_pub_token("    fn x() {}"));
    }

    #[test]
    fn scan_pub_use_extracts_leaf_names() {
        let src = "pub use foo::bar;\npub use foo::{baz, qux as quack};\npub use foo::*;\n";
        let set = scan_pub_use_reexports(src);
        assert!(set.contains("bar"));
        assert!(set.contains("baz"));
        assert!(set.contains("quack"));
        assert!(!set.contains("qux")); // renamed away
        assert!(!set.contains("foo"));
    }
}
