//! `wraith refactor split-fn` — split a fn body at a statement
//! boundary into two named fns. The first fn captures the values
//! introduced before the split point and returns them; the second fn
//! takes those values as args and runs the remainder.
//!
//! v1 scope (wb-5lgj.31): refuses if the split line isn't at a top-
//! level statement boundary inside the target fn body.

use std::path::PathBuf;

use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::refactor_shared::{
    apply_edits, find_fn_in_file, fn_span_bytes, line_offsets, load_and_parse, FileEdit,
};

#[derive(Debug, thiserror::Error)]
pub enum SplitFnError {
    #[error("function `{0}` not found in `{1}`")]
    NotFound(String, PathBuf),
    #[error("line {0} is not at a statement boundary in `{1}`")]
    NotAtStatementBoundary(usize, String),
    #[error("split would produce an empty half")]
    EmptyHalf,
    #[error("--names must be exactly `<first>,<second>`")]
    BadNames,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Clone)]
pub struct SplitFnOptions {
    pub file: PathBuf,
    pub fn_name: String,
    pub at_line: usize,
    pub first_name: String,
    pub second_name: String,
}

#[derive(Debug, Clone)]
pub struct SplitFnResult {
    pub edits: Vec<FileEdit>,
    pub intermediates: Vec<String>,
}

pub fn split_fn(opts: &SplitFnOptions) -> Result<SplitFnResult, SplitFnError> {
    if opts.first_name.is_empty() || opts.second_name.is_empty() {
        return Err(SplitFnError::BadNames);
    }
    let (src, ast) = load_and_parse(&opts.file)?;
    let item = find_fn_in_file(&ast, &opts.fn_name)
        .ok_or_else(|| SplitFnError::NotFound(opts.fn_name.clone(), opts.file.clone()))?;

    // Statements at top level of the fn block.
    let stmts = &item.block.stmts;
    if stmts.is_empty() {
        return Err(SplitFnError::EmptyHalf);
    }

    // Find split index: first stmt whose start line >= at_line.
    let split_idx = stmts
        .iter()
        .position(|s| s.span().start().line >= opts.at_line)
        .ok_or_else(|| {
            SplitFnError::NotAtStatementBoundary(
                opts.at_line,
                opts.file.display().to_string(),
            )
        })?;

    if split_idx == 0 || split_idx >= stmts.len() {
        return Err(SplitFnError::EmptyHalf);
    }

    let actual_line = stmts[split_idx].span().start().line;
    if actual_line != opts.at_line {
        return Err(SplitFnError::NotAtStatementBoundary(
            opts.at_line,
            opts.file.display().to_string(),
        ));
    }

    let (before, after) = stmts.split_at(split_idx);

    // Identify bindings introduced in `before` and read in `after`.
    let mut bindings_before: Vec<String> = Vec::new();
    for s in before {
        collect_let_idents(s, &mut bindings_before);
    }
    let mut used_after = IdentCollector::default();
    for s in after {
        used_after.visit_stmt(s);
    }
    let intermediates: Vec<String> = bindings_before
        .iter()
        .filter(|b| used_after.idents.contains(*b))
        .cloned()
        .collect();

    // Render new fns. We deliberately use `i32` as placeholder types
    // and let the user re-type after the split runs (matches the
    // simplification used in extract-fn v1).
    let body_indent = "    ";
    let offs = line_offsets(&src);
    let before_text = stmts_text(&src, &offs, before);
    let after_text = stmts_text(&src, &offs, after);

    let first_ret_type = match intermediates.len() {
        0 => String::new(),
        1 => " -> i32".to_string(),
        n => format!(" -> ({})", vec!["i32"; n].join(", ")),
    };
    let first_tail = match intermediates.len() {
        0 => String::new(),
        1 => format!("{body_indent}{}\n", intermediates[0]),
        _ => format!("{body_indent}({})\n", intermediates.join(", ")),
    };
    let first_fn = format!(
        "fn {name}(){ret} {{\n{body}{tail}}}\n",
        name = opts.first_name,
        ret = first_ret_type,
        body = before_text,
        tail = first_tail,
    );

    let second_params: String = intermediates
        .iter()
        .map(|n| format!("{n}: i32"))
        .collect::<Vec<_>>()
        .join(", ");
    let second_fn = format!(
        "fn {name}({params}) {{\n{body}}}\n",
        name = opts.second_name,
        params = second_params,
        body = after_text,
    );

    // Build the new body for the original fn: chain `first` → `second`.
    let chain = match intermediates.len() {
        0 => format!("{body_indent}{}();\n{body_indent}{}();\n", opts.first_name, opts.second_name),
        1 => format!(
            "{body_indent}let {var} = {first}();\n{body_indent}{second}({var});\n",
            var = intermediates[0],
            first = opts.first_name,
            second = opts.second_name
        ),
        _ => format!(
            "{body_indent}let ({vars}) = {first}();\n{body_indent}{second}({vars});\n",
            vars = intermediates.join(", "),
            first = opts.first_name,
            second = opts.second_name
        ),
    };

    // Re-render the original fn: keep the signature, replace the body.
    let original_text = src[fn_span_bytes(&src, item).0..fn_span_bytes(&src, item).1].to_string();
    let new_original = render_fn_with_new_body(&original_text, &chain);

    // Splice everything back into the file. Layout:
    //   <preamble>
    //   <new original fn>
    //   <first_fn>
    //   <second_fn>
    //   <rest>
    let (fn_start, fn_end, _) = fn_span_bytes(&src, item);
    let mut out = String::with_capacity(src.len() + first_fn.len() + second_fn.len());
    out.push_str(&src[..fn_start]);
    out.push_str(&new_original);
    if !new_original.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
    out.push_str(&first_fn);
    out.push('\n');
    out.push_str(&second_fn);
    out.push_str(&src[fn_end..]);

    Ok(SplitFnResult {
        edits: vec![FileEdit {
            path: opts.file.clone(),
            new_contents: out,
        }],
        intermediates,
    })
}

pub fn apply(edits: &[FileEdit]) -> anyhow::Result<usize> {
    apply_edits(edits).map_err(|e| anyhow::anyhow!("write failed: {e}"))
}

fn collect_let_idents(stmt: &syn::Stmt, out: &mut Vec<String>) {
    if let syn::Stmt::Local(l) = stmt {
        collect_pat_idents(&l.pat, out);
    }
}

fn collect_pat_idents(pat: &syn::Pat, out: &mut Vec<String>) {
    match pat {
        syn::Pat::Ident(pi) => {
            out.push(pi.ident.to_string());
        }
        syn::Pat::Tuple(t) => {
            for p in &t.elems {
                collect_pat_idents(p, out);
            }
        }
        syn::Pat::TupleStruct(t) => {
            for p in &t.elems {
                collect_pat_idents(p, out);
            }
        }
        _ => {}
    }
}

#[derive(Default)]
struct IdentCollector {
    idents: std::collections::HashSet<String>,
}

impl<'ast> Visit<'ast> for IdentCollector {
    fn visit_ident(&mut self, i: &'ast syn::Ident) {
        self.idents.insert(i.to_string());
    }
}

/// Slice the source text covering a contiguous run of statements,
/// using their line spans. Returns the raw text including leading
/// indentation and trailing newline of each line.
fn stmts_text(src: &str, offs: &[usize], stmts: &[syn::Stmt]) -> String {
    if stmts.is_empty() {
        return String::new();
    }
    let first = stmts.first().unwrap().span().start().line;
    let last = stmts.last().unwrap().span().end().line;
    let start_byte = offs[first - 1];
    let end_byte = if last < offs.len() {
        offs[last]
    } else {
        src.len()
    };
    src[start_byte..end_byte].to_string()
}

/// Replace the body of a fn item's text with a new body. The
/// signature (everything up to and including the first `{`) is
/// preserved; everything between the matching braces is replaced.
fn render_fn_with_new_body(fn_text: &str, new_body: &str) -> String {
    let bytes = fn_text.as_bytes();
    let mut open: Option<usize> = None;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'{' {
            open = Some(i);
            break;
        }
    }
    let Some(open) = open else {
        return fn_text.to_string();
    };
    // Find matching close.
    let mut depth = 0i32;
    let mut close: Option<usize> = None;
    let mut i = open;
    let mut in_str = false;
    let mut escape = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_str {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }
    let Some(close) = close else {
        return fn_text.to_string();
    };
    let mut out = String::with_capacity(fn_text.len());
    out.push_str(&fn_text[..=open]);
    out.push('\n');
    out.push_str(new_body);
    out.push_str(&fn_text[close..]);
    out
}
