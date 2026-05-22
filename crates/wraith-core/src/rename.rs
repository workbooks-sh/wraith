//! `wraith refactor rename` — workspace-wide rename of a single
//! symbol (fn, struct, const, mod). v1 covers free-standing symbols;
//! trait methods are refused.
//!
//! The agent says WHAT to rename; we do the find + replace and ensure
//! collisions don't silently break the build.

use std::path::PathBuf;

use crate::refactor_shared::{
    apply_edits, file_mentions_token, load_and_parse, rename_idents, workspace_rs_files, FileEdit,
};
use crate::workspace::Workspace;

#[derive(Debug, thiserror::Error)]
pub enum RenameError {
    #[error("symbol `{0}` not found in any workspace crate")]
    NotFound(String),
    #[error("rename collides with existing symbol `{0}` in the same module")]
    Collision(String),
    #[error("trait methods aren't supported in v1; see wb-5lgj.31 follow-up")]
    TraitMethod,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Clone)]
pub struct RenameOptions {
    pub symbol: String,
    pub new_name: String,
}

#[derive(Debug, Clone)]
pub struct RenameResult {
    pub leaf_name: String,
    pub edits: Vec<FileEdit>,
    pub files_touched: usize,
    pub renames_applied: usize,
}

pub fn rename(ws: &Workspace, opts: &RenameOptions) -> Result<RenameResult, RenameError> {
    let leaf = opts
        .symbol
        .rsplit("::")
        .next()
        .unwrap_or(&opts.symbol)
        .to_string();
    if leaf == opts.new_name {
        return Err(RenameError::Other(anyhow::anyhow!(
            "new name is identical to current name"
        )));
    }
    if !is_valid_ident(&opts.new_name) {
        return Err(RenameError::Other(anyhow::anyhow!(
            "`{}` is not a valid Rust identifier",
            opts.new_name
        )));
    }

    let files = workspace_rs_files(ws);
    let mut found_anywhere = false;
    let mut collision = false;
    let mut found_as_trait_method = false;
    let mut edits: Vec<FileEdit> = Vec::new();
    let mut total_renames = 0usize;

    for file in &files {
        let Ok((src, ast)) = load_and_parse(file) else {
            continue;
        };
        let scan = scan_file(&ast, &leaf, &opts.new_name);
        if scan.def_found || scan.referenced {
            found_anywhere = true;
        }
        if scan.collides {
            collision = true;
        }
        if scan.trait_method {
            found_as_trait_method = true;
        }
        if !file_mentions_token(&src, &leaf) {
            continue;
        }
        let new_src = rename_idents(&src, &leaf, &opts.new_name);
        if new_src != src {
            let renames_in_this_file = count_token_occurrences(&src, &leaf);
            total_renames += renames_in_this_file;
            edits.push(FileEdit {
                path: file.clone(),
                new_contents: new_src,
            });
        }
    }

    if !found_anywhere {
        return Err(RenameError::NotFound(opts.symbol.clone()));
    }
    if found_as_trait_method {
        return Err(RenameError::TraitMethod);
    }
    if collision {
        return Err(RenameError::Collision(opts.new_name.clone()));
    }

    let files_touched = edits.len();
    Ok(RenameResult {
        leaf_name: leaf,
        edits,
        files_touched,
        renames_applied: total_renames,
    })
}

pub fn apply(edits: &[FileEdit]) -> anyhow::Result<usize> {
    apply_edits(edits).map_err(|e| anyhow::anyhow!("failed to write edits: {e}"))
}

struct Scan {
    def_found: bool,
    referenced: bool,
    collides: bool,
    trait_method: bool,
}

fn scan_file(file: &syn::File, leaf: &str, new_name: &str) -> Scan {
    let mut s = Scan {
        def_found: false,
        referenced: false,
        collides: false,
        trait_method: false,
    };
    scan_items(&file.items, leaf, new_name, &mut s);
    s
}

fn scan_items(items: &[syn::Item], leaf: &str, new_name: &str, s: &mut Scan) {
    for it in items {
        match it {
            syn::Item::Fn(f) => {
                if f.sig.ident == leaf {
                    s.def_found = true;
                }
                if f.sig.ident == new_name {
                    s.collides = true;
                }
            }
            syn::Item::Struct(it) => {
                if it.ident == leaf {
                    s.def_found = true;
                }
                if it.ident == new_name {
                    s.collides = true;
                }
            }
            syn::Item::Const(it) => {
                if it.ident == leaf {
                    s.def_found = true;
                }
                if it.ident == new_name {
                    s.collides = true;
                }
            }
            syn::Item::Mod(m) => {
                if m.ident == leaf {
                    s.def_found = true;
                }
                if m.ident == new_name {
                    s.collides = true;
                }
                if let Some((_, sub)) = &m.content {
                    scan_items(sub, leaf, new_name, s);
                }
            }
            syn::Item::Trait(t) => {
                for tit in &t.items {
                    if let syn::TraitItem::Fn(f) = tit {
                        if f.sig.ident == leaf {
                            s.trait_method = true;
                        }
                    }
                }
            }
            syn::Item::Impl(im) => {
                // impl blocks: a fn with the same name on a struct is
                // OK to rename (free-standing inherent method), but if
                // this impl is an impl-of-trait we must refuse.
                let is_trait_impl = im.trait_.is_some();
                for ii in &im.items {
                    if let syn::ImplItem::Fn(f) = ii {
                        if f.sig.ident == leaf && is_trait_impl {
                            s.trait_method = true;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn is_valid_ident(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    for c in chars {
        if !(c.is_ascii_alphanumeric() || c == '_') {
            return false;
        }
    }
    // Bar a few reserved words to keep things safe.
    !matches!(
        s,
        "fn" | "mod"
            | "struct"
            | "enum"
            | "impl"
            | "trait"
            | "let"
            | "if"
            | "else"
            | "match"
            | "loop"
            | "for"
            | "while"
            | "return"
            | "break"
            | "continue"
            | "self"
            | "Self"
            | "super"
            | "crate"
            | "use"
            | "pub"
            | "as"
            | "ref"
            | "mut"
            | "const"
            | "static"
            | "type"
            | "where"
            | "move"
            | "dyn"
            | "async"
            | "await"
            | "in"
            | "true"
            | "false"
    )
}

fn count_token_occurrences(src: &str, name: &str) -> usize {
    let mut count = 0;
    let mut cur = String::new();
    let mut in_ident = false;
    for c in src.chars() {
        let is_id = c.is_ascii_alphanumeric() || c == '_';
        if is_id != in_ident {
            if in_ident && cur == name {
                count += 1;
            }
            cur.clear();
            in_ident = is_id;
        }
        cur.push(c);
    }
    if in_ident && cur == name {
        count += 1;
    }
    count
}

// Silence unused warning since PathBuf isn't actually constructed here.
#[allow(dead_code)]
fn _unused_pathbuf() -> Option<PathBuf> {
    None
}
