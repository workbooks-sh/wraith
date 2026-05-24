//! `extract-fn` refactor — lifts a contiguous line range out of an
//! existing fn into a new fn, inferring the signature from data-flow.
//!
//! v2 scope (wb-5lgj.27 + wb-5lgj.38) — on top of v1:
//!   - early `return val` → extracted fn returns `Option<T>`; caller does
//!     `if let Some(v) = f(...) { return v; }`
//!   - `?` operator → extracted fn returns `Result<_, _>`; caller does
//!     `let x = f(...)?;` (zero-return) or `let r = f(...)?;` (some-return)
//!   - `.await` → extracted fn is `async`; caller awaits. Refused if the
//!     enclosing fn isn't async.
//!   - `self.` access → extracted fn becomes an associated fn taking
//!     `self_ref: &Self` or `&mut Self`; caller passes `self`. Refused
//!     when the enclosing fn isn't a method.
//!   - match-arm-body extraction — when the selected range is exactly a
//!     match-arm body, extract it into a fn whose params include any
//!     bindings introduced by the arm pattern.
//!   - `break` / `continue` extraction is REFUSED (control-flow shape
//!     needs caller-side dispatch enum; deferred to v3). Nested
//!     break/continue targeting an inner loop in the range is fine
//!     and not refused.

use std::collections::BTreeSet;
use std::path::PathBuf;

use proc_macro2::LineColumn;
use syn::spanned::Spanned;
use syn::visit::Visit;

#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
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
    #[error("no enclosing function found for lines {start}..{end} in {path}")]
    NoEnclosingFn {
        path: PathBuf,
        start: usize,
        end: usize,
    },
    #[error(
        "range does not align with a contiguous run of statements in the enclosing function"
    )]
    RangeMisaligned,
    #[error(
        "range contains {pattern} that extract-fn v2 cannot safely lift — refusing to produce broken code"
    )]
    UnsupportedPattern { pattern: &'static str },
    #[error("range is empty")]
    EmptyRange,
}

#[derive(Debug, Clone)]
pub struct ExtractOptions {
    pub file: PathBuf,
    pub line_start: usize,
    pub line_end: usize,
    pub new_fn_name: String,
}

#[derive(Debug, Clone)]
pub struct ExtractResult {
    pub new_source: String,
    pub call_args: Vec<CallArg>,
    pub returns: Vec<String>,
    pub fn_signature: String,
}

#[derive(Debug, Clone)]
pub struct CallArg {
    pub name: String,
    pub mutable: bool,
}

/// What extract-fn would do with a given (file, range). Used by
/// `health --suggest-extractions` to pre-filter candidates against the
/// v2 acceptor without producing an artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Feasibility {
    /// Straightforward extract — no control-flow rewrites needed.
    Ok,
    /// v2 will succeed but must rewrite control-flow (early-return,
    /// `?`, `.await`, or self.) into a different shape at the callsite.
    V2OnlyWithRewrite,
    /// Range cannot be extracted; reason is human-readable.
    Refused(String),
}

pub fn extract_fn(opts: &ExtractOptions) -> Result<ExtractResult, ExtractError> {
    let src = std::fs::read_to_string(&opts.file).map_err(|e| ExtractError::Io {
        path: opts.file.clone(),
        source: e,
    })?;
    let ast = syn::parse_file(&src).map_err(|e| ExtractError::Parse {
        path: opts.file.clone(),
        source: e,
    })?;

    if let Some(arm) = find_enclosing_match_arm(&ast, opts.line_start, opts.line_end) {
        return extract_match_arm(&src, opts, arm, &ast);
    }

    let enclosing = find_enclosing_fn(&ast, opts.line_start, opts.line_end).ok_or_else(|| {
        ExtractError::NoEnclosingFn {
            path: opts.file.clone(),
            start: opts.line_start,
            end: opts.line_end,
        }
    })?;

    let outer_is_async = enclosing.sig.asyncness.is_some();
    let outer_is_method = first_is_self_receiver(&enclosing.sig);

    // Find the INNERMOST block whose stmts can host the range. For
    // deeply-nested code (run_image-style monoliths) this lets the user
    // extract a sub-region inside a match arm or if branch.
    let innermost_stmts = innermost_block_stmts(enclosing.block, opts.line_start, opts.line_end);
    let (range_stmts, before_stmts, after_stmts) =
        partition_stmts(innermost_stmts, opts.line_start, opts.line_end)?;
    if range_stmts.is_empty() {
        return Err(ExtractError::EmptyRange);
    }

    // Classify control-flow patterns in the range.
    let flow = scan_range_flow(&range_stmts);

    // Hard refusals.
    if flow.has_loop_escape {
        return Err(ExtractError::UnsupportedPattern {
            pattern: "break/continue targeting an outer loop",
        });
    }
    if flow.has_await && !outer_is_async {
        return Err(ExtractError::UnsupportedPattern {
            pattern: ".await in a non-async enclosing fn",
        });
    }
    if (flow.has_self_field || flow.has_self_method) && !outer_is_method {
        return Err(ExtractError::UnsupportedPattern {
            pattern: "self. access in a non-method enclosing fn",
        });
    }
    if flow.has_return && flow.has_try {
        // Mixing both shapes means we'd need both Option<T> and ? sugar in
        // the same signature — punt to v3 rather than guess.
        return Err(ExtractError::UnsupportedPattern {
            pattern: "both early-return and ? in the same range",
        });
    }

    // Data-flow analysis (reads / writes / bindings).
    let bindings_before = collect_bindings(&before_stmts);
    let bindings_in_range = collect_bindings_in_stmts(&range_stmts);
    let mut used_after = IdentCollector::default();
    for s in &after_stmts {
        used_after.visit_stmt(s);
    }
    let mut analyzer = RangeAnalyzer::default();
    for s in &range_stmts {
        analyzer.visit_stmt(s);
    }

    let mut params: Vec<CallArg> = Vec::new();
    for name in &bindings_before {
        let is_read = analyzer.reads.contains(name);
        let is_mut = analyzer.writes.contains(name);
        if is_read || is_mut {
            params.push(CallArg {
                name: name.clone(),
                mutable: is_mut,
            });
        }
    }
    params.sort_by(|a, b| a.name.cmp(&b.name));

    let mut returns: Vec<String> = bindings_in_range
        .iter()
        .filter(|n| used_after.idents.contains(*n))
        .cloned()
        .collect();
    returns.sort();

    // Decide on body rewrite + return shape.
    let body_text = stmts_text_range(&src, &range_stmts);
    let rewritten_body = rewrite_body_for_flow(&body_text, &flow, &returns);

    // Compute fn signature.
    let mut sig_params: Vec<String> = Vec::new();
    if flow.has_self_field || flow.has_self_method {
        if flow.has_self_mut {
            sig_params.push("self_ref: &mut Self".to_string());
        } else {
            sig_params.push("self_ref: &Self".to_string());
        }
    }
    for p in &params {
        if p.mutable {
            sig_params.push(format!("{}: &mut i32", p.name));
        } else {
            // Take by value (i32 is Copy) so the extracted body's uses
            // — comparisons, indexing, etc. — don't need callers to
            // hand-deref. Mutated params still need &mut.
            sig_params.push(format!("{}: i32", p.name));
        }
    }

    let plain_ret = match returns.len() {
        0 => String::new(),
        1 => " -> i32".to_string(),
        n => format!(" -> ({})", vec!["i32"; n].join(", ")),
    };

    let ret_type = if flow.has_return {
        // Wraps the plain shape in Option<…>. The early-return path
        // returns Some(val); the fall-through returns None.
        match returns.len() {
            0 => " -> Option<i32>".to_string(),
            1 => " -> Option<i32>".to_string(),
            n => format!(" -> Option<({})>", vec!["i32"; n].join(", ")),
        }
    } else if flow.has_try {
        // ? lifts the natural shape into Result<_, _>. We don't know the
        // error type, so use a boxed dyn Error as a permissive default.
        match returns.len() {
            0 => " -> Result<(), Box<dyn std::error::Error>>".to_string(),
            1 => " -> Result<i32, Box<dyn std::error::Error>>".to_string(),
            n => format!(
                " -> Result<({}), Box<dyn std::error::Error>>",
                vec!["i32"; n].join(", ")
            ),
        }
    } else {
        plain_ret.clone()
    };

    let async_kw = if flow.has_await { "async " } else { "" };

    // wb-5lgj.41: hoist any LOCAL `use` statements (declared inside the
    // enclosing fn body) that the extracted code depends on. Without
    // this, code that referenced `compose::composite_over` via an
    // in-fn `use wavelet::{compose, ...};` fails to resolve at the
    // top-level extraction destination.
    let hoisted_uses = collect_hoisted_local_uses(&enclosing.block, &rewritten_body.body);

    let new_fn = format!(
        "{async_}fn {name}({sig}){ret} {{\n{uses}{body}{tail}}}\n",
        async_ = async_kw,
        name = opts.new_fn_name,
        sig = sig_params.join(", "),
        ret = ret_type,
        uses = hoisted_uses,
        body = indent_body(&rewritten_body.body),
        tail = rewritten_body.tail,
    );

    // Build the call site.
    let mut call_args: Vec<String> = Vec::new();
    if flow.has_self_field || flow.has_self_method {
        if flow.has_self_mut {
            call_args.push("&mut self".to_string());
        } else {
            call_args.push("&self".to_string());
        }
    }
    for p in &params {
        if p.mutable {
            call_args.push(format!("&mut {}", p.name));
        } else {
            call_args.push(p.name.clone());
        }
    }
    let args_joined = call_args.join(", ");
    let call_prefix = if flow.has_self_field || flow.has_self_method {
        format!("Self::{}", opts.new_fn_name)
    } else {
        opts.new_fn_name.clone()
    };
    let await_suffix = if flow.has_await { ".await" } else { "" };

    let call_indent = leading_indent(&src, range_stmts[0].span().start());

    let call_line = build_call_line(
        &call_indent,
        &call_prefix,
        &args_joined,
        await_suffix,
        &flow,
        &returns,
    );

    let new_source = splice_source(
        &src,
        opts.line_start,
        opts.line_end,
        &call_line,
        enclosing.block.brace_token.span.close().end(),
        &new_fn,
    );

    let fn_signature = format!(
        "{}fn {}({}){}",
        async_kw,
        opts.new_fn_name,
        sig_params.join(", "),
        ret_type
    );

    Ok(ExtractResult {
        new_source,
        call_args: params,
        returns,
        fn_signature,
    })
}

/// Same plumbing as `extract_fn`, but for the case where the range
/// matches a single match-arm body. Bindings introduced by the arm
/// pattern become params alongside the usual outer-scope reads.
fn extract_match_arm(
    src: &str,
    opts: &ExtractOptions,
    arm_info: MatchArmInfo<'_>,
    ast: &syn::File,
) -> Result<ExtractResult, ExtractError> {
    // Body capture — column-precise, NOT line-only. For inline arms the
    // body span starts AFTER the `=>` separator; previous line-only
    // slicing was including `} => ` from the pattern closing brace.
    let body_text = match arm_info.body_expr {
        syn::Expr::Block(blk) => {
            // Slice just the inner stmts — drop the wrapping braces.
            if let (Some(first), Some(last)) =
                (blk.block.stmts.first(), blk.block.stmts.last())
            {
                let s = first.span().start();
                let e = last.span().end();
                slice_span_text(src, s, e)
            } else {
                String::new()
            }
        }
        _ => {
            let s = arm_info.body_expr.span().start();
            let e = arm_info.body_expr.span().end();
            slice_span_text(src, s, e)
        }
    };

    let mut scan = ArmFlowScan::default();
    syn::visit::visit_expr(&mut scan, arm_info.body_expr);
    if scan.has_loop_escape {
        return Err(ExtractError::UnsupportedPattern {
            pattern: "break/continue inside the match-arm body",
        });
    }

    // Source-order pattern bindings (no sort/dedup — destructuring
    // declares fields in a specific order, and callsite args must match).
    let pat_bindings: Vec<String> = pat_idents_in_source_order(arm_info.pat);

    // Param type inference. For Pat::Struct patterns like
    // `EnumOrStruct::Variant { a, b, c }`, walk the enum/struct decl in
    // the local file and pull each field's declared type text.
    let pat_types = infer_pat_binding_types(arm_info.pat, ast);

    let mut sig_param_parts: Vec<String> = Vec::with_capacity(pat_bindings.len());
    let mut missing_types: Vec<String> = Vec::new();
    for name in &pat_bindings {
        match pat_types.get(name) {
            Some(ty) => sig_param_parts.push(format!("{}: {}", name, ty)),
            None => {
                missing_types.push(name.clone());
                sig_param_parts.push(format!("{}: /* TYPE: unknown */ _", name));
            }
        }
    }
    if !missing_types.is_empty() {
        // Refuse rather than silently lie with `i32`. The bug spec
        // (wb-5lgj.40) is explicit: never default to `i32` for
        // match-arm-body extraction — that produces invalid Rust.
        return Err(ExtractError::UnsupportedPattern {
            pattern: "match-arm-body params whose types can't be resolved from the local file \
(extraction would produce invalid Rust; resolve the discriminant's enum/struct decl into the same file or add it to the wraith resolver)",
        });
    }
    let sig_params = sig_param_parts.join(", ");
    let call_args = pat_bindings.join(", ");

    // Return-type inference from tail expressions.
    let ret_type_text = infer_return_type_text(arm_info.body_expr);
    let ret_clause = match &ret_type_text {
        Some(t) => format!(" -> {}", t),
        None => String::new(),
    };

    // wb-5lgj.41: hoist local `use` statements from the enclosing fn
    // body. Same rationale as the block-extract path.
    let hoisted_uses = match find_enclosing_fn(ast, opts.line_start, opts.line_end) {
        Some(enclosing) => collect_hoisted_local_uses(&enclosing.block, &body_text),
        None => String::new(),
    };

    let new_fn = format!(
        "fn {name}({sig}){ret} {{\n{uses}{body}}}\n",
        name = opts.new_fn_name,
        sig = sig_params,
        ret = ret_clause,
        uses = hoisted_uses,
        body = indent_body(&body_text),
    );

    // Reconstruct the arm header line in place: `<indent>PATTERN => handle(args),`.
    let pat_span = arm_info.pat.span();
    let pat_text = slice_span_text(src, pat_span.start(), pat_span.end());
    let call_line = format!(
        "{indent}{pat} => {name}({args}),",
        indent = arm_info.arm_indent,
        pat = pat_text,
        name = opts.new_fn_name,
        args = call_args,
    );
    let _ = arm_info.has_trailing_comma;

    // Replace lines from the arm's pattern start through the arm body
    // end — i.e. the whole arm declaration — with the new call_line.
    let arm_first_line = arm_info.pat.span().start().line;
    let new_source = splice_source(
        src,
        arm_first_line,
        arm_info.body_end_line,
        &call_line,
        arm_info.enclosing_fn_close,
        &new_fn,
    );

    let fn_signature = format!(
        "fn {}({}){}",
        opts.new_fn_name, sig_params, ret_clause
    );
    Ok(ExtractResult {
        new_source,
        call_args: pat_bindings
            .iter()
            .map(|n| CallArg {
                name: n.clone(),
                mutable: false,
            })
            .collect(),
        returns: Vec::new(),
        fn_signature,
    })
}

/// Walks a pattern in source declaration order and returns the bound
/// identifiers in that order. Unlike `pat_idents`, does NOT sort —
/// callers depending on declaration order (callsite arg order, struct
/// field-type lookup) need the original sequence.
fn pat_idents_in_source_order(p: &syn::Pat) -> Vec<String> {
    let mut out = Vec::new();
    pat_idents(p, &mut out);
    // pat_idents walks struct fields in `fields` declaration order
    // already — fields come out source-ordered for Pat::Struct.
    out
}

/// For a pattern shaped like `Path::Variant { a, b, c }` or
/// `Struct { a, b }`, resolve each binding's declared type via the
/// enum-variant decl or struct decl living in the local file. Returns
/// a map from binding-name → type-text (as written in source).
fn infer_pat_binding_types(
    pat: &syn::Pat,
    ast: &syn::File,
) -> std::collections::HashMap<String, String> {
    use std::collections::HashMap;
    let mut out: HashMap<String, String> = HashMap::new();

    match pat {
        syn::Pat::Struct(ps) => {
            let segs: Vec<&syn::PathSegment> = ps.path.segments.iter().collect();
            if segs.is_empty() {
                return out;
            }
            let last = segs.last().unwrap().ident.to_string();
            let parent_name = if segs.len() >= 2 {
                Some(segs[segs.len() - 2].ident.to_string())
            } else {
                None
            };
            if let Some(enum_name) = &parent_name {
                if let Some(fields) = find_enum_variant_fields(ast, enum_name, &last) {
                    collect_field_types_for_pat(ps, &fields, &mut out);
                    return out;
                }
            }
            if let Some(fields) = find_struct_fields(ast, &last) {
                collect_field_types_for_pat(ps, &fields, &mut out);
            }
        }
        syn::Pat::TupleStruct(ts) => {
            let segs: Vec<&syn::PathSegment> = ts.path.segments.iter().collect();
            if segs.is_empty() {
                return out;
            }
            let last = segs.last().unwrap().ident.to_string();
            let parent_name = if segs.len() >= 2 {
                Some(segs[segs.len() - 2].ident.to_string())
            } else {
                None
            };
            if let Some(enum_name) = &parent_name {
                if let Some(types) = find_enum_variant_tuple_types(ast, enum_name, &last) {
                    collect_tuple_types_for_pat(ts, &types, &mut out);
                    return out;
                }
            }
            if let Some(types) = find_struct_tuple_types(ast, &last) {
                collect_tuple_types_for_pat(ts, &types, &mut out);
            }
        }
        _ => {}
    }
    out
}

fn collect_tuple_types_for_pat(
    ts: &syn::PatTupleStruct,
    types: &[String],
    out: &mut std::collections::HashMap<String, String>,
) {
    for (i, el) in ts.elems.iter().enumerate() {
        let ty = match types.get(i) {
            Some(t) => t.clone(),
            None => continue,
        };
        let mut idents = Vec::new();
        pat_idents(el, &mut idents);
        if let Some(name) = idents.first() {
            out.insert(name.clone(), ty);
        }
    }
}

fn find_enum_variant_tuple_types(
    ast: &syn::File,
    enum_name: &str,
    variant_name: &str,
) -> Option<Vec<String>> {
    fn search(items: &[syn::Item], en: &str, vn: &str) -> Option<Vec<String>> {
        for it in items {
            match it {
                syn::Item::Enum(e) if e.ident == en => {
                    for v in &e.variants {
                        if v.ident == vn {
                            if let syn::Fields::Unnamed(u) = &v.fields {
                                return Some(
                                    u.unnamed.iter().map(|f| type_to_text(&f.ty)).collect(),
                                );
                            }
                        }
                    }
                }
                syn::Item::Mod(m) => {
                    if let Some((_, items)) = &m.content {
                        if let Some(r) = search(items, en, vn) {
                            return Some(r);
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }
    search(&ast.items, enum_name, variant_name)
}

fn find_struct_tuple_types(ast: &syn::File, name: &str) -> Option<Vec<String>> {
    fn search(items: &[syn::Item], n: &str) -> Option<Vec<String>> {
        for it in items {
            match it {
                syn::Item::Struct(s) if s.ident == n => {
                    if let syn::Fields::Unnamed(u) = &s.fields {
                        return Some(u.unnamed.iter().map(|f| type_to_text(&f.ty)).collect());
                    }
                }
                syn::Item::Mod(m) => {
                    if let Some((_, items)) = &m.content {
                        if let Some(r) = search(items, n) {
                            return Some(r);
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }
    search(&ast.items, name)
}

fn collect_field_types_for_pat(
    ps: &syn::PatStruct,
    fields: &[(String, String)],
    out: &mut std::collections::HashMap<String, String>,
) {
    let field_map: std::collections::HashMap<&str, &str> =
        fields.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect();
    for f in &ps.fields {
        let field_name = match &f.member {
            syn::Member::Named(id) => id.to_string(),
            syn::Member::Unnamed(_) => continue,
        };
        let binding = match &*f.pat {
            syn::Pat::Ident(pi) => pi.ident.to_string(),
            // `field: bound_name` shorthand → bound_name from the inner pat
            _ => {
                let mut idents = Vec::new();
                pat_idents(&f.pat, &mut idents);
                if let Some(only) = idents.first() {
                    only.clone()
                } else {
                    continue;
                }
            }
        };
        if let Some(ty) = field_map.get(field_name.as_str()) {
            out.insert(binding, (*ty).to_string());
        }
    }
}

/// Returns Vec<(field_name, type_text)> in declaration order for the
/// named enum's named variant, if the enum lives in this file.
fn find_enum_variant_fields(
    ast: &syn::File,
    enum_name: &str,
    variant_name: &str,
) -> Option<Vec<(String, String)>> {
    fn search_items(
        items: &[syn::Item],
        enum_name: &str,
        variant_name: &str,
    ) -> Option<Vec<(String, String)>> {
        for it in items {
            match it {
                syn::Item::Enum(e) if e.ident == enum_name => {
                    for v in &e.variants {
                        if v.ident == variant_name {
                            return Some(extract_named_fields(&v.fields));
                        }
                    }
                }
                syn::Item::Mod(m) => {
                    if let Some((_, items)) = &m.content {
                        if let Some(r) = search_items(items, enum_name, variant_name) {
                            return Some(r);
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }
    search_items(&ast.items, enum_name, variant_name)
}

fn find_struct_fields(
    ast: &syn::File,
    struct_name: &str,
) -> Option<Vec<(String, String)>> {
    fn search_items(items: &[syn::Item], name: &str) -> Option<Vec<(String, String)>> {
        for it in items {
            match it {
                syn::Item::Struct(s) if s.ident == name => {
                    return Some(extract_named_fields(&s.fields));
                }
                syn::Item::Mod(m) => {
                    if let Some((_, items)) = &m.content {
                        if let Some(r) = search_items(items, name) {
                            return Some(r);
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }
    search_items(&ast.items, struct_name)
}

fn extract_named_fields(fields: &syn::Fields) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let syn::Fields::Named(named) = fields {
        for f in &named.named {
            if let Some(id) = &f.ident {
                out.push((id.to_string(), type_to_text(&f.ty)));
            }
        }
    }
    out
}

fn type_to_text(ty: &syn::Type) -> String {
    use quote::ToTokens;
    let mut ts = proc_macro2::TokenStream::new();
    ty.to_tokens(&mut ts);
    // Normalize whitespace — quote round-trip puts a space between
    // tokens (e.g. `Option < i32 >`). Tighten the common cases.
    let raw = ts.to_string();
    tighten_type_text(&raw)
}

fn tighten_type_text(s: &str) -> String {
    // Remove spaces around angle brackets, double colons, commas in
    // generics, and the unary `&`. We don't need a perfect formatter —
    // just something rustfmt will accept.
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i] as char;
        let nxt = bytes.get(i + 1).map(|b| *b as char);
        let prv = if out.is_empty() {
            None
        } else {
            out.chars().last()
        };
        if c == ' ' {
            // Skip if surrounded by structural punctuation that
            // doesn't need spacing.
            if matches!(prv, Some('<') | Some('>') | Some('&') | Some(':') | Some(',') | Some('('))
                || matches!(nxt, Some('<') | Some('>') | Some(',') | Some(':') | Some(')'))
            {
                i += 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Walk all tail-position expressions in `body` and, if every tail
/// yields the same inferable type, return that type's text. Recognized
/// patterns: `Ty::Variant`, `Ty::Variant(...)`, `Ty::method(...)`.
/// `return X;` tails count too (their argument's type is checked).
fn infer_return_type_text(body: &syn::Expr) -> Option<String> {
    let mut tails: Vec<&syn::Expr> = Vec::new();
    collect_tail_exprs(body, &mut tails);
    if tails.is_empty() {
        return None;
    }
    let mut inferred: Option<String> = None;
    for t in tails {
        let ty = match tail_expr_type(t) {
            Some(s) => s,
            None => return None,
        };
        match &inferred {
            None => inferred = Some(ty),
            Some(prev) if prev == &ty => {}
            _ => return None,
        }
    }
    inferred
}

fn collect_tail_exprs<'a>(expr: &'a syn::Expr, out: &mut Vec<&'a syn::Expr>) {
    match expr {
        syn::Expr::Block(b) => {
            if let Some(last) = b.block.stmts.last() {
                tail_from_stmt(last, out);
            }
        }
        syn::Expr::If(i) => {
            collect_tail_exprs_block(&i.then_branch, out);
            if let Some((_, else_expr)) = &i.else_branch {
                collect_tail_exprs(else_expr, out);
            }
        }
        syn::Expr::Match(m) => {
            for arm in &m.arms {
                collect_tail_exprs(&arm.body, out);
            }
        }
        syn::Expr::Return(r) => {
            if let Some(v) = &r.expr {
                out.push(v);
            } else {
                // bare `return;` — unit tail, breaks inference
                out.push(expr);
            }
        }
        _ => out.push(expr),
    }
}

fn collect_tail_exprs_block<'a>(b: &'a syn::Block, out: &mut Vec<&'a syn::Expr>) {
    if let Some(last) = b.stmts.last() {
        tail_from_stmt(last, out);
    }
}

fn tail_from_stmt<'a>(s: &'a syn::Stmt, out: &mut Vec<&'a syn::Expr>) {
    match s {
        syn::Stmt::Expr(e, None) => collect_tail_exprs(e, out),
        // `expr;` is a statement (semicolon present) — its value is ()
        // — push the original expr; tail_expr_type returns None for it.
        syn::Stmt::Expr(e, Some(_)) => out.push(e),
        _ => {
            // local / item — no tail value
        }
    }
}

fn tail_expr_type(expr: &syn::Expr) -> Option<String> {
    match expr {
        // `Ty::Variant`
        syn::Expr::Path(p) if p.qself.is_none() && p.path.segments.len() >= 2 => {
            let n = p.path.segments.len();
            Some(p.path.segments[n - 2].ident.to_string())
        }
        // `Ty::method(args)` or `Ty::Variant(args)`
        syn::Expr::Call(c) => match &*c.func {
            syn::Expr::Path(p) if p.qself.is_none() && p.path.segments.len() >= 2 => {
                let n = p.path.segments.len();
                Some(p.path.segments[n - 2].ident.to_string())
            }
            _ => None,
        },
        _ => None,
    }
}

fn slice_span_text(src: &str, start: LineColumn, end: LineColumn) -> String {
    let lines: Vec<&str> = src.lines().collect();
    if start.line == end.line {
        let line = lines.get(start.line - 1).copied().unwrap_or("");
        let s = line.chars().skip(start.column).take(end.column - start.column).collect::<String>();
        return s;
    }
    let mut out = String::new();
    for ln in start.line..=end.line {
        let line = lines.get(ln - 1).copied().unwrap_or("");
        if ln == start.line {
            out.push_str(&line.chars().skip(start.column).collect::<String>());
            out.push('\n');
        } else if ln == end.line {
            out.push_str(&line.chars().take(end.column).collect::<String>());
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

struct RewrittenBody {
    body: String,
    /// Trailing literal (e.g. `    None\n`) appended after the body.
    tail: String,
}

fn rewrite_body_for_flow(body_text: &str, flow: &RangeFlow, returns: &[String]) -> RewrittenBody {
    // Strategy:
    //   - early-return: rewrite `return X;` → `return Some(X);` (or
    //     `return Some(());` if return has no value), append `    None`
    //     fall-through, and (if returns are also present) lift them into
    //     a Some(...) tail too.
    //   - ?: pass through; append `Ok(<returns>)` tail.
    //   - else: pass through; the usual tuple/ident tail.
    let body = if flow.has_return {
        rewrite_return_lines(body_text)
    } else {
        body_text.to_string()
    };

    let tail = if flow.has_return {
        let tail_expr = match returns.len() {
            0 => "None".to_string(),
            _ => "None".to_string(),
        };
        format!("    {tail}\n", tail = tail_expr)
    } else if flow.has_try {
        match returns.len() {
            0 => "    Ok(())\n".to_string(),
            1 => format!("    Ok({})\n", returns[0]),
            _ => format!("    Ok(({}))\n", returns.join(", ")),
        }
    } else {
        match returns.len() {
            0 => String::new(),
            1 => format!("    {}\n", returns[0]),
            _ => format!("    ({})\n", returns.join(", ")),
        }
    };
    RewrittenBody { body, tail }
}

/// Naive text rewrite: every `return X;` becomes `return Some(X);`.
/// Bare `return;` becomes `return Some(());`. We only touch lines whose
/// trimmed prefix is `return ` or exactly `return;`.
fn rewrite_return_lines(body: &str) -> String {
    let mut out = String::new();
    for raw_line in body.lines() {
        let trimmed = raw_line.trim_start();
        let indent_len = raw_line.len() - trimmed.len();
        if trimmed == "return;" {
            out.push_str(&raw_line[..indent_len]);
            out.push_str("return Some(());\n");
        } else if let Some(rest) = trimmed.strip_prefix("return ") {
            let rest = rest.trim_end();
            let rest = rest.strip_suffix(';').unwrap_or(rest);
            out.push_str(&raw_line[..indent_len]);
            out.push_str(&format!("return Some({});\n", rest));
        } else {
            out.push_str(raw_line);
            out.push('\n');
        }
    }
    out
}

fn build_call_line(
    indent: &str,
    call_prefix: &str,
    args: &str,
    await_suffix: &str,
    flow: &RangeFlow,
    returns: &[String],
) -> String {
    // ? case: callsite uses ? suffix
    if flow.has_try {
        let lhs = match returns.len() {
            0 => String::new(),
            1 => format!("let {} = ", returns[0]),
            _ => format!("let ({}) = ", returns.join(", ")),
        };
        return format!(
            "{indent}{lhs}{prefix}({args}){aw}?;",
            indent = indent,
            lhs = lhs,
            prefix = call_prefix,
            args = args,
            aw = await_suffix,
        );
    }
    // early-return case: callsite dispatches Option
    if flow.has_return {
        let bind = match returns.len() {
            0 => "_v".to_string(),
            1 => returns[0].clone(),
            _ => format!("({})", returns.join(", ")),
        };
        return format!(
            "{indent}if let Some({bind}) = {prefix}({args}){aw} {{ return {bind}; }}",
            indent = indent,
            bind = bind,
            prefix = call_prefix,
            args = args,
            aw = await_suffix,
        );
    }
    // normal case: optional let-binding
    let lhs = match returns.len() {
        0 => String::new(),
        1 => format!("let {} = ", returns[0]),
        _ => format!("let ({}) = ", returns.join(", ")),
    };
    format!(
        "{indent}{lhs}{prefix}({args}){aw};",
        indent = indent,
        lhs = lhs,
        prefix = call_prefix,
        args = args,
        aw = await_suffix,
    )
}

#[derive(Default, Debug)]
struct RangeFlow {
    has_return: bool,
    has_try: bool,
    has_await: bool,
    has_self_field: bool,
    has_self_method: bool,
    has_self_mut: bool,
    has_loop_escape: bool,
}

fn scan_range_flow(stmts: &[&syn::Stmt]) -> RangeFlow {
    // Depth-tracked visit so break/continue inside an inner loop in the
    // range don't count as an outer loop escape.
    struct V {
        loop_depth: u32,
        flow: RangeFlow,
    }
    impl<'ast> Visit<'ast> for V {
        fn visit_expr_return(&mut self, _: &'ast syn::ExprReturn) {
            self.flow.has_return = true;
        }
        fn visit_expr_try(&mut self, e: &'ast syn::ExprTry) {
            self.flow.has_try = true;
            syn::visit::visit_expr_try(self, e);
        }
        fn visit_expr_await(&mut self, e: &'ast syn::ExprAwait) {
            self.flow.has_await = true;
            syn::visit::visit_expr_await(self, e);
        }
        fn visit_expr_break(&mut self, e: &'ast syn::ExprBreak) {
            if self.loop_depth == 0 {
                self.flow.has_loop_escape = true;
            }
            syn::visit::visit_expr_break(self, e);
        }
        fn visit_expr_continue(&mut self, _: &'ast syn::ExprContinue) {
            if self.loop_depth == 0 {
                self.flow.has_loop_escape = true;
            }
        }
        fn visit_expr_for_loop(&mut self, e: &'ast syn::ExprForLoop) {
            self.loop_depth += 1;
            syn::visit::visit_expr_for_loop(self, e);
            self.loop_depth -= 1;
        }
        fn visit_expr_while(&mut self, e: &'ast syn::ExprWhile) {
            self.loop_depth += 1;
            syn::visit::visit_expr_while(self, e);
            self.loop_depth -= 1;
        }
        fn visit_expr_loop(&mut self, e: &'ast syn::ExprLoop) {
            self.loop_depth += 1;
            syn::visit::visit_expr_loop(self, e);
            self.loop_depth -= 1;
        }
        fn visit_expr_field(&mut self, f: &'ast syn::ExprField) {
            if let syn::Expr::Path(p) = &*f.base {
                if p.path.is_ident("self") {
                    self.flow.has_self_field = true;
                }
            }
            syn::visit::visit_expr_field(self, f);
        }
        fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
            if let syn::Expr::Path(p) = &*m.receiver {
                if p.path.is_ident("self") {
                    self.flow.has_self_method = true;
                }
            }
            syn::visit::visit_expr_method_call(self, m);
        }
        fn visit_expr_assign(&mut self, a: &'ast syn::ExprAssign) {
            // self.x = … → needs &mut self
            if let syn::Expr::Field(f) = &*a.left {
                if let syn::Expr::Path(p) = &*f.base {
                    if p.path.is_ident("self") {
                        self.flow.has_self_mut = true;
                    }
                }
            }
            syn::visit::visit_expr_assign(self, a);
        }
        fn visit_expr_reference(&mut self, r: &'ast syn::ExprReference) {
            if r.mutability.is_some() {
                if let syn::Expr::Field(f) = &*r.expr {
                    if let syn::Expr::Path(p) = &*f.base {
                        if p.path.is_ident("self") {
                            self.flow.has_self_mut = true;
                        }
                    }
                }
            }
            syn::visit::visit_expr_reference(self, r);
        }
    }
    let mut v = V {
        loop_depth: 0,
        flow: RangeFlow::default(),
    };
    for s in stmts {
        v.visit_stmt(s);
    }
    v.flow
}

fn first_is_self_receiver(sig: &syn::Signature) -> bool {
    matches!(sig.inputs.first(), Some(syn::FnArg::Receiver(_)))
}

// ---------------------------------------------------------------------------
// match-arm extraction
// ---------------------------------------------------------------------------

#[allow(dead_code)]
struct MatchArmInfo<'a> {
    pat: &'a syn::Pat,
    body_expr: &'a syn::Expr,
    /// First line of the arm body (the `{` line, or the inline expr's line).
    body_start_line: usize,
    /// Last line of the arm body (the `}` line, or the inline expr's line).
    body_end_line: usize,
    /// Inner statements' lines, if the body is a Block.
    inner_start_line: usize,
    inner_end_line: usize,
    /// Indent of the arm header line — used to position the call.
    arm_indent: String,
    has_trailing_comma: bool,
    enclosing_fn_close: LineColumn,
}

fn find_enclosing_match_arm<'a>(
    file: &'a syn::File,
    line_start: usize,
    line_end: usize,
) -> Option<MatchArmInfo<'a>> {
    // Find any fn whose body contains a match expr whose arm-body lines
    // match the requested range exactly.
    let mut best: Option<MatchArmInfo<'a>> = None;
    visit_fns_with_close(&file.items, &mut |block, close| {
        let mut finder = ArmFinder {
            line_start,
            line_end,
            close,
            best: None,
        };
        finder.visit_block(block);
        if finder.best.is_some() {
            best = finder.best.take();
        }
    });
    best
}

fn visit_fns_with_close<'a, F: FnMut(&'a syn::Block, LineColumn)>(
    items: &'a [syn::Item],
    cb: &mut F,
) {
    for it in items {
        match it {
            syn::Item::Fn(f) => cb(&f.block, f.block.brace_token.span.close().end()),
            syn::Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    visit_fns_with_close(items, cb);
                }
            }
            syn::Item::Impl(im) => {
                for it in &im.items {
                    if let syn::ImplItem::Fn(mf) = it {
                        cb(&mf.block, mf.block.brace_token.span.close().end());
                    }
                }
            }
            _ => {}
        }
    }
}

struct ArmFinder<'a> {
    line_start: usize,
    line_end: usize,
    close: LineColumn,
    best: Option<MatchArmInfo<'a>>,
}

impl<'a, 'ast: 'a> Visit<'ast> for ArmFinder<'a> {
    fn visit_expr_match(&mut self, m: &'ast syn::ExprMatch) {
        for arm in &m.arms {
            let body_span = arm.body.span();
            let bs = body_span.start().line;
            let be = body_span.end().line;
            // Match if the requested range matches either the body span
            // exactly, OR (for block-bodied arms) the inner statement
            // range. Lets the caller pass either selector style.
            let (inner_s, inner_e) = if let syn::Expr::Block(blk) = &*arm.body {
                if let (Some(first), Some(last)) =
                    (blk.block.stmts.first(), blk.block.stmts.last())
                {
                    (first.span().start().line, last.span().end().line)
                } else {
                    (bs, be)
                }
            } else {
                (bs, be)
            };
            let matches_outer = bs == self.line_start && be == self.line_end;
            let matches_inner =
                inner_s == self.line_start && inner_e == self.line_end;
            if matches_outer || matches_inner {
                let arm_indent = arm
                    .pat
                    .span()
                    .start()
                    .column
                    .checked_sub(0)
                    .map(|c| " ".repeat(c))
                    .unwrap_or_default();
                self.best = Some(MatchArmInfo {
                    pat: &arm.pat,
                    body_expr: &arm.body,
                    body_start_line: bs,
                    body_end_line: be,
                    inner_start_line: inner_s,
                    inner_end_line: inner_e,
                    arm_indent,
                    has_trailing_comma: arm.comma.is_some(),
                    enclosing_fn_close: self.close,
                });
            }
        }
        syn::visit::visit_expr_match(self, m);
    }
}

#[derive(Default)]
struct ArmFlowScan {
    has_loop_escape: bool,
    loop_depth: u32,
}
impl<'ast> Visit<'ast> for ArmFlowScan {
    fn visit_expr_break(&mut self, _: &'ast syn::ExprBreak) {
        if self.loop_depth == 0 {
            self.has_loop_escape = true;
        }
    }
    fn visit_expr_continue(&mut self, _: &'ast syn::ExprContinue) {
        if self.loop_depth == 0 {
            self.has_loop_escape = true;
        }
    }
    fn visit_expr_for_loop(&mut self, e: &'ast syn::ExprForLoop) {
        self.loop_depth += 1;
        syn::visit::visit_expr_for_loop(self, e);
        self.loop_depth -= 1;
    }
    fn visit_expr_while(&mut self, e: &'ast syn::ExprWhile) {
        self.loop_depth += 1;
        syn::visit::visit_expr_while(self, e);
        self.loop_depth -= 1;
    }
    fn visit_expr_loop(&mut self, e: &'ast syn::ExprLoop) {
        self.loop_depth += 1;
        syn::visit::visit_expr_loop(self, e);
        self.loop_depth -= 1;
    }
}

// ---------------------------------------------------------------------------
// Feasibility probe used by `health --suggest-extractions`
// ---------------------------------------------------------------------------

/// Without writing anything, predict what `extract_fn` would do for a
/// given range. Returns `Refused(reason)` if extraction would fail,
/// otherwise distinguishes the rewrite-free `Ok` case from the
/// `V2OnlyWithRewrite` case.
pub fn feasibility_probe(
    file: &std::path::Path,
    line_start: usize,
    line_end: usize,
) -> Feasibility {
    let Ok(src) = std::fs::read_to_string(file) else {
        return Feasibility::Refused("io error".to_string());
    };
    let Ok(ast) = syn::parse_file(&src) else {
        return Feasibility::Refused("parse error".to_string());
    };

    if find_enclosing_match_arm(&ast, line_start, line_end).is_some() {
        // Match arms always need a v2 rewrite (the legacy v1 acceptor
        // refused them).
        return Feasibility::V2OnlyWithRewrite;
    }

    let Some(enclosing) = find_enclosing_fn(&ast, line_start, line_end) else {
        return Feasibility::Refused("no enclosing fn".to_string());
    };
    let outer_is_async = enclosing.sig.asyncness.is_some();
    let outer_is_method = first_is_self_receiver(&enclosing.sig);

    let inner_stmts = innermost_block_stmts(enclosing.block, line_start, line_end);
    let Ok((range_stmts, _, _)) = partition_stmts(inner_stmts, line_start, line_end) else {
        return Feasibility::Refused("range misaligned".to_string());
    };
    if range_stmts.is_empty() {
        return Feasibility::Refused("empty range".to_string());
    }

    let flow = scan_range_flow(&range_stmts);
    if flow.has_loop_escape {
        return Feasibility::Refused("break/continue targeting outer loop".to_string());
    }
    if flow.has_await && !outer_is_async {
        return Feasibility::Refused(".await in non-async fn".to_string());
    }
    if (flow.has_self_field || flow.has_self_method) && !outer_is_method {
        return Feasibility::Refused("self. in non-method fn".to_string());
    }
    if flow.has_return && flow.has_try {
        return Feasibility::Refused("mixed return + ? in same range".to_string());
    }

    let needs_rewrite = flow.has_return
        || flow.has_try
        || flow.has_await
        || flow.has_self_field
        || flow.has_self_method;
    if needs_rewrite {
        Feasibility::V2OnlyWithRewrite
    } else {
        Feasibility::Ok
    }
}

// ---------------------------------------------------------------------------
// Shared helpers (unchanged from v1 except where noted)
// ---------------------------------------------------------------------------

/// A view over a fn-like declaration — either a free `fn` or an
/// `impl` method. Carries just the bits extract-fn needs.
struct EnclosingFn<'a> {
    sig: &'a syn::Signature,
    block: &'a syn::Block,
}

/// Walk down through nested blocks (match arm bodies, if branches,
/// loop bodies) and return the slice of statements of the deepest
/// block that still fully contains [line_start, line_end].
fn innermost_block_stmts<'a>(
    block: &'a syn::Block,
    line_start: usize,
    line_end: usize,
) -> &'a [syn::Stmt] {
    // For each child block we recurse into, prefer it if it strictly
    // contains the range. Otherwise stay at the current level.
    for s in &block.stmts {
        if let Some(inner) = stmt_inner_block(s, line_start, line_end) {
            return innermost_block_stmts(inner, line_start, line_end);
        }
    }
    &block.stmts
}

fn stmt_inner_block<'a>(
    s: &'a syn::Stmt,
    line_start: usize,
    line_end: usize,
) -> Option<&'a syn::Block> {
    let expr = match s {
        syn::Stmt::Expr(e, _) => e,
        _ => return None,
    };
    expr_inner_block(expr, line_start, line_end)
}

fn expr_inner_block<'a>(
    expr: &'a syn::Expr,
    line_start: usize,
    line_end: usize,
) -> Option<&'a syn::Block> {
    fn contains(b: &syn::Block, ls: usize, le: usize) -> bool {
        // Strict containment: the block must open BEFORE the range and
        // close AFTER. Equal-start would mean the range covers the
        // block's brace line — extraction wouldn't make sense there.
        let s = b.span().start().line;
        let e = b.span().end().line;
        s < ls && le < e
    }
    match expr {
        syn::Expr::Block(b) if contains(&b.block, line_start, line_end) => Some(&b.block),
        syn::Expr::If(i) => {
            if contains(&i.then_branch, line_start, line_end) {
                return Some(&i.then_branch);
            }
            if let Some((_, else_expr)) = &i.else_branch {
                return expr_inner_block(else_expr, line_start, line_end);
            }
            None
        }
        syn::Expr::ForLoop(f) if contains(&f.body, line_start, line_end) => Some(&f.body),
        syn::Expr::While(w) if contains(&w.body, line_start, line_end) => Some(&w.body),
        syn::Expr::Loop(l) if contains(&l.body, line_start, line_end) => Some(&l.body),
        syn::Expr::Match(m) => {
            for arm in &m.arms {
                if let Some(b) = expr_inner_block(&arm.body, line_start, line_end) {
                    return Some(b);
                }
            }
            None
        }
        _ => None,
    }
}

fn find_enclosing_fn<'a>(
    file: &'a syn::File,
    line_start: usize,
    line_end: usize,
) -> Option<EnclosingFn<'a>> {
    let mut best: Option<EnclosingFn<'a>> = None;
    let mut best_width: Option<usize> = None;
    visit_fns(&file.items, &mut |f: EnclosingFn<'a>| {
        let s = f.block.span().start().line;
        let e = f.block.span().end().line;
        if s <= line_start && line_end <= e {
            let width = e - s;
            if best_width.map(|bw| width < bw).unwrap_or(true) {
                best_width = Some(width);
                best = Some(f);
            }
        }
    });
    best
}

fn visit_fns<'a, F: FnMut(EnclosingFn<'a>)>(items: &'a [syn::Item], cb: &mut F) {
    for it in items {
        match it {
            syn::Item::Fn(f) => cb(EnclosingFn {
                sig: &f.sig,
                block: &f.block,
            }),
            syn::Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    visit_fns(items, cb);
                }
            }
            syn::Item::Impl(im) => {
                for it in &im.items {
                    if let syn::ImplItem::Fn(mf) = it {
                        cb(EnclosingFn {
                            sig: &mf.sig,
                            block: &mf.block,
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

fn partition_stmts<'a>(
    stmts: &'a [syn::Stmt],
    line_start: usize,
    line_end: usize,
) -> Result<(Vec<&'a syn::Stmt>, Vec<&'a syn::Stmt>, Vec<&'a syn::Stmt>), ExtractError> {
    let mut before = Vec::new();
    let mut inside = Vec::new();
    let mut after = Vec::new();
    for s in stmts {
        let span = s.span();
        let s_line = span.start().line;
        let e_line = span.end().line;
        if e_line < line_start {
            before.push(s);
        } else if s_line >= line_start && e_line <= line_end {
            inside.push(s);
        } else if s_line > line_end {
            after.push(s);
        } else {
            return Err(ExtractError::RangeMisaligned);
        }
    }
    Ok((inside, before, after))
}

#[derive(Default)]
struct IdentCollector {
    idents: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for IdentCollector {
    fn visit_expr_path(&mut self, i: &'ast syn::ExprPath) {
        if i.qself.is_none() && i.path.segments.len() == 1 {
            self.idents
                .insert(i.path.segments[0].ident.to_string());
        }
        syn::visit::visit_expr_path(self, i);
    }
}

#[derive(Default)]
struct RangeAnalyzer {
    reads: BTreeSet<String>,
    writes: BTreeSet<String>,
}

impl RangeAnalyzer {
    fn lhs_ident(expr: &syn::Expr) -> Option<String> {
        match expr {
            syn::Expr::Path(p) if p.qself.is_none() && p.path.segments.len() == 1 => {
                Some(p.path.segments[0].ident.to_string())
            }
            _ => None,
        }
    }
}

impl<'ast> Visit<'ast> for RangeAnalyzer {
    fn visit_expr_path(&mut self, i: &'ast syn::ExprPath) {
        if i.qself.is_none() && i.path.segments.len() == 1 {
            self.reads
                .insert(i.path.segments[0].ident.to_string());
        }
    }
    fn visit_expr_assign(&mut self, i: &'ast syn::ExprAssign) {
        if let Some(name) = Self::lhs_ident(&i.left) {
            self.writes.insert(name);
        } else {
            self.visit_expr(&i.left);
        }
        self.visit_expr(&i.right);
    }
    fn visit_expr_binary(&mut self, i: &'ast syn::ExprBinary) {
        if is_compound_assign(&i.op) {
            if let Some(name) = Self::lhs_ident(&i.left) {
                self.writes.insert(name);
                self.visit_expr(&i.right);
                return;
            }
        }
        syn::visit::visit_expr_binary(self, i);
    }
    fn visit_expr_reference(&mut self, i: &'ast syn::ExprReference) {
        if i.mutability.is_some() {
            if let Some(name) = Self::lhs_ident(&i.expr) {
                self.writes.insert(name);
            }
        }
        syn::visit::visit_expr_reference(self, i);
    }
    fn visit_expr_method_call(&mut self, i: &'ast syn::ExprMethodCall) {
        self.visit_expr(&i.receiver);
        for a in &i.args {
            self.visit_expr(a);
        }
    }
    fn visit_expr_call(&mut self, i: &'ast syn::ExprCall) {
        if !matches!(&*i.func, syn::Expr::Path(_)) {
            self.visit_expr(&i.func);
        }
        for a in &i.args {
            self.visit_expr(a);
        }
    }
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        let toks = m.tokens.to_string();
        for tok in tokenize_idents(&toks) {
            self.reads.insert(tok);
        }
    }
}

fn is_compound_assign(op: &syn::BinOp) -> bool {
    matches!(
        op,
        syn::BinOp::AddAssign(_)
            | syn::BinOp::SubAssign(_)
            | syn::BinOp::MulAssign(_)
            | syn::BinOp::DivAssign(_)
            | syn::BinOp::RemAssign(_)
            | syn::BinOp::BitAndAssign(_)
            | syn::BinOp::BitOrAssign(_)
            | syn::BinOp::BitXorAssign(_)
            | syn::BinOp::ShlAssign(_)
            | syn::BinOp::ShrAssign(_)
    )
}

fn tokenize_idents(s: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_alphabetic() || c == '_' {
            cur.push(c);
            while let Some(&n) = chars.peek() {
                if n.is_ascii_alphanumeric() || n == '_' {
                    cur.push(n);
                    chars.next();
                } else {
                    break;
                }
            }
            if !is_rust_keyword(&cur) {
                out.insert(cur.clone());
            }
            cur.clear();
        }
    }
    out
}

fn is_rust_keyword(s: &str) -> bool {
    matches!(
        s,
        "let"
            | "mut"
            | "if"
            | "else"
            | "match"
            | "while"
            | "for"
            | "loop"
            | "return"
            | "break"
            | "continue"
            | "fn"
            | "pub"
            | "use"
            | "as"
            | "in"
            | "ref"
            | "self"
            | "Self"
            | "true"
            | "false"
            | "move"
            | "async"
            | "await"
            | "const"
            | "static"
            | "type"
            | "impl"
            | "trait"
            | "where"
    )
}

fn collect_bindings(stmts: &[&syn::Stmt]) -> Vec<String> {
    let mut out = Vec::new();
    for s in stmts {
        collect_bindings_one(s, &mut out);
    }
    out
}

fn collect_bindings_in_stmts(stmts: &[&syn::Stmt]) -> Vec<String> {
    let mut out = Vec::new();
    for s in stmts {
        collect_bindings_one(s, &mut out);
    }
    out
}

fn collect_bindings_one(s: &syn::Stmt, out: &mut Vec<String>) {
    if let syn::Stmt::Local(local) = s {
        pat_idents(&local.pat, out);
    }
}

fn pat_idents(p: &syn::Pat, out: &mut Vec<String>) {
    match p {
        syn::Pat::Ident(pi) => out.push(pi.ident.to_string()),
        syn::Pat::Tuple(t) => {
            for el in &t.elems {
                pat_idents(el, out);
            }
        }
        syn::Pat::TupleStruct(ts) => {
            for el in &ts.elems {
                pat_idents(el, out);
            }
        }
        syn::Pat::Struct(ps) => {
            for f in &ps.fields {
                pat_idents(&f.pat, out);
            }
        }
        syn::Pat::Reference(r) => pat_idents(&r.pat, out),
        syn::Pat::Type(t) => pat_idents(&t.pat, out),
        _ => {}
    }
}

fn stmts_text_range(src: &str, stmts: &[&syn::Stmt]) -> String {
    if stmts.is_empty() {
        return String::new();
    }
    let first_line = stmts.first().unwrap().span().start().line;
    let last_line = stmts.last().unwrap().span().end().line;
    stmts_text_range_lines(src, first_line, last_line)
}

fn stmts_text_range_lines(src: &str, first_line: usize, last_line: usize) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = String::new();
    for i in (first_line - 1)..=(last_line - 1) {
        if i >= lines.len() {
            break;
        }
        out.push_str(lines[i]);
        out.push('\n');
    }
    out
}

fn leading_indent(src: &str, at: LineColumn) -> String {
    let line = src.lines().nth(at.line - 1).unwrap_or("");
    let mut s = String::new();
    for c in line.chars() {
        if c == ' ' || c == '\t' {
            s.push(c);
        } else {
            break;
        }
    }
    s
}

/// wb-5lgj.41: collect any `use` statements defined locally inside
/// `enclosing_block` whose imported leaf identifiers appear as tokens
/// in `body_text`. Returns them as a newline-terminated string suitable
/// for prepending to the extracted fn body (4-space indented).
///
/// Conservative on resolution — we use a token scan rather than real
/// name resolution. False positives (hoisting an unused use) trip
/// `unused_imports` warnings, not errors. False negatives would silently
/// produce E0433 in the extracted fn, so we lean toward inclusion.
///
/// Glob imports (`use foo::*;`) are always hoisted when present, since
/// we can't enumerate their leaf names from the syn tree alone.
fn collect_hoisted_local_uses(enclosing_block: &syn::Block, body_text: &str) -> String {
    use syn::{Item, Stmt, UseTree};

    fn leaf_names(tree: &UseTree, out: &mut Vec<String>) -> bool {
        // Returns `is_glob` — caller hoists unconditionally when true.
        match tree {
            UseTree::Path(p) => leaf_names(&p.tree, out),
            UseTree::Name(n) => {
                out.push(n.ident.to_string());
                false
            }
            UseTree::Rename(r) => {
                out.push(r.rename.to_string());
                false
            }
            UseTree::Glob(_) => true,
            UseTree::Group(g) => {
                let mut g_glob = false;
                for sub in &g.items {
                    if leaf_names(sub, out) {
                        g_glob = true;
                    }
                }
                g_glob
            }
        }
    }

    fn token_in_body(name: &str, body: &str) -> bool {
        // Word-boundary check — name must appear as a whole token, not
        // a substring of a longer identifier.
        for (i, _) in body.match_indices(name) {
            let before_ok = i == 0
                || !body.as_bytes()[i - 1].is_ascii_alphanumeric()
                    && body.as_bytes()[i - 1] != b'_';
            let after = i + name.len();
            let after_ok = after >= body.len()
                || !body.as_bytes()[after].is_ascii_alphanumeric()
                    && body.as_bytes()[after] != b'_';
            if before_ok && after_ok {
                return true;
            }
        }
        false
    }

    let mut out = String::new();
    for stmt in &enclosing_block.stmts {
        let item_use = match stmt {
            Stmt::Item(Item::Use(u)) => u,
            _ => continue,
        };
        let mut names = Vec::new();
        let is_glob = leaf_names(&item_use.tree, &mut names);
        let any_used = is_glob || names.iter().any(|n| token_in_body(n, body_text));
        if !any_used {
            continue;
        }
        // Emit the use statement verbatim via syn's pretty-print equivalent.
        // prettyplease isn't a dep; manual rebuild via quote! would
        // require adding quote, and the syn::Item already round-trips
        // via Display? No — syn::Item doesn't impl Display. Construct
        // a minimal `use ...;` text from the UseTree via tokens.
        let use_text = use_item_to_text(item_use);
        out.push_str("    ");
        out.push_str(&use_text);
        out.push('\n');
    }
    out
}

/// Render a `syn::ItemUse` back to `use <path>;` text. Uses TokenStream
/// formatting which keeps the original token shape (paths, brace groups,
/// renames). Token output is space-separated rather than perfectly
/// pretty-printed but compiles identically.
fn use_item_to_text(item_use: &syn::ItemUse) -> String {
    use quote::ToTokens;
    let mut ts = proc_macro2::TokenStream::new();
    item_use.to_tokens(&mut ts);
    ts.to_string()
}

fn indent_body(body: &str) -> String {
    let lines: Vec<&str> = body.lines().collect();

    // wb-5lgj.42: when callers feed in body text from a syn span, the
    // FIRST line is column-stripped (the span starts at the token, not
    // at column 0) while subsequent lines include their original leading
    // whitespace. A naive min over all lines sees 0 on line 1 and
    // refuses to dedent line 2+, preserving (or amplifying) the source's
    // original deep indent. Skip the first non-empty line from the
    // min-indent computation iff it has zero leading spaces — then the
    // dedent reflects the actual indent of the captured statements.
    let nonblank: Vec<&&str> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    let min_indent = if nonblank.len() >= 2
        && nonblank[0].chars().take_while(|c| *c == ' ').count() == 0
    {
        nonblank
            .iter()
            .skip(1)
            .map(|l| l.chars().take_while(|c| *c == ' ').count())
            .min()
            .unwrap_or(0)
    } else {
        nonblank
            .iter()
            .map(|l| l.chars().take_while(|c| *c == ' ').count())
            .min()
            .unwrap_or(0)
    };
    let mut out = String::new();
    for l in &lines {
        if l.trim().is_empty() {
            out.push('\n');
            continue;
        }
        // First line was column-stripped by the span slicer — it has no
        // leading whitespace to remove. Strip min_indent only from lines
        // that actually have ≥ min_indent leading spaces.
        let leading = l.chars().take_while(|c| *c == ' ').count();
        let strip = std::cmp::min(leading, min_indent);
        let stripped: String = l.chars().skip(strip).collect();
        out.push_str("    ");
        out.push_str(&stripped);
        out.push('\n');
    }
    out
}

fn splice_source(
    src: &str,
    range_start_line: usize,
    range_end_line: usize,
    call_line: &str,
    enclosing_fn_close: LineColumn,
    new_fn_text: &str,
) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = String::new();
    let mut i = 0usize;
    while i < lines.len() {
        let lineno = i + 1;
        if lineno == range_start_line {
            out.push_str(call_line);
            out.push('\n');
            while i < lines.len() && (i + 1) <= range_end_line {
                i += 1;
            }
            continue;
        }
        out.push_str(lines[i]);
        out.push('\n');
        if lineno == enclosing_fn_close.line {
            out.push('\n');
            out.push_str(new_fn_text);
        }
        i += 1;
    }
    out
}
