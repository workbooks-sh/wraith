//! `wraith refactor inline` — replace every call site with the fn
//! body, with parameter→argument substitution, then delete the
//! original fn. v1 scope (wb-5lgj.31):
//!
//! - refuses generic fns (need monomorphization)
//! - refuses fns whose body uses `return` other than as a tail expression
//! - refuses recursive fns
//! - refuses fns with `?` / `.await` / `self` references
//! - refuses fns with `loop` / `while` / `break` / `continue`
//! - parameter names must be plain `ident: Ty` patterns
//!
//! Each call site is rewritten as `(<body with params substituted>)`
//! so the inlined block remains a single expression.

use std::path::PathBuf;

use syn::visit::Visit;

use crate::refactor_shared::{
    apply_edits, file_mentions_token, find_fn_in_file, fn_span_bytes, load_and_parse, rename_idents,
    workspace_rs_files, FileEdit,
};
use crate::workspace::Workspace;

#[derive(Debug, thiserror::Error)]
pub enum InlineError {
    #[error("function `{0}` not found in `{1}`")]
    NotFound(String, PathBuf),
    #[error("can't inline: {0}")]
    Unsupported(&'static str),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Clone)]
pub struct InlineOptions {
    pub file: PathBuf,
    pub fn_name: String,
}

#[derive(Debug, Clone)]
pub struct InlineResult {
    pub edits: Vec<FileEdit>,
    pub call_sites_replaced: usize,
}

pub fn inline(ws: &Workspace, opts: &InlineOptions) -> Result<InlineResult, InlineError> {
    let (src, ast) = load_and_parse(&opts.file)?;
    let item = find_fn_in_file(&ast, &opts.fn_name)
        .ok_or_else(|| InlineError::NotFound(opts.fn_name.clone(), opts.file.clone()))?;

    if !item.sig.generics.params.is_empty() {
        return Err(InlineError::Unsupported("fn is generic"));
    }
    if scan_for_refusal(item, &opts.fn_name) {
        return Err(InlineError::Unsupported(
            "body uses return / ? / await / self / loop / break / continue or is recursive",
        ));
    }

    let mut params: Vec<String> = Vec::new();
    for input in &item.sig.inputs {
        match input {
            syn::FnArg::Receiver(_) => {
                return Err(InlineError::Unsupported("fn takes a receiver"));
            }
            syn::FnArg::Typed(pt) => {
                let syn::Pat::Ident(pi) = &*pt.pat else {
                    return Err(InlineError::Unsupported("non-ident parameter pattern"));
                };
                params.push(pi.ident.to_string());
            }
        }
    }

    let body_text = body_inline_text(item, &src);

    let files = workspace_rs_files(ws);
    let mut edits: Vec<FileEdit> = Vec::new();
    let mut total_calls = 0usize;
    for file in &files {
        let Ok((file_src, _)) = load_and_parse(file) else {
            continue;
        };
        if !file_mentions_token(&file_src, &opts.fn_name) {
            continue;
        }
        let (mut new_src, n) = rewrite_calls_in_file(&file_src, &opts.fn_name, &params, &body_text);
        total_calls += n;
        if file == &opts.file {
            new_src = delete_fn_def(&new_src, &opts.fn_name)?;
        }
        if new_src != file_src {
            edits.push(FileEdit {
                path: file.clone(),
                new_contents: new_src,
            });
        }
    }

    Ok(InlineResult {
        edits,
        call_sites_replaced: total_calls,
    })
}

pub fn apply(edits: &[FileEdit]) -> anyhow::Result<usize> {
    apply_edits(edits).map_err(|e| anyhow::anyhow!("write failed: {e}"))
}

fn body_inline_text(item: &syn::ItemFn, src: &str) -> String {
    use syn::spanned::Spanned;
    let body_span = item.block.span();
    let start = body_span.start();
    let end = body_span.end();
    let offs = crate::refactor_shared::line_offsets(src);
    let start_byte = offs[start.line - 1] + start.column;
    let end_byte = if end.line - 1 < offs.len() {
        offs[end.line - 1] + end.column
    } else {
        src.len()
    };
    let block_text = src[start_byte..end_byte.min(src.len())].to_string();
    let inner = block_text
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .trim()
        .to_string();
    if item.block.stmts.len() == 1 {
        if let syn::Stmt::Expr(_, None) = &item.block.stmts[0] {
            return inner;
        }
    }
    format!("{{ {} }}", inner)
}

fn scan_for_refusal(item: &syn::ItemFn, self_name: &str) -> bool {
    struct V<'a> {
        bad: bool,
        self_name: &'a str,
    }
    impl<'ast> Visit<'ast> for V<'_> {
        fn visit_expr_return(&mut self, _: &'ast syn::ExprReturn) {
            self.bad = true;
        }
        fn visit_expr_try(&mut self, _: &'ast syn::ExprTry) {
            self.bad = true;
        }
        fn visit_expr_await(&mut self, _: &'ast syn::ExprAwait) {
            self.bad = true;
        }
        fn visit_expr_loop(&mut self, _: &'ast syn::ExprLoop) {
            self.bad = true;
        }
        fn visit_expr_while(&mut self, _: &'ast syn::ExprWhile) {
            self.bad = true;
        }
        fn visit_expr_break(&mut self, _: &'ast syn::ExprBreak) {
            self.bad = true;
        }
        fn visit_expr_continue(&mut self, _: &'ast syn::ExprContinue) {
            self.bad = true;
        }
        fn visit_expr_path(&mut self, p: &'ast syn::ExprPath) {
            if p.path.is_ident("self") || p.path.is_ident("Self") {
                self.bad = true;
                return;
            }
            if p.path.is_ident(self.self_name) {
                self.bad = true;
                return;
            }
            syn::visit::visit_expr_path(self, p);
        }
    }
    let mut v = V { bad: false, self_name };
    v.visit_block(&item.block);
    v.bad
}

fn rewrite_calls_in_file(
    src: &str,
    fn_name: &str,
    params: &[String],
    body_inline: &str,
) -> (String, usize) {
    let mut out = String::with_capacity(src.len());
    let mut count = 0usize;
    let bytes = src.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        let prev = if i > 0 { bytes[i - 1] } else { 0u8 };
        if is_ident_start(b)
            && !is_ident_continue(prev)
            && match_ident_at(bytes, i, fn_name)
        {
            // Skip whitespace and find `(`.
            let after = i + fn_name.len();
            let mut j = after;
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'(' {
                if let Some((args, end_paren)) = parse_call_args(src, j) {
                    if args.len() == params.len() {
                        let mut replaced = body_inline.to_string();
                        for (p, a) in params.iter().zip(args.iter()) {
                            replaced = rename_idents(&replaced, p, &format!("({})", a.trim()));
                        }
                        out.push('(');
                        out.push_str(&replaced);
                        out.push(')');
                        count += 1;
                        i = end_paren + 1;
                        continue;
                    }
                }
            }
        }
        let ch_len = utf8_char_len(b);
        out.push_str(&src[i..i + ch_len]);
        i += ch_len;
    }
    (out, count)
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}
fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn match_ident_at(bytes: &[u8], i: usize, name: &str) -> bool {
    let nb = name.as_bytes();
    if i + nb.len() > bytes.len() {
        return false;
    }
    if &bytes[i..i + nb.len()] != nb {
        return false;
    }
    if let Some(&next) = bytes.get(i + nb.len()) {
        if is_ident_continue(next) {
            return false;
        }
    }
    true
}

fn utf8_char_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b < 0xC0 {
        1
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    }
}

fn parse_call_args(src: &str, lparen: usize) -> Option<(Vec<String>, usize)> {
    let bytes = src.as_bytes();
    if bytes.get(lparen)? != &b'(' {
        return None;
    }
    let mut depth = 0i32;
    let mut args: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut i = lparen;
    let mut in_str = false;
    let mut escape = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_str {
            cur.push(b as char);
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
            b'(' => {
                depth += 1;
                if depth > 1 {
                    cur.push('(');
                }
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    let trimmed = cur.trim().to_string();
                    if !trimmed.is_empty() || !args.is_empty() {
                        args.push(trimmed);
                    }
                    return Some((args, i));
                } else {
                    cur.push(')');
                }
            }
            b',' if depth == 1 => {
                args.push(cur.trim().to_string());
                cur.clear();
            }
            b'"' => {
                in_str = true;
                cur.push('"');
            }
            _ => {
                let ch_len = utf8_char_len(b);
                if ch_len == 1 {
                    cur.push(b as char);
                } else {
                    cur.push_str(&src[i..i + ch_len]);
                    i += ch_len;
                    continue;
                }
            }
        }
        i += 1;
    }
    None
}

pub fn delete_fn_def(src: &str, fn_name: &str) -> Result<String, InlineError> {
    let ast = syn::parse_file(src)
        .map_err(|e| InlineError::Other(anyhow::anyhow!("parse failed: {e}")))?;
    let item = find_fn_in_file(&ast, fn_name).ok_or_else(|| {
        InlineError::NotFound(fn_name.to_string(), PathBuf::from("<in-memory>"))
    })?;
    let (start, end, _) = fn_span_bytes(src, item);
    let mut out = String::with_capacity(src.len());
    out.push_str(&src[..start]);
    out.push_str(&src[end..]);
    Ok(out)
}
