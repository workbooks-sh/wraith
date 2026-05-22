//! `extract-shared` refactor — agent-driven cluster unification
//! (wb-5lgj.30 part B).
//!
//! Inputs (all decided by the calling agent):
//!   - unified fn signature (parsed via `syn::parse_str::<ItemFn>`)
//!   - per-member param mapping (which divergent value gets passed at
//!     each call site)
//!   - extraction target module path (`crate::util`, etc.)
//!
//! Behavior:
//!   1. Resolve cluster + run diff_cluster::diff_members
//!   2. Verify the mapping covers every divergence
//!   3. Generate the shared fn: common skeleton from member[0],
//!      substitute divergent positions with the param idents identified
//!      from the mapping. Original member params are renamed
//!      positionally onto the new signature.
//!   4. Write the shared fn at `extract_to`, delete member defs, rewrite
//!      call sites.
//!   5. cargo check; rollback on failure.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use proc_macro2::{Delimiter, Group, Ident, Span, TokenStream, TokenTree};
use quote::ToTokens;
use syn::spanned::Spanned;

use crate::dedupe::DedupeError;
use crate::diff_cluster::{diff_members, flatten_tokens, token_text, Divergence, DivergenceKind};
use crate::report::{ClusterMember, Finding, FindingKind};

#[derive(Debug, thiserror::Error)]
pub enum ExtractSharedError {
    #[error("not a duplicate-cluster finding")]
    NotACluster,
    #[error("failed to parse --signature `{sig}`: {source}")]
    BadSignature {
        sig: String,
        #[source]
        source: syn::Error,
    },
    #[error("param-mapping missing keys: [{}]", missing.join(", "))]
    MissingMapping { missing: Vec<String> },
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("dedupe: {0}")]
    Dedupe(#[from] DedupeError),
    #[error("cargo check failed after extract-shared (rolled back)")]
    BuildFailed { stderr: String },
    #[error("cluster has no members")]
    EmptyCluster,
    #[error("invalid --param-mapping shape: expected object of object-of-strings")]
    BadMappingShape,
    #[error("invalid --extract-to `{0}` — must be like `crate::util` or `crate_a::util::mod`")]
    BadExtractTo(String),
}

#[derive(Debug, Clone)]
pub struct ExtractSharedOptions {
    pub root: PathBuf,
    pub signature: String,
    /// JSON object `{ "<symbol>": { "<param>": "<rust-expr>" } }`.
    pub param_mapping: serde_json::Value,
    pub extract_to: String,
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
pub struct FileEdit {
    pub path: PathBuf,
    pub new_contents: String,
}

#[derive(Debug, Clone)]
pub struct ExtractSharedPlan {
    pub shared_fn_text: String,
    pub edits: Vec<FileEdit>,
    pub extract_to_path: PathBuf,
    pub verified: bool,
}

pub fn extract_shared(
    finding: &Finding,
    opts: &ExtractSharedOptions,
) -> Result<ExtractSharedPlan, ExtractSharedError> {
    let members = match &finding.kind {
        FindingKind::DuplicateCluster { members, .. } => members.clone(),
        _ => return Err(ExtractSharedError::NotACluster),
    };
    if members.is_empty() {
        return Err(ExtractSharedError::EmptyCluster);
    }

    let new_sig: syn::ItemFn = syn::parse_str(&format!("{} {{}}", opts.signature))
        .map_err(|e| ExtractSharedError::BadSignature {
            sig: opts.signature.clone(),
            source: e,
        })?;

    let mapping = parse_mapping(&opts.param_mapping)?;
    let diff = diff_members(&format!("{}", members[0].symbol), &members)?;

    // Verify mapping completeness: for each divergence and each member,
    // we need a param-mapping entry that names the param + value.
    verify_mapping_completeness(&members, &diff.divergences, &mapping)?;

    // Determine, for each divergence position, which param it
    // corresponds to (by matching the per-member value).
    let position_to_param = assign_positions_to_params(&diff.divergences, &mapping)?;

    // Generate shared fn body from member[0] skeleton with divergence
    // substitutions + original-param positional rename.
    let member0 = &members[0];
    let resolved0 = crate::diff_cluster::resolve_member(member0)?;
    let new_fn_text = generate_shared_fn(
        &new_sig,
        &resolved0.item,
        &diff.divergences,
        &position_to_param,
        member0,
    );

    // Compute target file for the shared fn.
    let (extract_to_path, extract_existing, lib_mod_decl) =
        resolve_extract_to(&opts.root, &opts.extract_to, &members)?;

    let mut edits: Vec<FileEdit> = Vec::new();

    // Write shared fn into target file (create or append).
    let shared_file_contents = if let Some(existing) = &extract_existing {
        let mut s = existing.clone();
        if !s.ends_with('\n') {
            s.push('\n');
        }
        s.push('\n');
        s.push_str(&new_fn_text);
        s
    } else {
        new_fn_text.clone()
    };
    edits.push(FileEdit {
        path: extract_to_path.clone(),
        new_contents: shared_file_contents,
    });

    // Queue the lib.rs mod decl edit if needed.
    if let Some((lib_path, new_lib)) = lib_mod_decl {
        if let Some(pos) = edits.iter().position(|e| e.path == lib_path) {
            edits[pos].new_contents = new_lib;
        } else {
            edits.push(FileEdit {
                path: lib_path,
                new_contents: new_lib,
            });
        }
    }

    // For each cluster member: rewrite its file to delete the def,
    // insert a use of the canonical path, and rewrite local callers.
    let shared_leaf = new_sig.sig.ident.to_string();
    let canonical_use_path = format!("{}::{}", opts.extract_to.trim_start_matches("crate::"), shared_leaf);
    let canonical_use_full = if opts.extract_to.starts_with("crate::") {
        format!("crate::{}", canonical_use_path)
    } else {
        format!("{}::{}", opts.extract_to, shared_leaf)
    };

    // Group members by file (a file may host multiple cluster members
    // even if rare).
    let mut by_file: BTreeMap<PathBuf, Vec<ClusterMember>> = BTreeMap::new();
    for m in &members {
        by_file.entry(PathBuf::from(&m.file)).or_default().push(m.clone());
    }

    for (file, mems) in &by_file {
        // Skip if this file is the extract_to file (we already wrote
        // the shared fn there; we'll still want to delete originals
        // if any reside there).
        let mut src = if let Some(prev) = edits.iter().rposition(|e| e.path == *file) {
            edits[prev].new_contents.clone()
        } else {
            std::fs::read_to_string(file).map_err(|e| ExtractSharedError::Io {
                path: file.clone(),
                source: e,
            })?
        };

        // Delete each member's definition (descending order to preserve
        // byte offsets).
        let mut spans: Vec<(usize, usize, String)> = Vec::new();
        for m in mems {
            if let Some((s, e)) = locate_fn_span_in_source(&src, &leaf_of(&m.symbol)) {
                spans.push((s, e, leaf_of(&m.symbol)));
            }
        }
        spans.sort_by(|a, b| b.0.cmp(&a.0));
        for (s, e, _) in &spans {
            let mut s = *s;
            let mut e = *e;
            while e < src.len() && src.as_bytes()[e] == b'\n' {
                e += 1;
                break;
            }
            while s > 0 && src.as_bytes()[s - 1] == b'\n'
                && s >= 2
                && src.as_bytes()[s - 2] == b'\n'
            {
                s -= 1;
                break;
            }
            src.replace_range(s..e, "");
        }

        // Rewrite call sites: each member-leaf(args) → shared-leaf(mapped_args).
        for m in mems {
            let leaf = leaf_of(&m.symbol);
            let member_map = mapping.get(&m.symbol).or_else(|| mapping.get(&leaf));
            if let Some(pm) = member_map {
                src = rewrite_call_sites(&src, &leaf, &shared_leaf, pm, &new_sig);
            }
        }

        // Insert `use <canonical>;` if the file isn't where shared fn
        // lives.
        if file != &extract_to_path {
            let import = format!("use {};", canonical_use_full);
            if !src.contains(&import) {
                src = insert_use(&src, &import);
            }
        }

        // Upsert this edit.
        if let Some(pos) = edits.iter().position(|e| e.path == *file) {
            edits[pos].new_contents = src;
        } else {
            edits.push(FileEdit {
                path: file.clone(),
                new_contents: src,
            });
        }
    }

    // Also rewrite callers in OTHER files (workspace-wide ident-aware
    // substitution + insert use).
    rewrite_external_callers(&opts.root, &members, &shared_leaf, &mapping, &new_sig, &canonical_use_full, &mut edits, &extract_to_path)?;

    // Apply edits to disk (unless dry-run) and run cargo check.
    let mut verified = false;
    if !opts.dry_run {
        let backups = snapshot_files(&edits)?;
        apply_edits(&edits)?;
        match run_cargo_check(&opts.root) {
            Ok(()) => {
                verified = true;
            }
            Err(stderr) => {
                restore_files(&backups)?;
                return Err(ExtractSharedError::BuildFailed { stderr });
            }
        }
    }

    Ok(ExtractSharedPlan {
        shared_fn_text: new_fn_text,
        edits,
        extract_to_path,
        verified,
    })
}

// ----- helpers --------------------------------------------------------

fn leaf_of(symbol: &str) -> String {
    symbol.rsplit("::").next().unwrap_or(symbol).to_string()
}

type Mapping = BTreeMap<String, BTreeMap<String, String>>;

fn parse_mapping(v: &serde_json::Value) -> Result<Mapping, ExtractSharedError> {
    let obj = v.as_object().ok_or(ExtractSharedError::BadMappingShape)?;
    let mut out: Mapping = BTreeMap::new();
    for (k, inner) in obj {
        let inner_obj = inner.as_object().ok_or(ExtractSharedError::BadMappingShape)?;
        let mut m = BTreeMap::new();
        for (pname, pval) in inner_obj {
            let s = pval.as_str().ok_or(ExtractSharedError::BadMappingShape)?;
            m.insert(pname.clone(), s.to_string());
        }
        out.insert(k.clone(), m);
    }
    Ok(out)
}

fn verify_mapping_completeness(
    members: &[ClusterMember],
    divergences: &[Divergence],
    mapping: &Mapping,
) -> Result<(), ExtractSharedError> {
    let non_blocking: Vec<&Divergence> = divergences
        .iter()
        .filter(|d| {
            matches!(
                d.kind,
                DivergenceKind::StringLiteral | DivergenceKind::Numeric
            )
        })
        .collect();
    let mut missing = Vec::new();
    for m in members {
        let leaf = leaf_of(&m.symbol);
        let entry = mapping.get(&m.symbol).or_else(|| mapping.get(&leaf));
        let Some(entry) = entry else {
            missing.push(m.symbol.clone());
            continue;
        };
        // For each non-blocking divergence, the entry must include
        // SOME param whose value matches the divergence value for this
        // member. We don't require a particular param name; we only
        // require coverage.
        for d in &non_blocking {
            let val = d.values_by_member.get(&m.symbol);
            let Some(val) = val else { continue };
            let covered = entry.values().any(|v| v.trim() == val.trim());
            if !covered {
                missing.push(format!("{}/{}", m.symbol, d.position.path));
            }
        }
    }
    if !missing.is_empty() {
        return Err(ExtractSharedError::MissingMapping { missing });
    }
    Ok(())
}

/// For each non-blocking divergence position, decide which signature
/// param it represents — by checking which mapping param name shows
/// the divergence's per-member value.
fn assign_positions_to_params(
    divergences: &[Divergence],
    mapping: &Mapping,
) -> Result<BTreeMap<usize, String>, ExtractSharedError> {
    let mut out: BTreeMap<usize, String> = BTreeMap::new();
    for d in divergences {
        if !matches!(
            d.kind,
            DivergenceKind::StringLiteral | DivergenceKind::Numeric
        ) {
            continue;
        }
        // Find the param name whose value matches the per-member
        // divergence values across mapping entries.
        let mut chosen: Option<String> = None;
        if let Some((sym0, val0)) = d.values_by_member.iter().next() {
            let leaf = leaf_of(sym0);
            let Some(entry) = mapping.get(sym0).or_else(|| mapping.get(&leaf)) else {
                continue;
            };
            for (param, v) in entry {
                if v.trim() == val0.trim() {
                    chosen = Some(param.clone());
                    break;
                }
            }
        }
        if let Some(p) = chosen {
            out.insert(d.position.index, p);
        }
    }
    Ok(out)
}

fn generate_shared_fn(
    new_sig: &syn::ItemFn,
    member0_item: &syn::ItemFn,
    divergences: &[Divergence],
    position_to_param: &BTreeMap<usize, String>,
    _member0: &ClusterMember,
) -> String {
    // Build a substitution map from divergence index → param ident.
    // Then walk the member[0] body TokenStream and substitute matching
    // flat positions.
    let mut body_ts = TokenStream::new();
    member0_item.block.to_tokens(&mut body_ts);

    // Original param idents → new param idents (positional rename).
    let orig_params = collect_param_idents(&member0_item.sig);
    let new_params = collect_param_idents(&new_sig.sig);
    let mut param_rename: BTreeMap<String, String> = BTreeMap::new();
    for (i, orig) in orig_params.iter().enumerate() {
        if let Some(new) = new_params.get(i) {
            param_rename.insert(orig.clone(), new.clone());
        }
    }

    let mut index = 0usize;
    let new_block = substitute_tokens(
        &body_ts,
        divergences,
        position_to_param,
        &param_rename,
        &mut index,
    );

    let sig_text = new_sig.sig.to_token_stream().to_string();
    // Reformat for readability.
    let body_text = new_block.to_string();
    format!("pub {sig_text} {body_text}\n")
}

fn collect_param_idents(sig: &syn::Signature) -> Vec<String> {
    let mut out = Vec::new();
    for arg in &sig.inputs {
        if let syn::FnArg::Typed(pt) = arg {
            if let syn::Pat::Ident(pi) = &*pt.pat {
                out.push(pi.ident.to_string());
            }
        }
    }
    out
}

fn substitute_tokens(
    ts: &TokenStream,
    divergences: &[Divergence],
    position_to_param: &BTreeMap<usize, String>,
    param_rename: &BTreeMap<String, String>,
    index: &mut usize,
) -> TokenStream {
    let mut out = TokenStream::new();
    let div_index_set: BTreeMap<usize, String> = divergences
        .iter()
        .filter_map(|d| {
            position_to_param
                .get(&d.position.index)
                .map(|p| (d.position.index, p.clone()))
        })
        .collect();

    for tt in ts.clone() {
        match tt {
            TokenTree::Group(g) => {
                // Emit the open-token slot first.
                let cur = *index;
                *index += 1;
                let nested = substitute_tokens(
                    &g.stream(),
                    divergences,
                    position_to_param,
                    param_rename,
                    index,
                );
                let mut new_g = Group::new(g.delimiter(), nested);
                new_g.set_span(g.span());
                out.extend(std::iter::once(TokenTree::Group(new_g)));
                // Close-token slot.
                *index += 1;
                let _ = cur;
            }
            TokenTree::Ident(id) => {
                let cur = *index;
                *index += 1;
                if let Some(p) = div_index_set.get(&cur) {
                    out.extend(std::iter::once(TokenTree::Ident(Ident::new(
                        p,
                        Span::call_site(),
                    ))));
                } else if let Some(renamed) = param_rename.get(&id.to_string()) {
                    out.extend(std::iter::once(TokenTree::Ident(Ident::new(
                        renamed,
                        id.span(),
                    ))));
                } else {
                    out.extend(std::iter::once(TokenTree::Ident(id)));
                }
            }
            TokenTree::Literal(lit) => {
                let cur = *index;
                *index += 1;
                if let Some(p) = div_index_set.get(&cur) {
                    out.extend(std::iter::once(TokenTree::Ident(Ident::new(
                        p,
                        Span::call_site(),
                    ))));
                } else {
                    out.extend(std::iter::once(TokenTree::Literal(lit)));
                }
            }
            TokenTree::Punct(p) => {
                *index += 1;
                out.extend(std::iter::once(TokenTree::Punct(p)));
            }
        }
    }
    out
}

/// Returns (target_path, existing_contents, optional_lib_mod_decl_edit).
/// The lib-mod-decl edit is a `(lib_rs_path, new_contents)` that the
/// caller queues as an edit (instead of writing directly), so dry-run
/// stays read-only.
fn resolve_extract_to(
    root: &Path,
    extract_to: &str,
    members: &[ClusterMember],
) -> Result<(PathBuf, Option<String>, Option<(PathBuf, String)>), ExtractSharedError> {
    // Strip leading `crate::` if present.
    let path = extract_to.trim_start_matches("crate::");
    let parts: Vec<&str> = path.split("::").collect();
    if parts.is_empty() {
        return Err(ExtractSharedError::BadExtractTo(extract_to.to_string()));
    }

    // Determine the host crate for the shared fn: if `crate::...`, use
    // the crate of member[0]; otherwise the first segment names the crate.
    let (crate_dir, mod_segments) = if extract_to.starts_with("crate::") {
        let crate_dir = crate_dir_for_member(root, &members[0])?;
        (crate_dir, parts.to_vec())
    } else {
        // First segment is crate name (snake-case).
        let crate_name = parts[0];
        let crate_dir = find_crate_dir_by_name(root, crate_name)?;
        let rest: Vec<&str> = parts[1..].to_vec();
        (crate_dir, rest)
    };

    if mod_segments.is_empty() {
        // Default to crate's lib.rs / main.rs.
        let lib_rs = crate_dir.join("src").join("lib.rs");
        if lib_rs.exists() {
            let existing = std::fs::read_to_string(&lib_rs).ok();
            return Ok((lib_rs, existing, None));
        }
    }

    let src_dir = crate_dir.join("src");
    let mod_name = mod_segments[mod_segments.len() - 1];
    let mod_file = src_dir.join(format!("{mod_name}.rs"));
    if mod_file.exists() {
        let existing = std::fs::read_to_string(&mod_file).ok();
        return Ok((mod_file, existing, None));
    }
    // Queue a `pub mod <name>;` insertion in lib.rs as a deferred edit
    // (so dry-run can preview without writing).
    let lib_rs = src_dir.join("lib.rs");
    let lib_edit = if lib_rs.exists() {
        let lib_src = std::fs::read_to_string(&lib_rs).unwrap_or_default();
        let mod_decl = format!("pub mod {mod_name};");
        if !lib_src.contains(&mod_decl) {
            let mut updated = lib_src.clone();
            if !updated.ends_with('\n') {
                updated.push('\n');
            }
            updated.push_str(&mod_decl);
            updated.push('\n');
            Some((lib_rs, updated))
        } else {
            None
        }
    } else {
        None
    };
    Ok((mod_file, None, lib_edit))
}

fn crate_dir_for_member(root: &Path, m: &ClusterMember) -> Result<PathBuf, ExtractSharedError> {
    // Walk up from the file path until we find a Cargo.toml. Restrict
    // search to within `root`.
    let mut cur = PathBuf::from(&m.file);
    cur.pop();
    while cur.starts_with(root) {
        if cur.join("Cargo.toml").exists() {
            return Ok(cur);
        }
        if !cur.pop() {
            break;
        }
    }
    Ok(root.to_path_buf())
}

fn find_crate_dir_by_name(root: &Path, crate_name: &str) -> Result<PathBuf, ExtractSharedError> {
    let mut candidates = vec![
        root.join(crate_name),
        root.join(crate_name.replace('_', "-")),
    ];
    candidates.retain(|p| p.join("Cargo.toml").exists());
    if let Some(p) = candidates.into_iter().next() {
        return Ok(p);
    }
    Ok(root.join(crate_name))
}

fn locate_fn_span_in_source(src: &str, leaf: &str) -> Option<(usize, usize)> {
    let ast = syn::parse_file(src).ok()?;
    let mut found: Option<(usize, usize)> = None;
    visit_items(&ast.items, &mut |item: &syn::Item| {
        if let syn::Item::Fn(f) = item {
            if f.sig.ident == leaf && found.is_none() {
                let start_line = f.sig.fn_token.span.start().line;
                let end_line = f.block.brace_token.span.close().end().line;
                let offsets = line_offsets(src);
                // Walk back to absorb #[attr] / /// lines.
                let mut effective_start = start_line;
                while effective_start > 1 {
                    let prev = effective_start - 1;
                    let txt = line_text(src, &offsets, prev).trim_start();
                    if txt.starts_with("#[")
                        || txt.starts_with("///")
                        || txt.starts_with("//!")
                    {
                        effective_start = prev;
                    } else {
                        break;
                    }
                }
                let start_byte = offsets[effective_start - 1];
                let end_byte = if end_line >= offsets.len() {
                    src.len()
                } else {
                    offsets[end_line]
                };
                found = Some((start_byte, end_byte));
            }
        }
    });
    found
}

fn visit_items<F: FnMut(&syn::Item)>(items: &[syn::Item], cb: &mut F) {
    for it in items {
        cb(it);
        if let syn::Item::Mod(m) = it {
            if let Some((_, items)) = &m.content {
                visit_items(items, cb);
            }
        }
    }
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

fn line_text<'a>(src: &'a str, offs: &[usize], line: usize) -> &'a str {
    let s = offs[line - 1];
    let e = offs.get(line).copied().unwrap_or(src.len());
    &src[s..e.min(src.len())]
}

fn rewrite_call_sites(
    src: &str,
    from_leaf: &str,
    to_leaf: &str,
    member_map: &BTreeMap<String, String>,
    new_sig: &syn::ItemFn,
) -> String {
    // For each call expression matching `from_leaf(args)`, replace with
    // `to_leaf(mapped_args)` where mapped_args is the new sig's params
    // resolved through member_map.
    let mapped_args: Vec<String> = collect_param_idents(&new_sig.sig)
        .iter()
        .map(|p| {
            member_map
                .get(p)
                .cloned()
                .unwrap_or_else(|| format!("/* missing {} */", p))
        })
        .collect();
    let replacement_call = format!("{}({})", to_leaf, mapped_args.join(", "));

    // Token-level scan: find runs `from_leaf` followed by `(`. Replace
    // the entire `from_leaf(...balanced parens...)` with replacement_call.
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    while i < bytes.len() {
        let rest = &src[i..];
        if let Some(_pos) = rest.strip_prefix(from_leaf) {
            // Ensure the prior char is not an ident char.
            let prev_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            // Ensure the next char (after from_leaf) is `(`, optionally
            // through whitespace, AND the following character after
            // from_leaf is not an ident char (already true since '(').
            let after = i + from_leaf.len();
            let next_non_ws = skip_ws_idx(src, after);
            if prev_ok
                && next_non_ws < bytes.len()
                && bytes[next_non_ws] == b'('
                && (after == bytes.len() || !is_ident_byte(bytes[after]) || bytes[after] == b'(')
            {
                // Verify next byte after from_leaf is not part of an
                // ident continuation (so we don't match `from_leaf_x`).
                if after < bytes.len() && is_ident_byte(bytes[after]) {
                    out.push(src.as_bytes()[i] as char);
                    i += 1;
                    continue;
                }
                // Find balanced close paren.
                if let Some(close_idx) = find_balanced_close(src, next_non_ws) {
                    out.push_str(&replacement_call);
                    i = close_idx + 1;
                    continue;
                }
            }
        }
        let ch_len = src[i..].chars().next().unwrap().len_utf8();
        out.push_str(&src[i..i + ch_len]);
        i += ch_len;
    }
    out
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn skip_ws_idx(src: &str, start: usize) -> usize {
    let bytes = src.as_bytes();
    let mut i = start;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    i
}

fn find_balanced_close(src: &str, open_paren_idx: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut depth = 0i32;
    let mut i = open_paren_idx;
    let mut in_str = false;
    let mut str_ch: u8 = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_str {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == str_ch {
                in_str = false;
            }
        } else {
            match b {
                b'"' | b'\'' => {
                    in_str = true;
                    str_ch = b;
                }
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

fn insert_use(src: &str, import_line: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let mut last_use_idx: Option<usize> = None;
    for (i, l) in lines.iter().enumerate() {
        if l.trim_start().starts_with("use ") {
            last_use_idx = Some(i);
        }
    }
    let insert_at = match last_use_idx {
        Some(i) => i + 1,
        None => {
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
    let had_nl = src.ends_with('\n');
    let mut out = out_lines.join("\n");
    if had_nl {
        out.push('\n');
    }
    out
}

fn rewrite_external_callers(
    root: &Path,
    members: &[ClusterMember],
    shared_leaf: &str,
    mapping: &Mapping,
    new_sig: &syn::ItemFn,
    canonical_use_full: &str,
    edits: &mut Vec<FileEdit>,
    extract_to_path: &Path,
) -> Result<(), ExtractSharedError> {
    let member_files: std::collections::BTreeSet<PathBuf> = members
        .iter()
        .map(|m| PathBuf::from(&m.file))
        .collect();
    // Walk all .rs files under root.
    let walker = walkdir::WalkDir::new(root);
    for entry in walker.into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if !p.extension().map(|e| e == "rs").unwrap_or(false) {
            continue;
        }
        if member_files.contains(p) || p == extract_to_path {
            continue;
        }
        let src = match std::fs::read_to_string(p) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mut updated = src.clone();
        let mut touched = false;
        for m in members {
            let leaf = leaf_of(&m.symbol);
            let member_map = mapping.get(&m.symbol).or_else(|| mapping.get(&leaf));
            let Some(pm) = member_map else { continue };
            if updated.contains(&leaf) {
                let after = rewrite_call_sites(&updated, &leaf, shared_leaf, pm, new_sig);
                if after != updated {
                    updated = after;
                    touched = true;
                }
            }
        }
        if touched {
            let import = format!("use {};", canonical_use_full);
            if !updated.contains(&import) {
                updated = insert_use(&updated, &import);
            }
            // Upsert.
            if let Some(pos) = edits.iter().position(|e| e.path == *p) {
                edits[pos].new_contents = updated;
            } else {
                edits.push(FileEdit {
                    path: p.to_path_buf(),
                    new_contents: updated,
                });
            }
        }
    }
    Ok(())
}

fn snapshot_files(edits: &[FileEdit]) -> Result<Vec<(PathBuf, Option<String>)>, ExtractSharedError> {
    let mut backups = Vec::new();
    for e in edits {
        let prev = std::fs::read_to_string(&e.path).ok();
        backups.push((e.path.clone(), prev));
    }
    Ok(backups)
}

fn apply_edits(edits: &[FileEdit]) -> Result<(), ExtractSharedError> {
    for e in edits {
        if let Some(parent) = e.path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| ExtractSharedError::Io {
                path: e.path.clone(),
                source: err,
            })?;
        }
        std::fs::write(&e.path, &e.new_contents).map_err(|err| ExtractSharedError::Io {
            path: e.path.clone(),
            source: err,
        })?;
    }
    Ok(())
}

fn restore_files(backups: &[(PathBuf, Option<String>)]) -> Result<(), ExtractSharedError> {
    for (p, prev) in backups {
        match prev {
            Some(s) => std::fs::write(p, s).ok(),
            None => std::fs::remove_file(p).ok(),
        };
    }
    Ok(())
}

fn run_cargo_check(root: &Path) -> Result<(), String> {
    let out = Command::new("cargo")
        .arg("check")
        .arg("--quiet")
        .current_dir(root)
        .output()
        .map_err(|e| format!("failed to spawn cargo: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).to_string())
    }
}

// Silence unused-import lint when these helpers haven't yet been called.
#[allow(dead_code)]
fn _silence() {
    let _ = (Delimiter::Parenthesis, |t: &TokenTree| token_text(t));
    let _ = flatten_tokens;
    let _ = syn::Item::Fn as fn(_) -> _;
}

#[allow(dead_code)]
fn _silence_spanned(s: Span) {
    let _ = s.source_text();
}

#[allow(dead_code)]
fn _silence_spanned_use(x: &syn::ItemFn) {
    let _ = x.block.span();
}
