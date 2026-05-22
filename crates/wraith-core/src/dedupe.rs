//! `dedupe-cluster` refactor — collapse byte-identical members of a
//! duplicate cluster into a single canonical definition (wb-5lgj.28).
//!
//! Pre-condition: every member of the cluster has token-identical body
//! (similarity == 1.00 against every other member). Similar-but-not-
//! identical clusters are routed to `wraith refactor unify-cluster`
//! (wb-5lgj.30).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use quote::ToTokens;
use syn::visit::Visit;

use crate::graph::{ReferenceGraph, Visibility};
use crate::report::{ClusterMember, FindingKind, Finding};

#[derive(Debug, thiserror::Error)]
pub enum DedupeError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: syn::Error,
    },
    #[error("finding is not a duplicate-cluster finding")]
    NotACluster,
    #[error("cluster has non-identical members; use 'wraith refactor unify-cluster' (wb-5lgj.30) for similar-but-not-identical refactors")]
    NotByteIdentical,
    #[error("cluster member `{symbol}` was not located in {file}")]
    MemberNotFound { symbol: String, file: PathBuf },
    #[error("--canonical=`{spec}` does not match any cluster member")]
    CanonicalNotFound { spec: String },
    #[error(
        "cluster members reference different definitions of `{name}`:\n  - {member_a} @ {file_a} uses {def_a}\n  - {member_b} @ {file_b} uses {def_b}\nCannot safely dedupe — consider 'wraith refactor extract-shared' with the const as a parameter."
    )]
    ScopeDivergence {
        name: String,
        member_a: String,
        file_a: String,
        def_a: String,
        member_b: String,
        file_b: String,
        def_b: String,
    },
    #[error(
        "canonical `{canonical}` is {current}; consumers need at least {needed}. Pass --elevate-canonical-to={needed} (or drop --no-elevate)."
    )]
    CanonicalInsufficientVisibility {
        canonical: String,
        current: &'static str,
        needed: &'static str,
    },
}

#[derive(Debug, Clone, Default)]
pub struct DedupeOptions {
    /// Optional canonical selector — accepts a qualified symbol
    /// (e.g. `crate_a::pick_ext`), a leaf symbol (`pick_ext`), or a
    /// file path / crate name. If unset, the lex-first member by
    /// (file, line) wins.
    pub canonical: Option<String>,
    /// If set, refuse to elevate visibility on the canonical fn. When
    /// the canonical is too restrictive for consumers, return
    /// `CanonicalInsufficientVisibility` instead of rewriting it.
    pub no_elevate: bool,
}

/// Side-effect notice emitted alongside a successful dedupe — e.g.
/// "elevated visibility from private to pub(crate)". CLI prints these
/// in dry-run + apply output.
#[derive(Debug, Clone)]
pub struct DedupeNotice {
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct FileEdit {
    pub path: PathBuf,
    pub new_contents: String,
}

#[derive(Debug, Clone)]
pub struct DedupeResult {
    pub canonical: ClusterMember,
    pub removed: Vec<ClusterMember>,
    pub edits: Vec<FileEdit>,
    pub notices: Vec<DedupeNotice>,
}

pub fn dedupe_cluster_from_finding(
    finding: &Finding,
    opts: &DedupeOptions,
    graph: &ReferenceGraph,
) -> Result<DedupeResult, DedupeError> {
    let members = match &finding.kind {
        FindingKind::DuplicateCluster { members, .. } => members.clone(),
        _ => return Err(DedupeError::NotACluster),
    };
    dedupe_members(&members, opts, graph)
}

pub fn dedupe_members(
    members: &[ClusterMember],
    opts: &DedupeOptions,
    graph: &ReferenceGraph,
) -> Result<DedupeResult, DedupeError> {
    let resolved: Vec<ResolvedMember> = members
        .iter()
        .map(resolve_member)
        .collect::<Result<_, _>>()?;

    assert_all_byte_identical(&resolved)?;
    assert_free_names_agree(&resolved, graph)?;

    let canonical_idx = pick_canonical(&resolved, opts.canonical.as_deref())?;
    let canonical = &resolved[canonical_idx];

    // Group non-canonical members by source file so we apply edits per
    // file (multiple removed fns in the same file collapse into one
    // edit).
    let mut per_file: BTreeMap<PathBuf, Vec<&ResolvedMember>> = BTreeMap::new();
    for (i, m) in resolved.iter().enumerate() {
        if i == canonical_idx {
            continue;
        }
        per_file.entry(m.file.clone()).or_default().push(m);
    }

    // Visibility planning — does the canonical need to be elevated for
    // consumers in other crates / modules?
    let needs_cross_crate = per_file
        .values()
        .any(|v| v.iter().any(|m| m.crate_name != canonical.crate_name));
    let needed: Visibility = if needs_cross_crate {
        Visibility::Public
    } else if !per_file.is_empty() {
        Visibility::PubCrate
    } else {
        canonical.visibility
    };
    let needs_elevation = vis_rank(canonical.visibility) < vis_rank(needed);
    let mut notices: Vec<DedupeNotice> = Vec::new();
    if needs_elevation {
        if opts.no_elevate {
            return Err(DedupeError::CanonicalInsufficientVisibility {
                canonical: canonical.member.symbol.clone(),
                current: vis_label(canonical.visibility),
                needed: vis_label(needed),
            });
        }
        notices.push(DedupeNotice {
            message: format!(
                "note: elevating {} visibility from {} to {}",
                canonical.member.symbol,
                vis_label(canonical.visibility),
                vis_label(needed),
            ),
        });
    }

    let mut edits = Vec::new();
    for (file, removed_here) in &per_file {
        let edit = rewrite_file(file, removed_here, canonical)?;
        edits.push(edit);
    }

    // Apply visibility elevation to the canonical file. If we already
    // have an edit pending on the canonical file, fold the elevation
    // into it; otherwise create a new edit.
    if needs_elevation {
        let elevated = elevate_canonical_vis(canonical, needed)?;
        if let Some(existing) = edits.iter_mut().find(|e| e.path == elevated.path) {
            // shouldn't happen — canonical lives in a file with no removed
            // members by construction — but fold defensively.
            existing.new_contents = re_elevate_in(&existing.new_contents, canonical, needed);
        } else {
            edits.push(elevated);
        }
    }

    Ok(DedupeResult {
        canonical: canonical.member.clone(),
        removed: per_file
            .values()
            .flat_map(|v| v.iter().map(|r| r.member.clone()))
            .collect(),
        edits,
        notices,
    })
}

fn vis_rank(v: Visibility) -> u8 {
    match v {
        Visibility::Private => 0,
        Visibility::PubCrate => 1,
        Visibility::Public => 2,
    }
}

fn vis_label(v: Visibility) -> &'static str {
    match v {
        Visibility::Private => "private",
        Visibility::PubCrate => "pub(crate)",
        Visibility::Public => "pub",
    }
}

#[derive(Debug, Clone)]
struct ResolvedMember {
    member: ClusterMember,
    file: PathBuf,
    crate_name: String,
    leaf_name: String,
    body_tokens: String,
    fn_byte_start: usize,
    fn_byte_end: usize,
    fn_line_start: usize,
    visibility: Visibility,
    /// Free uppercase-leading identifiers used in the fn body — these
    /// resolve to consts / statics / types and must agree across all
    /// cluster members (wb-5lgj.37).
    upper_free_names: BTreeSet<String>,
    /// Line where the fn keyword (and `pub` token, if any) starts —
    /// used to rewrite visibility (wb-5lgj.39).
    fn_keyword_line: usize,
}

fn resolve_member(m: &ClusterMember) -> Result<ResolvedMember, DedupeError> {
    let file = PathBuf::from(&m.file);
    let src = std::fs::read_to_string(&file).map_err(|e| DedupeError::Io {
        path: file.clone(),
        source: e,
    })?;
    let ast = syn::parse_file(&src).map_err(|e| DedupeError::Parse {
        path: file.clone(),
        source: e,
    })?;

    let (crate_name, leaf_name) = split_qualified(&m.symbol);
    let item = find_fn_by_name_and_line(&ast, &leaf_name, m.line)
        .ok_or_else(|| DedupeError::MemberNotFound {
            symbol: m.symbol.clone(),
            file: file.clone(),
        })?;

    let (fn_byte_start, fn_byte_end, fn_line_start) = fn_span_bytes(&src, item);
    let body_tokens = normalize_fn_body_string(item);
    let visibility = vis_of_syn(&item.vis);
    let upper_free_names = collect_upper_free_names(item);
    let fn_keyword_line = item.sig.fn_token.span.start().line;

    Ok(ResolvedMember {
        member: m.clone(),
        file,
        crate_name,
        leaf_name,
        body_tokens,
        fn_byte_start,
        fn_byte_end,
        fn_line_start,
        visibility,
        upper_free_names,
        fn_keyword_line,
    })
}

fn vis_of_syn(v: &syn::Visibility) -> Visibility {
    match v {
        syn::Visibility::Public(_) => Visibility::Public,
        syn::Visibility::Restricted(r) => {
            if r.path.is_ident("crate") {
                Visibility::PubCrate
            } else {
                Visibility::Private
            }
        }
        syn::Visibility::Inherited => Visibility::Private,
    }
}

/// Walk an `ItemFn` body and collect single-segment path identifiers
/// that start with an uppercase ASCII letter and are not bound as
/// params, locals, or generic type params. The output is the set of
/// "free" uppercase names — typically module-local consts, statics,
/// or top-level types — whose definitions must be the same across
/// cluster members for the dedupe to be safe (wb-5lgj.37).
fn collect_upper_free_names(item: &syn::ItemFn) -> BTreeSet<String> {
    let mut bound: BTreeSet<String> = BTreeSet::new();
    // Params (incl. patterns) contribute bound names.
    for input in &item.sig.inputs {
        if let syn::FnArg::Typed(pt) = input {
            collect_pat_idents(&pt.pat, &mut bound);
        }
    }
    // Generic type params + lifetimes + const params.
    for gp in item.sig.generics.params.iter() {
        match gp {
            syn::GenericParam::Type(t) => {
                bound.insert(t.ident.to_string());
            }
            syn::GenericParam::Const(c) => {
                bound.insert(c.ident.to_string());
            }
            _ => {}
        }
    }
    let mut v = UpperFreeCollector {
        bound,
        found: BTreeSet::new(),
    };
    v.visit_block(&item.block);
    v.found
}

fn collect_pat_idents(p: &syn::Pat, out: &mut BTreeSet<String>) {
    match p {
        syn::Pat::Ident(pi) => {
            out.insert(pi.ident.to_string());
        }
        syn::Pat::Tuple(t) => {
            for el in &t.elems {
                collect_pat_idents(el, out);
            }
        }
        syn::Pat::TupleStruct(ts) => {
            for el in &ts.elems {
                collect_pat_idents(el, out);
            }
        }
        syn::Pat::Struct(ps) => {
            for f in &ps.fields {
                collect_pat_idents(&f.pat, out);
            }
        }
        syn::Pat::Reference(r) => collect_pat_idents(&r.pat, out),
        syn::Pat::Type(t) => collect_pat_idents(&t.pat, out),
        syn::Pat::Or(o) => {
            for c in &o.cases {
                collect_pat_idents(c, out);
            }
        }
        _ => {}
    }
}

struct UpperFreeCollector {
    bound: BTreeSet<String>,
    found: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for UpperFreeCollector {
    fn visit_local(&mut self, l: &'ast syn::Local) {
        if let Some(init) = &l.init {
            self.visit_expr(&init.expr);
            if let Some(d) = &init.diverge {
                self.visit_expr(&d.1);
            }
        }
        collect_pat_idents(&l.pat, &mut self.bound);
    }
    fn visit_expr_path(&mut self, p: &'ast syn::ExprPath) {
        if p.qself.is_none() && p.path.segments.len() == 1 {
            let id = p.path.segments[0].ident.to_string();
            if starts_upper(&id) && !self.bound.contains(&id) {
                self.found.insert(id);
            }
        }
        // Multi-segment path: collect the FIRST segment (e.g. `KIND_LIST`
        // in `KIND_LIST::FOO` or a type prefix). Skip aliases starting
        // with `crate`/`self`/`super`/`Self`.
        if p.qself.is_none() && p.path.segments.len() > 1 {
            let head = &p.path.segments[0].ident;
            let s = head.to_string();
            if !matches!(s.as_str(), "crate" | "self" | "super" | "Self")
                && starts_upper(&s)
                && !self.bound.contains(&s)
            {
                self.found.insert(s);
            }
        }
        syn::visit::visit_expr_path(self, p);
    }
    fn visit_type_path(&mut self, t: &'ast syn::TypePath) {
        if t.qself.is_none() && !t.path.segments.is_empty() {
            let s = t.path.segments[0].ident.to_string();
            if !matches!(s.as_str(), "crate" | "self" | "super" | "Self")
                && starts_upper(&s)
                && !self.bound.contains(&s)
            {
                self.found.insert(s);
            }
        }
        syn::visit::visit_type_path(self, t);
    }
}

fn starts_upper(s: &str) -> bool {
    s.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false)
}

fn split_qualified(qualified: &str) -> (String, String) {
    let mut parts: Vec<&str> = qualified.split("::").collect();
    let leaf = parts.pop().unwrap_or(qualified).to_string();
    let crate_name = parts.first().copied().unwrap_or("").to_string();
    (crate_name, leaf)
}

fn find_fn_by_name_and_line<'a>(
    file: &'a syn::File,
    leaf: &str,
    line: usize,
) -> Option<&'a syn::ItemFn> {
    let mut found: Option<&'a syn::ItemFn> = None;
    walk_fns(&file.items, &mut |f: &'a syn::ItemFn| {
        if f.sig.ident == leaf {
            let l = f.sig.ident.span().start().line;
            // Prefer exact-line match; otherwise first by name.
            if l == line {
                found = Some(f);
            } else if found.is_none() {
                found = Some(f);
            }
        }
    });
    found
}

fn walk_fns<'a, F: FnMut(&'a syn::ItemFn)>(items: &'a [syn::Item], cb: &mut F) {
    for it in items {
        match it {
            syn::Item::Fn(f) => cb(f),
            syn::Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    walk_fns(items, cb);
                }
            }
            _ => {}
        }
    }
}

/// Byte span [start, end) covering the full fn item including its
/// (optional) doc comments and attributes immediately above. We keep
/// it simple: walk back over consecutive `#[...]` / `///` / `//!` lines
/// preceding the `fn` keyword. End span includes the closing brace.
fn fn_span_bytes(src: &str, item: &syn::ItemFn) -> (usize, usize, usize) {
    let start_line = item.sig.fn_token.span.start().line;
    let end_line = item.block.brace_token.span.close().end().line;
    let line_offsets = line_offsets(src);

    // Walk back to include attribute / doc-comment lines.
    let mut effective_start_line = start_line;
    while effective_start_line > 1 {
        let prev = effective_start_line - 1;
        let line_text = line_text(src, &line_offsets, prev).trim_start();
        if line_text.starts_with("#[")
            || line_text.starts_with("///")
            || line_text.starts_with("//!")
        {
            effective_start_line = prev;
        } else {
            break;
        }
    }

    let start_byte = line_offsets[effective_start_line - 1];
    let end_byte = if end_line >= line_offsets.len() {
        src.len()
    } else {
        line_offsets[end_line]
    };
    (start_byte, end_byte, effective_start_line)
}

fn line_offsets(src: &str) -> Vec<usize> {
    let mut offs = vec![0usize];
    for (i, b) in src.bytes().enumerate() {
        if b == b'\n' {
            offs.push(i + 1);
        }
    }
    offs
}

fn line_text<'a>(src: &'a str, line_offsets: &[usize], line: usize) -> &'a str {
    let start = line_offsets[line - 1];
    let end = line_offsets
        .get(line)
        .copied()
        .unwrap_or(src.len());
    &src[start..end.min(src.len())]
}

fn normalize_fn_body_string(item: &syn::ItemFn) -> String {
    // Body-only token-stream comparison: ignores whitespace / comments
    // and signature differences (fn name, where-clauses). Matches the
    // similarity definition used in dupes.rs (body tokens only).
    let mut ts = proc_macro2::TokenStream::new();
    item.block.to_tokens(&mut ts);
    ts.to_string()
}

fn assert_all_byte_identical(members: &[ResolvedMember]) -> Result<(), DedupeError> {
    let Some(first) = members.first() else {
        return Ok(());
    };
    for m in &members[1..] {
        if m.body_tokens != first.body_tokens {
            return Err(DedupeError::NotByteIdentical);
        }
    }
    Ok(())
}

/// Resolve each uppercase free name referenced inside a cluster member
/// against the workspace graph, scoped to that member's crate. If two
/// members reference the same name but it resolves to different
/// definition sites, refuse the dedupe (wb-5lgj.37).
fn assert_free_names_agree(
    members: &[ResolvedMember],
    graph: &ReferenceGraph,
) -> Result<(), DedupeError> {
    if members.len() < 2 {
        return Ok(());
    }
    let mut union: BTreeSet<&str> = BTreeSet::new();
    for m in members {
        for n in &m.upper_free_names {
            union.insert(n.as_str());
        }
    }
    for name in union {
        let mut sites: Vec<(usize, (PathBuf, usize))> = Vec::new();
        for (i, m) in members.iter().enumerate() {
            if !m.upper_free_names.contains(name) {
                continue;
            }
            let site = resolve_free_name_site(graph, &m.crate_name, &m.file, name);
            // Extern types like `String`, `Vec` won't be in the graph;
            // skip names we can't resolve.
            if let Some(s) = site {
                sites.push((i, s));
            }
        }
        if sites.len() < 2 {
            continue;
        }
        let (first_i, (first_file, first_line)) = &sites[0];
        for (i, (file, line)) in &sites[1..] {
            if file != first_file || line != first_line {
                return Err(DedupeError::ScopeDivergence {
                    name: name.to_string(),
                    member_a: members[*first_i].member.symbol.clone(),
                    file_a: members[*first_i].member.file.clone(),
                    def_a: format!("{}:{}", first_file.display(), first_line),
                    member_b: members[*i].member.symbol.clone(),
                    file_b: members[*i].member.file.clone(),
                    def_b: format!("{}:{}", file.display(), line),
                });
            }
        }
    }
    Ok(())
}

fn resolve_free_name_site(
    graph: &ReferenceGraph,
    from_crate: &str,
    from_file: &Path,
    name: &str,
) -> Option<(PathBuf, usize)> {
    let candidates = graph.by_name.get(name)?;
    // Strongest: same file (module-local const / static / type).
    for &idx in candidates {
        let s = &graph.symbols[idx];
        if s.file == from_file {
            return Some((s.file.clone(), s.line));
        }
    }
    // Next: same crate. With multiple same-crate hits we pick the first
    // deterministically — divergence comparison across members still
    // catches the case where two members of the cluster end up pointing
    // at different files even if both are nominally "same crate".
    let same_crate: Vec<usize> = candidates
        .iter()
        .copied()
        .filter(|&i| graph.symbols[i].symbol.crate_name == from_crate)
        .collect();
    if let Some(&i) = same_crate.first() {
        let s = &graph.symbols[i];
        return Some((s.file.clone(), s.line));
    }
    if candidates.len() == 1 {
        let s = &graph.symbols[candidates[0]];
        return Some((s.file.clone(), s.line));
    }
    None
}

/// Rewrite the canonical fn declaration to advertise `needed` visibility.
fn elevate_canonical_vis(
    canonical: &ResolvedMember,
    needed: Visibility,
) -> Result<FileEdit, DedupeError> {
    let src = std::fs::read_to_string(&canonical.file).map_err(|e| DedupeError::Io {
        path: canonical.file.clone(),
        source: e,
    })?;
    let out = rewrite_vis_at_line(&src, canonical.fn_keyword_line, needed);
    Ok(FileEdit {
        path: canonical.file.clone(),
        new_contents: out,
    })
}

fn re_elevate_in(src: &str, canonical: &ResolvedMember, needed: Visibility) -> String {
    rewrite_vis_at_line(src, canonical.fn_keyword_line, needed)
}

fn rewrite_vis_at_line(src: &str, fn_line: usize, needed: Visibility) -> String {
    let label = vis_label(needed);
    let lines: Vec<&str> = src.lines().collect();
    let mut out_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    if fn_line == 0 || fn_line > out_lines.len() {
        return src.to_string();
    }
    let idx = fn_line - 1;
    let line = &out_lines[idx];
    let indent_len = line.len() - line.trim_start().len();
    let (indent, body) = line.split_at(indent_len);
    let rest = body;
    let new_body = if let Some(after_pub) = rest.strip_prefix("pub") {
        // existing pub or pub(..)
        let after = after_pub;
        let after_vis = if after.starts_with('(') {
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
        match label {
            "pub" => format!("pub {}", after_vis),
            other => format!("{} {}", other, after_vis),
        }
    } else {
        // private — prepend the new visibility token
        format!("{} {}", label, rest)
    };
    out_lines[idx] = format!("{}{}", indent, new_body);
    let had_trailing_nl = src.ends_with('\n');
    let mut out = out_lines.join("\n");
    if had_trailing_nl {
        out.push('\n');
    }
    out
}

fn pick_canonical(
    members: &[ResolvedMember],
    spec: Option<&str>,
) -> Result<usize, DedupeError> {
    if let Some(spec) = spec {
        for (i, m) in members.iter().enumerate() {
            if matches_canonical_spec(m, spec) {
                return Ok(i);
            }
        }
        return Err(DedupeError::CanonicalNotFound {
            spec: spec.to_string(),
        });
    }
    // Default: lex-first by (file path string, line).
    let mut best = 0usize;
    for i in 1..members.len() {
        let a = &members[best];
        let b = &members[i];
        let ka = (a.file.display().to_string(), a.fn_line_start);
        let kb = (b.file.display().to_string(), b.fn_line_start);
        if kb < ka {
            best = i;
        }
    }
    Ok(best)
}

fn matches_canonical_spec(m: &ResolvedMember, spec: &str) -> bool {
    if m.member.symbol == spec {
        return true;
    }
    if m.crate_name == spec {
        return true;
    }
    if m.leaf_name == spec {
        return true;
    }
    if m.file.display().to_string() == spec {
        return true;
    }
    if let Some((file, line)) = spec.rsplit_once(':') {
        if let Ok(line_n) = line.parse::<usize>() {
            if m.file.display().to_string() == file && m.fn_line_start == line_n {
                return true;
            }
        }
    }
    false
}

fn rewrite_file(
    file: &Path,
    removed: &[&ResolvedMember],
    canonical: &ResolvedMember,
) -> Result<FileEdit, DedupeError> {
    let src = std::fs::read_to_string(file).map_err(|e| DedupeError::Io {
        path: file.to_path_buf(),
        source: e,
    })?;

    // 1) Delete the fn byte spans (descending order so byte offsets
    //    stay valid as we splice).
    let mut spans: Vec<(usize, usize)> =
        removed.iter().map(|r| (r.fn_byte_start, r.fn_byte_end)).collect();
    spans.sort_by(|a, b| b.0.cmp(&a.0));
    let mut out = src.clone();
    for (s, e) in spans {
        let mut s = s;
        // Eat trailing blank line after the deletion so we don't leave
        // a double blank.
        let mut e_extended = e;
        while e_extended < out.len() && (out.as_bytes()[e_extended] == b'\n')
        {
            e_extended += 1;
            break;
        }
        // Also eat the leading blank line before if present.
        while s > 0 && out.as_bytes()[s - 1] == b'\n'
            && s >= 2
            && out.as_bytes()[s - 2] == b'\n'
        {
            s -= 1;
            break;
        }
        out.replace_range(s..e_extended, "");
    }

    // 2) Rewrite call sites for renamed members (where leaf name
    //    differs from canonical leaf name).
    for r in removed {
        if r.leaf_name != canonical.leaf_name {
            out = rewrite_call_sites(&out, &r.leaf_name, &canonical.leaf_name);
        }
    }

    // 3) Insert `use <canonical_crate>::<canonical_leaf>;` if needed.
    //    We need an import when the removed member lived in a different
    //    crate from the canonical AND the canonical isn't already in
    //    the file's `use` list.
    let needs_import = removed
        .iter()
        .any(|r| r.crate_name != canonical.crate_name);
    if needs_import {
        let import = format!(
            "use {}::{};",
            canonical.crate_name, canonical.leaf_name
        );
        if !out.contains(&import) {
            out = insert_use(&out, &import);
        }
    }

    Ok(FileEdit {
        path: file.to_path_buf(),
        new_contents: out,
    })
}

fn rewrite_call_sites(src: &str, from: &str, to: &str) -> String {
    // Token-aware identifier replacement: split into runs of
    // identifier-chars and non-identifier-chars, replace whole-token
    // matches of `from`. Avoids matching substrings or paths like
    // `foo::from_x`.
    let mut out = String::with_capacity(src.len());
    let mut cur = String::new();
    let mut in_ident = false;
    for c in src.chars() {
        let is_id = c.is_ascii_alphanumeric() || c == '_';
        if is_id != in_ident {
            if in_ident {
                if cur == from {
                    out.push_str(to);
                } else {
                    out.push_str(&cur);
                }
                cur.clear();
            } else {
                out.push_str(&cur);
                cur.clear();
            }
            in_ident = is_id;
        }
        cur.push(c);
    }
    if in_ident {
        if cur == from {
            out.push_str(to);
        } else {
            out.push_str(&cur);
        }
    } else {
        out.push_str(&cur);
    }
    out
}

fn insert_use(src: &str, import_line: &str) -> String {
    // Insert after the last `use` statement if any; otherwise at the
    // top of the file (after a leading attribute / doc-comment block).
    let lines: Vec<&str> = src.lines().collect();
    let mut last_use_idx: Option<usize> = None;
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim_start();
        if t.starts_with("use ") {
            last_use_idx = Some(i);
        }
    }
    let insert_at = match last_use_idx {
        Some(i) => i + 1,
        None => {
            // Skip leading `//!` / `//` / `#![..]` / blank lines.
            let mut k = 0;
            while k < lines.len() {
                let t = lines[k].trim_start();
                if t.is_empty()
                    || t.starts_with("//")
                    || t.starts_with("#![")
                    || t.starts_with("#[")
                {
                    k += 1;
                } else {
                    break;
                }
            }
            k
        }
    };
    let mut out_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    out_lines.insert(insert_at, import_line.to_string());
    let had_trailing_nl = src.ends_with('\n');
    let mut out = out_lines.join("\n");
    if had_trailing_nl {
        out.push('\n');
    }
    out
}

/// Apply a slice of `FileEdit`s by writing each to disk.
pub fn apply_edits(edits: &[FileEdit]) -> Result<usize, DedupeError> {
    for e in edits {
        std::fs::write(&e.path, &e.new_contents).map_err(|err| DedupeError::Io {
            path: e.path.clone(),
            source: err,
        })?;
    }
    Ok(edits.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_call_sites_is_token_aware() {
        let src = "fn f() { pick_ext(\"x\"); other_pick_ext_thing(); }";
        let got = rewrite_call_sites(src, "pick_ext", "canonical_pick_ext");
        assert_eq!(
            got,
            "fn f() { canonical_pick_ext(\"x\"); other_pick_ext_thing(); }"
        );
    }

    #[test]
    fn split_qualified_handles_modules() {
        let (c, n) = split_qualified("crate_a::pick_ext");
        assert_eq!(c, "crate_a");
        assert_eq!(n, "pick_ext");
        let (c, n) = split_qualified("crate_a::sub::pick_ext");
        assert_eq!(c, "crate_a");
        assert_eq!(n, "pick_ext");
    }
}
