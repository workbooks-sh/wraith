use crate::graph::{Reference, ReferenceGraph, Symbol, SymbolKind, SymbolNode, Visibility};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use syn::spanned::Spanned;
use syn::visit::Visit;

/// State carried through the file walker.
struct FileWalker<'a> {
    graph: &'a mut ReferenceGraph,
    crate_name: String,
    file: PathBuf,
    module_path: Vec<String>,
    /// Are we inside `mod tests { ... }` (or similar `#[cfg(test)]` module)?
    in_tests: bool,
    /// Are we inside an `impl` block?
    in_impl_depth: u32,
    /// Are we inside an `impl Trait for Type` for a non-local trait? (best-effort).
    impl_foreign_trait: bool,
    /// References observed at the current file (mostly from `use` and path exprs).
    used_names: HashSet<String>,
}

pub fn parse_file(graph: &mut ReferenceGraph, crate_name: &str, file: &Path) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(file)?;
    let ast = match syn::parse_file(&text) {
        Ok(a) => a,
        // best-effort: skip files we can't parse
        Err(_) => return Ok(()),
    };

    let mut walker = FileWalker {
        graph,
        crate_name: crate_name.to_string(),
        file: file.to_path_buf(),
        module_path: Vec::new(),
        in_tests: false,
        in_impl_depth: 0,
        impl_foreign_trait: false,
        used_names: HashSet::new(),
    };

    walker.visit_file(&ast);
    Ok(())
}

fn vis_of(v: &syn::Visibility) -> Visibility {
    match v {
        syn::Visibility::Public(_) => Visibility::Public,
        syn::Visibility::Restricted(r) => {
            if r.path.is_ident("crate") {
                Visibility::PubCrate
            } else {
                // pub(super), pub(in path) — treat as private-ish for dead-code
                Visibility::Private
            }
        }
        syn::Visibility::Inherited => Visibility::Private,
    }
}

fn has_attr(attrs: &[syn::Attribute], name: &str) -> bool {
    attrs.iter().any(|a| a.path().is_ident(name))
}

fn attr_path_matches(attrs: &[syn::Attribute], segs: &[&str]) -> bool {
    attrs.iter().any(|a| {
        let p = a.path();
        if p.segments.len() != segs.len() {
            return false;
        }
        p.segments
            .iter()
            .zip(segs.iter())
            .all(|(s, want)| s.ident == *want)
    })
}

fn is_cfg_test_attr(a: &syn::Attribute) -> bool {
    if !a.path().is_ident("cfg") {
        return false;
    }
    if let syn::Meta::List(list) = &a.meta {
        return list.tokens.to_string().contains("test");
    }
    false
}

fn linecol(span: proc_macro2::Span) -> (usize, usize) {
    let s = span.start();
    (s.line, s.column + 1)
}

impl<'a, 'ast> Visit<'ast> for FileWalker<'a> {
    fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
        let is_test = has_attr(&i.attrs, "test") || has_attr(&i.attrs, "bench");
        let no_mangle = has_attr(&i.attrs, "no_mangle");
        let export_name = has_attr(&i.attrs, "export_name");
        let visibility = vis_of(&i.vis);
        let (line, col) = linecol(i.sig.ident.span());
        let name = i.sig.ident.to_string();
        let is_entry = name == "main" && self.module_path.is_empty();

        if !self.in_tests {
            self.graph.add_symbol(SymbolNode {
                symbol: Symbol {
                    crate_name: self.crate_name.clone(),
                    module_path: self.module_path.clone(),
                    name: name.clone(),
                },
                kind: SymbolKind::Fn,
                visibility,
                file: self.file.clone(),
                line,
                col,
                is_test,
                is_entry_point: is_entry,
                has_no_mangle: no_mangle,
                has_export_name: export_name,
                is_reexport: false,
                impl_of_foreign_trait: self.impl_foreign_trait && self.in_impl_depth > 0,
            });
        }

        syn::visit::visit_item_fn(self, i);
    }

    fn visit_item_struct(&mut self, i: &'ast syn::ItemStruct) {
        if !self.in_tests {
            let (line, col) = linecol(i.ident.span());
            self.graph.add_symbol(SymbolNode {
                symbol: Symbol {
                    crate_name: self.crate_name.clone(),
                    module_path: self.module_path.clone(),
                    name: i.ident.to_string(),
                },
                kind: SymbolKind::Struct,
                visibility: vis_of(&i.vis),
                file: self.file.clone(),
                line,
                col,
                is_test: false,
                is_entry_point: false,
                has_no_mangle: has_attr(&i.attrs, "no_mangle"),
                has_export_name: has_attr(&i.attrs, "export_name"),
                is_reexport: false,
                impl_of_foreign_trait: false,
            });
        }
        syn::visit::visit_item_struct(self, i);
    }

    fn visit_item_enum(&mut self, i: &'ast syn::ItemEnum) {
        if !self.in_tests {
            let (line, col) = linecol(i.ident.span());
            self.graph.add_symbol(SymbolNode {
                symbol: Symbol {
                    crate_name: self.crate_name.clone(),
                    module_path: self.module_path.clone(),
                    name: i.ident.to_string(),
                },
                kind: SymbolKind::Enum,
                visibility: vis_of(&i.vis),
                file: self.file.clone(),
                line,
                col,
                is_test: false,
                is_entry_point: false,
                has_no_mangle: false,
                has_export_name: false,
                is_reexport: false,
                impl_of_foreign_trait: false,
            });
        }
        syn::visit::visit_item_enum(self, i);
    }

    fn visit_item_trait(&mut self, i: &'ast syn::ItemTrait) {
        if !self.in_tests {
            let (line, col) = linecol(i.ident.span());
            self.graph.add_symbol(SymbolNode {
                symbol: Symbol {
                    crate_name: self.crate_name.clone(),
                    module_path: self.module_path.clone(),
                    name: i.ident.to_string(),
                },
                kind: SymbolKind::Trait,
                visibility: vis_of(&i.vis),
                file: self.file.clone(),
                line,
                col,
                is_test: false,
                is_entry_point: false,
                has_no_mangle: false,
                has_export_name: false,
                is_reexport: false,
                impl_of_foreign_trait: false,
            });
        }
        syn::visit::visit_item_trait(self, i);
    }

    fn visit_item_type(&mut self, i: &'ast syn::ItemType) {
        if !self.in_tests {
            let (line, col) = linecol(i.ident.span());
            self.graph.add_symbol(SymbolNode {
                symbol: Symbol {
                    crate_name: self.crate_name.clone(),
                    module_path: self.module_path.clone(),
                    name: i.ident.to_string(),
                },
                kind: SymbolKind::TypeAlias,
                visibility: vis_of(&i.vis),
                file: self.file.clone(),
                line,
                col,
                is_test: false,
                is_entry_point: false,
                has_no_mangle: false,
                has_export_name: false,
                is_reexport: false,
                impl_of_foreign_trait: false,
            });
        }
        syn::visit::visit_item_type(self, i);
    }

    fn visit_item_const(&mut self, i: &'ast syn::ItemConst) {
        if !self.in_tests {
            let (line, col) = linecol(i.ident.span());
            self.graph.add_symbol(SymbolNode {
                symbol: Symbol {
                    crate_name: self.crate_name.clone(),
                    module_path: self.module_path.clone(),
                    name: i.ident.to_string(),
                },
                kind: SymbolKind::Const,
                visibility: vis_of(&i.vis),
                file: self.file.clone(),
                line,
                col,
                is_test: false,
                is_entry_point: false,
                has_no_mangle: false,
                has_export_name: false,
                is_reexport: false,
                impl_of_foreign_trait: false,
            });
        }
        syn::visit::visit_item_const(self, i);
    }

    fn visit_item_static(&mut self, i: &'ast syn::ItemStatic) {
        if !self.in_tests {
            let (line, col) = linecol(i.ident.span());
            self.graph.add_symbol(SymbolNode {
                symbol: Symbol {
                    crate_name: self.crate_name.clone(),
                    module_path: self.module_path.clone(),
                    name: i.ident.to_string(),
                },
                kind: SymbolKind::Static,
                visibility: vis_of(&i.vis),
                file: self.file.clone(),
                line,
                col,
                is_test: false,
                is_entry_point: false,
                has_no_mangle: has_attr(&i.attrs, "no_mangle"),
                has_export_name: has_attr(&i.attrs, "export_name"),
                is_reexport: false,
                impl_of_foreign_trait: false,
            });
        }
        syn::visit::visit_item_static(self, i);
    }

    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        // skip tests modules
        let is_tests_mod = i.ident == "tests"
            || i.attrs.iter().any(|a| is_cfg_test_attr(a))
            || attr_path_matches(&i.attrs, &["cfg_attr"]);
        let was = self.in_tests;
        if is_tests_mod {
            self.in_tests = true;
        }
        self.module_path.push(i.ident.to_string());
        syn::visit::visit_item_mod(self, i);
        self.module_path.pop();
        self.in_tests = was;
    }

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        // detect "impl Trait for Type" — if Trait path's first segment is
        // an extern crate or "std"/"core"/"alloc", treat as foreign trait.
        let mut foreign = false;
        if let Some((_, ref path, _)) = i.trait_ {
            if let Some(first) = path.segments.first() {
                let s = first.ident.to_string();
                if matches!(s.as_str(), "std" | "core" | "alloc" | "serde" | "Iterator" | "From" | "Into" | "TryFrom" | "TryInto" | "Display" | "Debug" | "Clone" | "Default")
                {
                    foreign = true;
                }
                // also if first segment isn't a local mod / crate / self / super.
                if !matches!(s.as_str(), "crate" | "self" | "super") {
                    foreign = true;
                }
            }
        }
        let prev = self.impl_foreign_trait;
        self.impl_foreign_trait = foreign;
        self.in_impl_depth += 1;
        // also record reference to the trait, so we can mark the trait alive
        if let Some((_, ref path, _)) = i.trait_ {
            self.record_path_ref(path);
        }
        syn::visit::visit_item_impl(self, i);
        self.in_impl_depth -= 1;
        self.impl_foreign_trait = prev;
    }

    fn visit_item_use(&mut self, i: &'ast syn::ItemUse) {
        // walk the use tree to collect references
        collect_use_refs(&i.tree, &mut self.used_names);
        // also push into graph as references
        let mut idents: Vec<(Option<String>, String)> = Vec::new();
        walk_use_tree(&i.tree, None, &mut idents);
        for (root, name) in idents {
            let segments: Vec<String> = match &root {
                Some(r) if r != &name => vec![r.clone(), name.clone()],
                _ => vec![name.clone()],
            };
            self.graph.add_reference(Reference {
                from_file: self.file.clone(),
                from_crate: self.crate_name.clone(),
                name,
                root,
                segments,
                is_path_segment: false,
                line: i.span().start().line,
            });
        }
        syn::visit::visit_item_use(self, i);
    }

    fn visit_path(&mut self, p: &'ast syn::Path) {
        self.record_path_ref(p);
        syn::visit::visit_path(self, p);
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        // record macro name references; macro definitions themselves are
        // out of scope (DEFER per brief).
        if let Some(seg) = m.path.segments.last() {
            let all_segs: Vec<String> =
                m.path.segments.iter().map(|s| s.ident.to_string()).collect();
            let root = m.path.segments.first().map(|s| s.ident.to_string());
            let root = if m.path.segments.len() > 1 { root } else { None };
            self.graph.add_reference(Reference {
                from_file: self.file.clone(),
                from_crate: self.crate_name.clone(),
                name: seg.ident.to_string(),
                root,
                segments: all_segs,
                is_path_segment: false,
                line: m.path.span().start().line,
            });
        }
        // Macro bodies are raw token trees — syn::visit doesn't walk
        // into them, so anything interpolated via `format!("{X}")` or
        // passed positionally to a macro otherwise looks unreferenced.
        // Mark every ident we see (and every format-arg name in a string
        // literal) as a best-effort reference. Better to under-report
        // dead code than to delete a used item.
        self.scan_macro_tokens(&m.tokens, m.path.span().start().line);
        syn::visit::visit_macro(self, m);
    }

    fn visit_attribute(&mut self, a: &'ast syn::Attribute) {
        // attribute paths may reference traits via #[derive(Foo)]; pick those up.
        if a.path().is_ident("derive") {
            if let syn::Meta::List(list) = &a.meta {
                // tokens like `Clone, Debug, MyTrait`
                let s = list.tokens.to_string();
                for piece in s.split(',') {
                    let name = piece.trim().split("::").last().unwrap_or("").trim().to_string();
                    if !name.is_empty() {
                        let segments = vec![name.clone()];
                        self.graph.add_reference(Reference {
                            from_file: self.file.clone(),
                            from_crate: self.crate_name.clone(),
                            name,
                            root: None,
                            segments,
                            is_path_segment: false,
                            line: a.span().start().line,
                        });
                    }
                }
            }
        }
        syn::visit::visit_attribute(self, a);
    }
}

impl<'a> FileWalker<'a> {
    /// Walk a raw macro token stream and emit a reference for every
    /// identifier seen, plus every `{ident}` format-arg in any string
    /// literal. Macro bodies are tokens, not AST, so syn::visit doesn't
    /// descend into them — without this, `format!("{X}")` and any
    /// positionally-passed ident inside a macro look unreferenced and
    /// `fix --apply` would delete them.
    fn scan_macro_tokens(&mut self, tokens: &proc_macro2::TokenStream, fallback_line: usize) {
        use proc_macro2::TokenTree;
        for tt in tokens.clone() {
            match tt {
                TokenTree::Ident(id) => {
                    let name = id.to_string();
                    if is_primitive_or_noise(&name) {
                        continue;
                    }
                    let line = id.span().start().line;
                    let line = if line == 0 { fallback_line } else { line };
                    let segments = vec![name.clone()];
                    self.graph.add_reference(Reference {
                        from_file: self.file.clone(),
                        from_crate: self.crate_name.clone(),
                        name,
                        root: None,
                        segments,
                        is_path_segment: false,
                        line,
                    });
                }
                TokenTree::Group(g) => {
                    self.scan_macro_tokens(&g.stream(), fallback_line);
                }
                TokenTree::Literal(lit) => {
                    // Format-string literals (`"{ident}"`, `"{x:?}"`)
                    // implicitly reference idents; without this scan,
                    // a const used only via `format!("{X}")` looks dead.
                    let s = lit.to_string();
                    let mut emitted: Vec<String> = Vec::new();
                    scan_format_string_idents(&s, |ident| {
                        if !is_primitive_or_noise(ident) {
                            emitted.push(ident.to_string());
                        }
                    });
                    for ident in emitted {
                        let segments = vec![ident.clone()];
                        self.graph.add_reference(Reference {
                            from_file: self.file.clone(),
                            from_crate: self.crate_name.clone(),
                            name: ident,
                            root: None,
                            segments,
                            is_path_segment: false,
                            line: fallback_line,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    fn record_path_ref(&mut self, p: &syn::Path) {
        let segs: Vec<String> = p.segments.iter().map(|s| s.ident.to_string()).collect();
        if segs.is_empty() {
            return;
        }
        let last = segs.last().unwrap().clone();
        // Filter out primitive types and obvious noise.
        if is_primitive_or_noise(&last) {
            return;
        }
        let root = if segs.len() > 1 {
            Some(segs[0].clone())
        } else {
            None
        };
        self.graph.add_reference(Reference {
            from_file: self.file.clone(),
            from_crate: self.crate_name.clone(),
            name: last,
            root,
            segments: segs.clone(),
            is_path_segment: false,
            line: p.span().start().line,
        });
        // Also record intermediate segments — `foo::bar::baz` should also mark
        // `bar` alive (since it could be a re-exported mod or fn).
        for (i, s) in segs.iter().enumerate() {
            if i == 0 || i == segs.len() - 1 {
                continue;
            }
            if is_primitive_or_noise(s) {
                continue;
            }
            self.graph.add_reference(Reference {
                from_file: self.file.clone(),
                from_crate: self.crate_name.clone(),
                name: s.clone(),
                root: Some(segs[0].clone()),
                segments: segs[..=i].to_vec(),
                is_path_segment: true,
                line: p.span().start().line,
            });
        }
    }
}

/// Best-effort scan of a raw string-literal token (including the
/// surrounding quotes) for `{ident}` or `{ident:fmt}` interpolation
/// specs. Calls `cb` with each captured identifier name. Conservatively
/// ignores `{{` escapes and positional / indexed forms.
fn scan_format_string_idents(s: &str, mut cb: impl FnMut(&str)) {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if bytes.get(i + 1) == Some(&b'{') {
                i += 2;
                continue;
            }
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end] != b'}' && bytes[end] != b':' {
                end += 1;
            }
            if end > start {
                let chunk = &s[start..end];
                let is_ident = chunk
                    .chars()
                    .next()
                    .map(|c| c.is_alphabetic() || c == '_')
                    .unwrap_or(false)
                    && chunk
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_');
                if is_ident {
                    cb(chunk);
                }
            }
            i = end + 1;
            continue;
        }
        i += 1;
    }
}

fn is_primitive_or_noise(s: &str) -> bool {
    matches!(
        s,
        "bool"
            | "char"
            | "str"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "f32"
            | "f64"
            | "Self"
            | "self"
            | "_"
    )
}

fn collect_use_refs(tree: &syn::UseTree, into: &mut HashSet<String>) {
    match tree {
        syn::UseTree::Path(p) => {
            into.insert(p.ident.to_string());
            collect_use_refs(&p.tree, into);
        }
        syn::UseTree::Name(n) => {
            into.insert(n.ident.to_string());
        }
        syn::UseTree::Rename(r) => {
            into.insert(r.ident.to_string());
        }
        syn::UseTree::Glob(_) => {}
        syn::UseTree::Group(g) => {
            for t in &g.items {
                collect_use_refs(t, into);
            }
        }
    }
}

fn walk_use_tree(
    tree: &syn::UseTree,
    current_root: Option<String>,
    out: &mut Vec<(Option<String>, String)>,
) {
    match tree {
        syn::UseTree::Path(p) => {
            let new_root = current_root.clone().or_else(|| Some(p.ident.to_string()));
            // record the segment itself
            out.push((current_root.clone(), p.ident.to_string()));
            walk_use_tree(&p.tree, new_root, out);
        }
        syn::UseTree::Name(n) => {
            out.push((current_root, n.ident.to_string()));
        }
        syn::UseTree::Rename(r) => {
            out.push((current_root, r.ident.to_string()));
        }
        syn::UseTree::Glob(_) => {}
        syn::UseTree::Group(g) => {
            for t in &g.items {
                walk_use_tree(t, current_root.clone(), out);
            }
        }
    }
}
