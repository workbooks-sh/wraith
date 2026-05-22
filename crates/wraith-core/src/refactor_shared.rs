//! Small helpers shared by the wb-5lgj.31 refactor primitives
//! (`move-fn`, `rename`, `inline`, `split-fn`). Kept deliberately
//! narrow — anything bigger goes in its own module.

use std::path::{Path, PathBuf};

use crate::workspace::Workspace;

#[derive(Debug, Clone)]
pub struct FileEdit {
    pub path: PathBuf,
    pub new_contents: String,
}

/// Token-aware identifier replacement. Identical semantics to
/// `dedupe::rewrite_call_sites` — replaces whole-token occurrences of
/// `from` with `to`, never substrings.
pub fn rename_idents(src: &str, from: &str, to: &str) -> String {
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

/// Collect every `.rs` file across every workspace crate.
pub fn workspace_rs_files(ws: &Workspace) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for c in &ws.crates {
        for f in ws.crate_rs_files(c) {
            if !files.contains(&f) {
                files.push(f);
            }
        }
    }
    files
}

/// Line-start byte offsets for cheap slicing.
pub fn line_offsets(src: &str) -> Vec<usize> {
    let mut offs = vec![0usize];
    for (i, b) in src.bytes().enumerate() {
        if b == b'\n' {
            offs.push(i + 1);
        }
    }
    offs
}

/// Find an `ItemFn` by name anywhere in the file (including inside
/// inline modules). Returns the AST item plus its byte span (including
/// preceding attributes / doc comments) and starting line.
pub fn find_fn_in_file<'a>(
    file: &'a syn::File,
    leaf: &str,
) -> Option<&'a syn::ItemFn> {
    let mut found: Option<&'a syn::ItemFn> = None;
    walk_fns(&file.items, &mut |f: &'a syn::ItemFn| {
        if found.is_none() && f.sig.ident == leaf {
            found = Some(f);
        }
    });
    found
}

pub fn walk_fns<'a, F: FnMut(&'a syn::ItemFn)>(items: &'a [syn::Item], cb: &mut F) {
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

/// Byte span [start, end) of an `ItemFn`, walking back to include any
/// immediately preceding attribute / doc-comment lines. End span
/// includes the closing brace and the trailing newline (if present).
pub fn fn_span_bytes(src: &str, item: &syn::ItemFn) -> (usize, usize, usize) {
    use syn::spanned::Spanned;
    let start_line = item.sig.fn_token.span.start().line;
    let end_line = item.block.brace_token.span.close().end().line;
    let line_offsets = line_offsets(src);

    let mut effective_start_line = start_line;
    while effective_start_line > 1 {
        let prev = effective_start_line - 1;
        let line_text = line_at(src, &line_offsets, prev).trim_start();
        // Walk back through outer attributes / doc comments that
        // annotate the fn. `//!` is an inner doc comment and belongs
        // to the enclosing module — do not pull it into the fn span.
        if line_text.starts_with("#[") || line_text.starts_with("///") {
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
    let _ = item.span();
    (start_byte, end_byte, effective_start_line)
}

fn line_at<'a>(src: &'a str, line_offsets: &[usize], line: usize) -> &'a str {
    let start = line_offsets[line - 1];
    let end = line_offsets.get(line).copied().unwrap_or(src.len());
    &src[start..end.min(src.len())]
}

/// Apply a slice of `FileEdit`s to disk. Empty `path` strings are
/// skipped (used by tests).
pub fn apply_edits(edits: &[FileEdit]) -> std::io::Result<usize> {
    for e in edits {
        if let Some(parent) = e.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&e.path, &e.new_contents)?;
    }
    Ok(edits.len())
}

/// True when `src` (a `.rs` file body) contains a *whole-token* match
/// of `name`. Used as a cheap "does this file reference X?" filter.
pub fn file_mentions_token(src: &str, name: &str) -> bool {
    let mut in_ident = false;
    let mut cur = String::new();
    for c in src.chars() {
        let is_id = c.is_ascii_alphanumeric() || c == '_';
        if is_id != in_ident {
            if in_ident && cur == name {
                return true;
            }
            cur.clear();
            in_ident = is_id;
        }
        cur.push(c);
    }
    in_ident && cur == name
}

/// Split a qualified symbol into (crate_or_root, module_path_segments,
/// leaf_name). `wavelet::foo` → ("wavelet", [], "foo"); `wavelet::a::b`
/// → ("wavelet", ["a"], "b").
pub fn split_qualified(qualified: &str) -> (String, Vec<String>, String) {
    let parts: Vec<&str> = qualified.split("::").collect();
    if parts.is_empty() {
        return (String::new(), Vec::new(), qualified.to_string());
    }
    if parts.len() == 1 {
        return (String::new(), Vec::new(), parts[0].to_string());
    }
    let leaf = parts.last().unwrap().to_string();
    let root = parts[0].to_string();
    let mids: Vec<String> = parts[1..parts.len() - 1].iter().map(|s| s.to_string()).collect();
    (root, mids, leaf)
}

/// Parse a `<file>:<fn-name>` selector used by move-fn / inline / split-fn.
pub fn parse_file_fn_selector(selector: &str) -> anyhow::Result<(PathBuf, String)> {
    let (file_s, name) = selector
        .rsplit_once(':')
        .ok_or_else(|| anyhow::anyhow!("selector must be <file>:<fn-name>, got `{selector}`"))?;
    Ok((PathBuf::from(file_s), name.to_string()))
}

/// Load + parse a single Rust file. Path-tagged errors.
pub fn load_and_parse(path: &Path) -> anyhow::Result<(String, syn::File)> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
    let ast = syn::parse_file(&src)
        .map_err(|e| anyhow::anyhow!("failed to parse {}: {e}", path.display()))?;
    Ok((src, ast))
}
