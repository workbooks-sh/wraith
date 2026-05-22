//! Token-economy primitives: `ctx`, `summarize`, `ls`.
//!
//! Goal — let an AI agent retrieve the smallest useful slice of code
//! per task instead of `grep -rn` + whole-file `Read`. Each helper
//! returns structured data (serde-friendly) so the CLI can emit JSON
//! or render markdown without duplicating logic. See wb-5lgj.33.

use crate::graph::{ReferenceGraph, SymbolKind, SymbolNode, Visibility};
use crate::health::ComplexityVisitor;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use syn::spanned::Spanned;
use syn::visit::Visit;

// ---------------------------------------------------------------------------
// `wraith ctx <symbol>`
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct CtxNeighbor {
    pub symbol: String,
    pub signature: String,
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SymbolContext {
    pub symbol: String,
    pub kind: String,
    pub visibility: String,
    pub file: String,
    pub line_start: usize,
    pub line_end: usize,
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub imports: Vec<String>,
    pub callers: Vec<CtxNeighbor>,
    pub callees: Vec<CtxNeighbor>,
}

impl SymbolContext {
    pub fn render_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("# `{}` ({})\n\n", self.symbol, self.kind));
        out.push_str(&format!(
            "{}:{}..{}  ·  visibility: {}\n\n",
            self.file, self.line_start, self.line_end, self.visibility
        ));
        out.push_str("## Signature\n\n```rust\n");
        out.push_str(&self.signature);
        out.push_str("\n```\n\n");
        if let Some(body) = &self.body {
            out.push_str("## Body\n\n```rust\n");
            out.push_str(body);
            if !body.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```\n\n");
        }
        if !self.imports.is_empty() {
            out.push_str("## Imports\n\n```rust\n");
            for u in &self.imports {
                out.push_str(u);
                out.push('\n');
            }
            out.push_str("```\n\n");
        }
        if !self.callers.is_empty() {
            out.push_str("## Callers\n\n");
            for n in &self.callers {
                out.push_str(&format!(
                    "- `{}` — {}:{}\n  `{}`\n",
                    n.symbol, n.file, n.line, n.signature
                ));
            }
            out.push('\n');
        }
        if !self.callees.is_empty() {
            out.push_str("## Callees\n\n");
            for n in &self.callees {
                out.push_str(&format!(
                    "- `{}` — {}:{}\n  `{}`\n",
                    n.symbol, n.file, n.line, n.signature
                ));
            }
            out.push('\n');
        }
        out
    }
}

/// Look up a symbol in `graph` and assemble its smallest useful
/// surrounding context: signature, body (optional), `use` imports,
/// top-N callers and callees.
pub fn ctx(
    graph: &ReferenceGraph,
    target: &str,
    include_body: bool,
    neighbor_limit: usize,
) -> Option<SymbolContext> {
    let idx = locate_symbol(graph, target)?;
    let sym = &graph.symbols[idx];

    let (signature, body, line_start, line_end) = read_definition(sym);
    let imports = read_imports(&sym.file);

    let callers = collect_callers(graph, idx, neighbor_limit);
    let callees = collect_callees(graph, sym, neighbor_limit);

    Some(SymbolContext {
        symbol: sym.symbol.qualified(),
        kind: sym.kind.as_str().to_string(),
        visibility: vis_str(sym.visibility).to_string(),
        file: sym.file.display().to_string(),
        line_start,
        line_end,
        signature,
        body: if include_body { Some(body) } else { None },
        imports,
        callers,
        callees,
    })
}

fn locate_symbol(graph: &ReferenceGraph, target: &str) -> Option<usize> {
    // Exact qualified match first.
    for (idx, s) in graph.symbols.iter().enumerate() {
        if s.symbol.qualified() == target {
            return Some(idx);
        }
    }
    // Trailing-segment / leaf match.
    for (idx, s) in graph.symbols.iter().enumerate() {
        let q = s.symbol.qualified();
        if q.ends_with(&format!("::{}", target)) || s.symbol.name == target {
            return Some(idx);
        }
    }
    None
}

fn vis_str(v: Visibility) -> &'static str {
    match v {
        Visibility::Public => "pub",
        Visibility::PubCrate => "pub(crate)",
        Visibility::Private => "private",
    }
}

/// Parse the file holding `sym` and return (signature, body, line_start, line_end).
/// Falls back to a line-based slice if syn can't find the item.
fn read_definition(sym: &SymbolNode) -> (String, String, usize, usize) {
    let Ok(text) = std::fs::read_to_string(&sym.file) else {
        return (String::new(), String::new(), sym.line, sym.line);
    };
    let Ok(ast) = syn::parse_file(&text) else {
        return (String::new(), String::new(), sym.line, sym.line);
    };

    let mut finder = DefFinder {
        target_name: &sym.symbol.name,
        target_kind: sym.kind,
        target_module: &sym.symbol.module_path,
        cur_module: Vec::new(),
        result: None,
    };
    finder.visit_file(&ast);

    if let Some((line_start, line_end, sig_line_end)) = finder.result {
        let lines: Vec<&str> = text.lines().collect();
        let sig = slice_lines(&lines, line_start, sig_line_end);
        let body = slice_lines(&lines, line_start, line_end);
        (sig, body, line_start, line_end)
    } else {
        (String::new(), String::new(), sym.line, sym.line)
    }
}

fn slice_lines(lines: &[&str], start: usize, end: usize) -> String {
    if start == 0 || end == 0 || start > lines.len() {
        return String::new();
    }
    let s = start.saturating_sub(1);
    let e = end.min(lines.len());
    lines[s..e].join("\n")
}

struct DefFinder<'a> {
    target_name: &'a str,
    target_kind: SymbolKind,
    target_module: &'a [String],
    cur_module: Vec<String>,
    /// (line_start, line_end, signature_line_end)
    result: Option<(usize, usize, usize)>,
}

impl<'a> DefFinder<'a> {
    fn module_matches(&self) -> bool {
        self.cur_module.as_slice() == self.target_module
    }

    fn record(&mut self, ident_line: usize, span_end_line: usize, sig_end: usize) {
        // Use ident line as start (skips outer doc-comments / attrs);
        // that mirrors what `graph.line` stores for the symbol.
        let _ = ident_line;
        // Actually we want the full item span including attributes — but
        // we lack a portable way to get that without retokenizing. Use
        // span_start of the whole item where possible (caller passes it
        // via ident_line for now). Keep API simple.
        self.result = Some((ident_line, span_end_line, sig_end));
    }
}

impl<'a, 'ast> Visit<'ast> for DefFinder<'a> {
    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        self.cur_module.push(i.ident.to_string());
        syn::visit::visit_item_mod(self, i);
        self.cur_module.pop();
    }
    fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
        if self.target_kind == SymbolKind::Fn
            && i.sig.ident == self.target_name
            && self.module_matches()
            && self.result.is_none()
        {
            let start = i.sig.fn_token.span().start().line;
            let end = i.block.span().end().line;
            let sig_end = i.sig.output.span().end().line.max(i.sig.ident.span().end().line);
            self.record(start, end, sig_end);
        }
        syn::visit::visit_item_fn(self, i);
    }
    fn visit_item_struct(&mut self, i: &'ast syn::ItemStruct) {
        if self.target_kind == SymbolKind::Struct
            && i.ident == self.target_name
            && self.module_matches()
            && self.result.is_none()
        {
            let start = i.struct_token.span().start().line;
            let end = i.span().end().line;
            self.record(start, end, end);
        }
        syn::visit::visit_item_struct(self, i);
    }
    fn visit_item_enum(&mut self, i: &'ast syn::ItemEnum) {
        if self.target_kind == SymbolKind::Enum
            && i.ident == self.target_name
            && self.module_matches()
            && self.result.is_none()
        {
            let start = i.enum_token.span().start().line;
            let end = i.span().end().line;
            self.record(start, end, end);
        }
        syn::visit::visit_item_enum(self, i);
    }
    fn visit_item_trait(&mut self, i: &'ast syn::ItemTrait) {
        if self.target_kind == SymbolKind::Trait
            && i.ident == self.target_name
            && self.module_matches()
            && self.result.is_none()
        {
            let start = i.trait_token.span().start().line;
            let end = i.span().end().line;
            self.record(start, end, end);
        }
        syn::visit::visit_item_trait(self, i);
    }
    fn visit_item_type(&mut self, i: &'ast syn::ItemType) {
        if self.target_kind == SymbolKind::TypeAlias
            && i.ident == self.target_name
            && self.module_matches()
            && self.result.is_none()
        {
            let start = i.type_token.span().start().line;
            let end = i.span().end().line;
            self.record(start, end, end);
        }
        syn::visit::visit_item_type(self, i);
    }
    fn visit_item_const(&mut self, i: &'ast syn::ItemConst) {
        if self.target_kind == SymbolKind::Const
            && i.ident == self.target_name
            && self.module_matches()
            && self.result.is_none()
        {
            let start = i.const_token.span().start().line;
            let end = i.span().end().line;
            self.record(start, end, end);
        }
        syn::visit::visit_item_const(self, i);
    }
    fn visit_item_static(&mut self, i: &'ast syn::ItemStatic) {
        if self.target_kind == SymbolKind::Static
            && i.ident == self.target_name
            && self.module_matches()
            && self.result.is_none()
        {
            let start = i.static_token.span().start().line;
            let end = i.span().end().line;
            self.record(start, end, end);
        }
        syn::visit::visit_item_static(self, i);
    }
}

fn read_imports(file: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("use ") || trimmed.starts_with("pub use ") {
            out.push(trimmed.trim_end().to_string());
        }
    }
    out
}

fn collect_callers(
    graph: &ReferenceGraph,
    target_idx: usize,
    limit: usize,
) -> Vec<CtxNeighbor> {
    let target = &graph.symbols[target_idx];
    let target_crate = &target.symbol.crate_name;
    let target_name = &target.symbol.name;

    // Find files that reference our target name; map each reference to
    // its enclosing top-level fn via line proximity.
    let mut hits: Vec<(PathBuf, usize)> = Vec::new();
    for r in &graph.references {
        if &r.name != target_name {
            continue;
        }
        // crate scoping (mirrors referenced_symbols logic, conservatively)
        let crate_ok = match r.root.as_deref() {
            None => true,
            Some("crate") | Some("self") | Some("super") | Some("Self") => {
                &r.from_crate == target_crate
            }
            Some(root) => root == target_crate || &r.from_crate == target_crate,
        };
        if !crate_ok {
            continue;
        }
        // Filter out the reference inside the definition itself.
        if r.from_file == target.file && r.line == target.line {
            continue;
        }
        hits.push((r.from_file.clone(), r.line));
    }

    // Resolve each (file, line) to its enclosing fn symbol by scanning
    // `graph.symbols` for fns in that file whose line <= ref.line and
    // taking the nearest one.
    let mut callers: Vec<CtxNeighbor> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (file, line) in hits {
        if let Some(enclosing) = enclosing_fn(graph, &file, line) {
            if enclosing == target_idx {
                continue;
            }
            let s = &graph.symbols[enclosing];
            let q = s.symbol.qualified();
            if !seen.insert(q.clone()) {
                continue;
            }
            callers.push(CtxNeighbor {
                signature: read_signature(s),
                symbol: q,
                file: s.file.display().to_string(),
                line: s.line,
            });
            if callers.len() >= limit {
                break;
            }
        }
    }
    callers
}

fn enclosing_fn(graph: &ReferenceGraph, file: &Path, line: usize) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None; // (idx, fn_line)
    for (idx, s) in graph.symbols.iter().enumerate() {
        if s.kind != SymbolKind::Fn {
            continue;
        }
        if s.file != file {
            continue;
        }
        if s.line > line {
            continue;
        }
        match best {
            Some((_, prev)) if prev >= s.line => {}
            _ => best = Some((idx, s.line)),
        }
    }
    best.map(|(idx, _)| idx)
}

fn collect_callees(
    graph: &ReferenceGraph,
    target: &SymbolNode,
    limit: usize,
) -> Vec<CtxNeighbor> {
    if target.kind != SymbolKind::Fn {
        return Vec::new();
    }
    // Find references emitted from the file of the target whose line
    // falls inside the fn body. Resolve each to a candidate symbol by
    // leaf-name match (best-effort, same heuristic as referenced_symbols).
    let (_sig, _body, line_start, line_end) = read_definition(target);

    let mut callee_names: Vec<(String, usize)> = Vec::new(); // (name, line)
    let mut seen_name: BTreeSet<String> = BTreeSet::new();
    for r in &graph.references {
        if r.from_file != target.file {
            continue;
        }
        if r.line < line_start || r.line > line_end {
            continue;
        }
        if r.name == target.symbol.name {
            continue;
        }
        if r.is_path_segment {
            continue;
        }
        if !seen_name.insert(r.name.clone()) {
            continue;
        }
        callee_names.push((r.name.clone(), r.line));
    }

    let mut out: Vec<CtxNeighbor> = Vec::new();
    let mut seen_q: BTreeSet<String> = BTreeSet::new();
    for (name, _ref_line) in callee_names {
        let Some(candidates) = graph.by_name.get(&name) else {
            continue;
        };
        // Prefer same-crate match.
        let mut idx_opt: Option<usize> = None;
        for &c in candidates {
            if graph.symbols[c].symbol.crate_name == target.symbol.crate_name {
                idx_opt = Some(c);
                break;
            }
        }
        let idx = idx_opt.or_else(|| candidates.first().copied());
        let Some(idx) = idx else { continue };
        let s = &graph.symbols[idx];
        if s.kind != SymbolKind::Fn {
            continue;
        }
        let q = s.symbol.qualified();
        if !seen_q.insert(q.clone()) {
            continue;
        }
        out.push(CtxNeighbor {
            signature: read_signature(s),
            symbol: q,
            file: s.file.display().to_string(),
            line: s.line,
        });
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn read_signature(sym: &SymbolNode) -> String {
    let Ok(text) = std::fs::read_to_string(&sym.file) else {
        return String::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    if sym.line == 0 || sym.line > lines.len() {
        return String::new();
    }
    // For an fn, the signature ends at the `{` opening the body — scan
    // forward up to a small budget.
    if sym.kind == SymbolKind::Fn {
        let start = sym.line.saturating_sub(1);
        let mut buf = String::new();
        for line in lines.iter().skip(start).take(10) {
            if !buf.is_empty() {
                buf.push(' ');
            }
            let trimmed = line.trim();
            if let Some((before, _)) = trimmed.split_once('{') {
                buf.push_str(before.trim_end());
                return collapse_ws(&buf);
            }
            buf.push_str(trimmed);
            if trimmed.ends_with(';') {
                break;
            }
        }
        return collapse_ws(&buf);
    }
    // Otherwise: just return the symbol's defining line, trimmed.
    let line = lines[sym.line - 1].trim();
    collapse_ws(line)
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// `wraith summarize <file>`
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct PubItemSummary {
    pub kind: String,
    pub name: String,
    pub signature: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cyclomatic: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cognitive: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileSummary {
    pub file: String,
    pub loc: usize,
    pub imports: Vec<String>,
    pub pub_items: Vec<PubItemSummary>,
    pub depends_on: Vec<String>,
}

impl FileSummary {
    pub fn render_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("# `{}`\n\n", self.file));
        out.push_str(&format!("LOC: {}\n\n", self.loc));
        if !self.pub_items.is_empty() {
            out.push_str("## Pub items\n\n");
            out.push_str("| kind | name | line | cyclo | cog |\n");
            out.push_str("|------|------|------|-------|-----|\n");
            for p in &self.pub_items {
                out.push_str(&format!(
                    "| {} | `{}` | {} | {} | {} |\n",
                    p.kind,
                    p.name,
                    p.line,
                    p.cyclomatic.map(|n| n.to_string()).unwrap_or_default(),
                    p.cognitive.map(|n| n.to_string()).unwrap_or_default(),
                ));
            }
            out.push_str("\n## Signatures\n\n```rust\n");
            for p in &self.pub_items {
                out.push_str(&p.signature);
                out.push('\n');
            }
            out.push_str("```\n\n");
        }
        if !self.imports.is_empty() {
            out.push_str("## Imports\n\n```rust\n");
            for u in &self.imports {
                out.push_str(u);
                out.push('\n');
            }
            out.push_str("```\n\n");
        }
        if !self.depends_on.is_empty() {
            out.push_str("## Depends on\n\n");
            for d in &self.depends_on {
                out.push_str(&format!("- `{}`\n", d));
            }
        }
        out
    }
}

pub fn summarize(file: &Path, include_bodies: bool) -> Option<FileSummary> {
    let _ = include_bodies; // reserved — bodies are out-of-band via `ctx`
    let text = std::fs::read_to_string(file).ok()?;
    let ast = syn::parse_file(&text).ok()?;
    let loc = text.lines().count();
    let imports = read_imports(file);
    let lines: Vec<&str> = text.lines().collect();

    let mut walker = SummaryWalker {
        lines: &lines,
        module_path: Vec::new(),
        items: Vec::new(),
        deps: BTreeSet::new(),
    };
    walker.visit_file(&ast);

    Some(FileSummary {
        file: file.display().to_string(),
        loc,
        imports,
        pub_items: walker.items,
        depends_on: walker.deps.into_iter().collect(),
    })
}

struct SummaryWalker<'a> {
    lines: &'a [&'a str],
    module_path: Vec<String>,
    items: Vec<PubItemSummary>,
    deps: BTreeSet<String>,
}

impl<'a> SummaryWalker<'a> {
    fn is_pub(&self, vis: &syn::Visibility) -> bool {
        matches!(
            vis,
            syn::Visibility::Public(_) | syn::Visibility::Restricted(_)
        )
    }

    fn fn_signature(&self, i: &syn::ItemFn) -> String {
        let line = i.sig.fn_token.span().start().line;
        let start = line.saturating_sub(1);
        let mut buf = String::new();
        for l in self.lines.iter().skip(start).take(20) {
            if !buf.is_empty() {
                buf.push(' ');
            }
            let trimmed = l.trim();
            if let Some((before, _)) = trimmed.split_once('{') {
                buf.push_str(before.trim_end());
                return collapse_ws(&buf);
            }
            buf.push_str(trimmed);
        }
        collapse_ws(&buf)
    }

    fn ident_line_signature(&self, line: usize) -> String {
        if line == 0 || line > self.lines.len() {
            return String::new();
        }
        collapse_ws(self.lines[line - 1].trim_end_matches('{').trim())
    }
}

impl<'a, 'ast> Visit<'ast> for SummaryWalker<'a> {
    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        if i.ident == "tests" {
            return;
        }
        self.module_path.push(i.ident.to_string());
        syn::visit::visit_item_mod(self, i);
        self.module_path.pop();
    }

    fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
        if i.attrs.iter().any(|a| a.path().is_ident("test")) {
            syn::visit::visit_item_fn(self, i);
            return;
        }
        if self.is_pub(&i.vis) {
            let mut cv = ComplexityVisitor::new();
            cv.visit_block(&i.block);
            let line = i.sig.fn_token.span().start().line;
            self.items.push(PubItemSummary {
                kind: "fn".to_string(),
                name: i.sig.ident.to_string(),
                signature: self.fn_signature(i),
                line,
                cyclomatic: Some(cv.cyclo()),
                cognitive: Some(cv.cog()),
            });
        }
        syn::visit::visit_item_fn(self, i);
    }

    fn visit_item_struct(&mut self, i: &'ast syn::ItemStruct) {
        if self.is_pub(&i.vis) {
            let line = i.struct_token.span().start().line;
            self.items.push(PubItemSummary {
                kind: "struct".to_string(),
                name: i.ident.to_string(),
                signature: self.ident_line_signature(line),
                line,
                cyclomatic: None,
                cognitive: None,
            });
        }
        syn::visit::visit_item_struct(self, i);
    }

    fn visit_item_enum(&mut self, i: &'ast syn::ItemEnum) {
        if self.is_pub(&i.vis) {
            let line = i.enum_token.span().start().line;
            self.items.push(PubItemSummary {
                kind: "enum".to_string(),
                name: i.ident.to_string(),
                signature: self.ident_line_signature(line),
                line,
                cyclomatic: None,
                cognitive: None,
            });
        }
        syn::visit::visit_item_enum(self, i);
    }

    fn visit_item_trait(&mut self, i: &'ast syn::ItemTrait) {
        if self.is_pub(&i.vis) {
            let line = i.trait_token.span().start().line;
            self.items.push(PubItemSummary {
                kind: "trait".to_string(),
                name: i.ident.to_string(),
                signature: self.ident_line_signature(line),
                line,
                cyclomatic: None,
                cognitive: None,
            });
        }
        syn::visit::visit_item_trait(self, i);
    }

    fn visit_item_type(&mut self, i: &'ast syn::ItemType) {
        if self.is_pub(&i.vis) {
            let line = i.type_token.span().start().line;
            self.items.push(PubItemSummary {
                kind: "type".to_string(),
                name: i.ident.to_string(),
                signature: self.ident_line_signature(line),
                line,
                cyclomatic: None,
                cognitive: None,
            });
        }
        syn::visit::visit_item_type(self, i);
    }

    fn visit_item_const(&mut self, i: &'ast syn::ItemConst) {
        if self.is_pub(&i.vis) {
            let line = i.const_token.span().start().line;
            self.items.push(PubItemSummary {
                kind: "const".to_string(),
                name: i.ident.to_string(),
                signature: self.ident_line_signature(line),
                line,
                cyclomatic: None,
                cognitive: None,
            });
        }
        syn::visit::visit_item_const(self, i);
    }

    fn visit_item_static(&mut self, i: &'ast syn::ItemStatic) {
        if self.is_pub(&i.vis) {
            let line = i.static_token.span().start().line;
            self.items.push(PubItemSummary {
                kind: "static".to_string(),
                name: i.ident.to_string(),
                signature: self.ident_line_signature(line),
                line,
                cyclomatic: None,
                cognitive: None,
            });
        }
        syn::visit::visit_item_static(self, i);
    }

    fn visit_path(&mut self, p: &'ast syn::Path) {
        if p.segments.len() >= 2 {
            let head = &p.segments[0].ident.to_string();
            if !matches!(head.as_str(), "self" | "super" | "crate" | "Self") {
                self.deps.insert(head.to_string());
            }
        }
        syn::visit::visit_path(self, p);
    }
}

// ---------------------------------------------------------------------------
// `wraith ls [pattern]`
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct LsResult {
    pub symbol: String,
    pub kind: String,
    pub visibility: String,
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct LsResults {
    pub pattern: String,
    pub results: Vec<LsResult>,
    pub total: usize,
}

impl LsResults {
    pub fn render_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "# `wraith ls {}` — {} match(es)\n\n",
            self.pattern, self.total
        ));
        if self.results.is_empty() {
            out.push_str("_No matches._\n");
            return out;
        }
        out.push_str("| kind | symbol | vis | file:line |\n");
        out.push_str("|------|--------|-----|-----------|\n");
        for r in &self.results {
            out.push_str(&format!(
                "| {} | `{}` | {} | {}:{} |\n",
                r.kind, r.symbol, r.visibility, r.file, r.line,
            ));
        }
        out
    }
}

pub fn ls(graph: &ReferenceGraph, pattern: &str, kind_filter: Option<SymbolKind>) -> LsResults {
    let glob = Glob::compile(pattern);
    let mut out: Vec<LsResult> = Vec::new();
    for sym in &graph.symbols {
        if sym.is_test {
            continue;
        }
        if let Some(k) = kind_filter {
            if sym.kind != k {
                continue;
            }
        }
        let q = sym.symbol.qualified();
        let leaf = &sym.symbol.name;
        if !glob.matches(&q) && !glob.matches(leaf) {
            continue;
        }
        out.push(LsResult {
            symbol: q,
            kind: sym.kind.as_str().to_string(),
            visibility: vis_str(sym.visibility).to_string(),
            file: sym.file.display().to_string(),
            line: sym.line,
        });
    }
    out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    let total = out.len();
    LsResults {
        pattern: pattern.to_string(),
        results: out,
        total,
    }
}

/// Minimal glob: `*` matches any run of non-`::` characters; `**`
/// matches anything; everything else literal. Empty/`*` matches all.
struct Glob {
    parts: Vec<GlobPart>,
}

enum GlobPart {
    Literal(String),
    Star,
    DoubleStar,
}

impl Glob {
    fn compile(pat: &str) -> Self {
        if pat.is_empty() {
            return Glob {
                parts: vec![GlobPart::DoubleStar],
            };
        }
        let mut parts: Vec<GlobPart> = Vec::new();
        let mut buf = String::new();
        let mut chars = pat.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '*' {
                if !buf.is_empty() {
                    parts.push(GlobPart::Literal(std::mem::take(&mut buf)));
                }
                if chars.peek() == Some(&'*') {
                    chars.next();
                    parts.push(GlobPart::DoubleStar);
                } else {
                    parts.push(GlobPart::Star);
                }
            } else {
                buf.push(c);
            }
        }
        if !buf.is_empty() {
            parts.push(GlobPart::Literal(buf));
        }
        Glob { parts }
    }

    fn matches(&self, s: &str) -> bool {
        glob_match(&self.parts, s)
    }
}

fn glob_match(parts: &[GlobPart], s: &str) -> bool {
    match parts.first() {
        None => s.is_empty(),
        Some(GlobPart::Literal(lit)) => {
            if let Some(rest) = s.strip_prefix(lit.as_str()) {
                glob_match(&parts[1..], rest)
            } else {
                false
            }
        }
        Some(GlobPart::Star) => {
            // `*` does not cross `::` (segment boundary).
            for (i, _) in s.char_indices() {
                if s[..i].contains("::") {
                    break;
                }
                if glob_match(&parts[1..], &s[i..]) {
                    return true;
                }
            }
            // also match the whole tail if it has no `::`
            if !s.contains("::") && glob_match(&parts[1..], "") {
                return true;
            }
            false
        }
        Some(GlobPart::DoubleStar) => {
            for (i, _) in s.char_indices() {
                if glob_match(&parts[1..], &s[i..]) {
                    return true;
                }
            }
            glob_match(&parts[1..], "")
        }
    }
}

pub fn parse_kind(s: &str) -> Option<SymbolKind> {
    match s {
        "fn" => Some(SymbolKind::Fn),
        "struct" => Some(SymbolKind::Struct),
        "enum" => Some(SymbolKind::Enum),
        "trait" => Some(SymbolKind::Trait),
        "type" => Some(SymbolKind::TypeAlias),
        "const" => Some(SymbolKind::Const),
        "static" => Some(SymbolKind::Static),
        "mod" => Some(SymbolKind::Mod),
        _ => None,
    }
}
