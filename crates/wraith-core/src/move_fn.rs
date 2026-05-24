//! `wraith refactor move-fn` — move a fn from one location to another,
//! creating the destination module if needed, and rewriting all
//! workspace-wide `use` paths + fully-qualified call sites.
//!
//! Scope (wb-5lgj.31): a fn defined in a single source file, moved to
//! another module inside the same workspace. Cross-crate moves
//! require `--cross-crate=allow`.
//!
//! wb-5lgj.43 extends this to fully wire up the move so the result
//! compiles on the first try:
//! - intermediate `pub mod` chain registered all the way down
//! - std prelude `use` statements injected at the top of the new file
//! - `<dest-crate>::X` paths inside the moved body rewritten to `crate::X`
//! - callers in the same package but a different target (e.g. `bin/`
//!   when the fn lives in the lib) get a `use` line injected
//! - visibility lifted to `pub` when any caller is in a different
//!   target (bin/lib distinction) or another crate
//! - doc-comment placeholder scaffolded into the moved fn + the new
//!   `pub mod` declaration when the destination crate uses
//!   `#![deny(missing_docs)]`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use syn::visit::Visit;

use crate::refactor_shared::{
    apply_edits, file_mentions_token, find_fn_in_file, fn_span_bytes, load_and_parse,
    workspace_rs_files, FileEdit,
};
use crate::workspace::{CrateInfo, Workspace};

#[derive(Debug, thiserror::Error)]
pub enum MoveFnError {
    #[error("function `{0}` not found in `{1}`")]
    NotFound(String, PathBuf),
    #[error("destination crate `{0}` not found in the workspace")]
    DestCrateNotFound(String),
    #[error("cross-crate move requires --cross-crate=allow (src crate: `{src}`, dst crate: `{dst}`)")]
    CrossCrateDenied { src: String, dst: String },
    #[error("destination already defines a `{0}` in module `{1}`")]
    DestCollision(String, String),
    /// wb-5lgj.44: moving a fn out of a bin into a lib would leave the
    /// moved fn referencing sibling private fns that are inaccessible
    /// from the lib (bin items aren't importable). Refuse with the
    /// list so the caller can extract those sibling fns to the lib
    /// first, then re-run the move.
    #[error("cannot move `{fn_name}` from bin `{bin_file}` into lib — it calls bin-private functions that aren't accessible from the lib: {deps:?}. Move (or elevate to pub + relocate) these sibling fns into the lib first, then re-run.")]
    BinPrivateDeps {
        /// The fn being moved.
        fn_name: String,
        /// Path to the bin file the fn is in.
        bin_file: PathBuf,
        /// Names of sibling private fns the moved body calls.
        deps: Vec<String>,
    },
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Clone)]
pub struct MoveFnOptions {
    pub src_file: PathBuf,
    pub fn_name: String,
    /// e.g. `crate_b::shared` or `crate_a::sub`.
    pub dst_module_path: String,
    pub allow_cross_crate: bool,
}

#[derive(Debug, Clone)]
pub struct MoveFnResult {
    pub edits: Vec<FileEdit>,
    pub created_files: Vec<PathBuf>,
    pub dst_file: PathBuf,
    pub src_crate: String,
    pub dst_crate: String,
    /// Human-readable notices to print (e.g. visibility elevation).
    pub notices: Vec<String>,
}

pub fn move_fn(ws: &Workspace, opts: &MoveFnOptions) -> Result<MoveFnResult, MoveFnError> {
    let (src_text, src_ast) = load_and_parse(&opts.src_file)?;
    let item = find_fn_in_file(&src_ast, &opts.fn_name)
        .ok_or_else(|| MoveFnError::NotFound(opts.fn_name.clone(), opts.src_file.clone()))?;
    let (start, end, _line) = fn_span_bytes(&src_text, item);
    let fn_text = src_text[start..end].to_string();

    // Resolve src crate from path.
    let src_crate = crate_owning_file(ws, &opts.src_file)
        .ok_or_else(|| MoveFnError::Other(anyhow::anyhow!(
            "could not determine src crate for `{}`",
            opts.src_file.display()
        )))?;

    // Parse dst path: "<crate>[::<seg>...]".
    let mut parts: Vec<&str> = opts.dst_module_path.split("::").collect();
    if parts.is_empty() {
        return Err(MoveFnError::Other(anyhow::anyhow!(
            "empty --to module path"
        )));
    }
    let dst_crate_name = parts.remove(0).to_string();
    let dst_module_segments: Vec<String> = parts.iter().map(|s| s.to_string()).collect();

    let dst_crate = ws
        .crates
        .iter()
        .find(|c| c.name.replace('-', "_") == dst_crate_name || c.name == dst_crate_name)
        .ok_or_else(|| MoveFnError::DestCrateNotFound(dst_crate_name.clone()))?;

    if src_crate.name != dst_crate.name && !opts.allow_cross_crate {
        return Err(MoveFnError::CrossCrateDenied {
            src: src_crate.name.clone(),
            dst: dst_crate.name.clone(),
        });
    }

    let dst_crate_key = dst_crate.name.replace('-', "_");

    let lib_root = lib_root_file(dst_crate).ok_or_else(|| {
        MoveFnError::Other(anyhow::anyhow!(
            "destination crate `{}` has no lib.rs",
            dst_crate.name
        ))
    })?;

    // Identify the source target (lib root file vs bin file).
    let src_target_root = target_root_for_file(src_crate, &opts.src_file);
    let dst_target_root = lib_root.clone();
    let src_is_bin = src_target_root
        .as_ref()
        .map(|p| is_bin_path(p))
        .unwrap_or(false);
    let dst_is_lib = lib_root
        .file_name()
        .and_then(|s| s.to_str())
        .map(|n| n == "lib.rs")
        .unwrap_or(false);
    // For visibility elevation: caller in bin + fn in lib (or different
    // crate) counts as "crosses a target boundary".
    let crosses_target_boundary = src_crate.name != dst_crate.name
        || (src_is_bin && dst_is_lib && src_target_root.as_deref() != Some(&dst_target_root));

    // wb-5lgj.44: when moving from bin → lib, the moved fn cannot call
    // sibling private fns defined in the same bin file — bin items
    // aren't reachable from the lib (no `use crate::bin::X` works). Walk
    // the body, collect free identifier calls, intersect with same-file
    // private fns. If any survive, refuse with the list so the caller
    // can extract them to the lib first.
    if src_is_bin && dst_is_lib {
        let deps = collect_bin_private_deps(item, &src_ast);
        if !deps.is_empty() {
            return Err(MoveFnError::BinPrivateDeps {
                fn_name: opts.fn_name.clone(),
                bin_file: opts.src_file.clone(),
                deps,
            });
        }
    }

    let mut edits: Vec<FileEdit> = Vec::new();
    let mut created_files: Vec<PathBuf> = Vec::new();
    let mut notices: Vec<String> = Vec::new();

    // Detect missing_docs strictness on the destination crate. Look at
    // the crate root (lib.rs).
    let strict_docs = lib_root_has_deny_missing_docs(&lib_root);

    // Compute dst file path.
    let dst_file = compute_dst_file(&lib_root, &dst_module_segments);

    // Ensure dst file + chain of `mod` declarations.
    let already_existed = dst_file.exists();
    let dst_text_existing = if already_existed {
        std::fs::read_to_string(&dst_file).map_err(|e| {
            MoveFnError::Other(anyhow::anyhow!(
                "failed to read {}: {e}",
                dst_file.display()
            ))
        })?
    } else {
        String::new()
    };

    // Collision check in dst.
    if !dst_text_existing.is_empty() {
        if let Ok(parsed) = syn::parse_file(&dst_text_existing) {
            if find_fn_in_file(&parsed, &opts.fn_name).is_some() {
                return Err(MoveFnError::DestCollision(
                    opts.fn_name.clone(),
                    opts.dst_module_path.clone(),
                ));
            }
        }
    }

    // Rewrite `<dst_crate>::` → `crate::` inside the moved fn body.
    let rewritten_body = rewrite_crate_paths(&fn_text, &dst_crate_key);

    // Collect std prelude identifiers referenced in the body.
    let std_uses = collect_std_uses(&rewritten_body);

    // Visibility: ensure the moved fn is at least pub(crate) so call
    // sites still work. If callers cross a target boundary, elevate to
    // pub. Always doc-scaffold if the dst crate enforces missing_docs.
    let elevated_fn_text = ensure_visibility(&rewritten_body, crosses_target_boundary);
    if crosses_target_boundary {
        notices.push(format!(
            "wraith: elevated `{}` to `pub` (caller crosses a target boundary)",
            opts.fn_name
        ));
    }
    let docced_fn_text = if strict_docs {
        scaffold_doc_comment(&elevated_fn_text)
    } else {
        elevated_fn_text
    };

    // Compose dst file contents (only when newly created we prepend
    // std uses; if it already exists we trust the existing imports and
    // leave them alone).
    let dst_new_contents = if already_existed {
        append_fn(&dst_text_existing, &docced_fn_text)
    } else {
        let mut head = String::new();
        if !std_uses.is_empty() {
            for line in std_uses {
                head.push_str(&line);
                head.push('\n');
            }
            head.push('\n');
        }
        append_fn(&head, &docced_fn_text)
    };

    edits.push(FileEdit {
        path: dst_file.clone(),
        new_contents: dst_new_contents,
    });

    // Remove fn from src file FIRST so the source-removed content is in
    // `edits` when we register the module chain (which merges onto the
    // current `edits` view of any file it touches — e.g. lib.rs may be
    // both the src file AND the parent that gets `pub mod` added).
    let mut new_src = String::with_capacity(src_text.len());
    new_src.push_str(&src_text[..start]);
    new_src.push_str(&src_text[end..]);
    edits.push(FileEdit {
        path: opts.src_file.clone(),
        new_contents: new_src,
    });

    if !already_existed {
        created_files.push(dst_file.clone());

        // Register the full chain of `pub mod` declarations all the way
        // up to lib.rs, creating any intermediate `mod.rs` files.
        register_module_chain(
            &lib_root,
            &dst_module_segments,
            strict_docs,
            &mut edits,
            &mut created_files,
        );
    }

    // Compute fully-qualified path for the destination.
    let dst_qualified = if dst_module_segments.is_empty() {
        format!("{}::{}", dst_crate_key, opts.fn_name)
    } else {
        format!(
            "{}::{}::{}",
            dst_crate_key,
            dst_module_segments.join("::"),
            opts.fn_name
        )
    };

    // Rewrite `use` paths and fully qualified call sites across the workspace.
    let files = workspace_rs_files(ws);
    for file in &files {
        if file == &opts.src_file || file == &dst_file {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        if !file_mentions_token(&text, &opts.fn_name) {
            continue;
        }
        let updated = rewrite_use_paths(
            &text,
            &src_crate.name.replace('-', "_"),
            &opts.fn_name,
            &dst_qualified,
        );
        if updated != text {
            merge_edit(&mut edits, file.clone(), updated);
        }
    }

    // Inject `use` lines into every same-crate caller that still
    // references the fn unqualified — they were relying on the fn
    // being in scope in its old location, but it's gone now.
    inject_caller_uses(
        ws,
        &opts.src_file,
        &dst_file,
        src_crate,
        &dst_crate_key,
        &opts.fn_name,
        &dst_module_segments,
        crosses_target_boundary,
        &mut edits,
    );

    Ok(MoveFnResult {
        edits,
        created_files,
        dst_file,
        src_crate: src_crate.name.clone(),
        dst_crate: dst_crate.name.clone(),
        notices,
    })
}

pub fn apply(edits: &[FileEdit]) -> anyhow::Result<usize> {
    apply_edits(edits).map_err(|e| anyhow::anyhow!("write failed: {e}"))
}

fn crate_owning_file<'a>(ws: &'a Workspace, file: &Path) -> Option<&'a CrateInfo> {
    let canon = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    for c in &ws.crates {
        let croot = c.root_dir.canonicalize().unwrap_or_else(|_| c.root_dir.clone());
        if canon.starts_with(&croot) {
            return Some(c);
        }
    }
    None
}

fn lib_root_file(c: &CrateInfo) -> Option<PathBuf> {
    c.src_paths
        .iter()
        .find(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .map(|n| n == "lib.rs")
                .unwrap_or(false)
        })
        .cloned()
        .or_else(|| {
            c.src_paths
                .iter()
                .find(|p| {
                    p.file_name()
                        .and_then(|s| s.to_str())
                        .map(|n| n == "main.rs")
                        .unwrap_or(false)
                })
                .cloned()
        })
        .or_else(|| c.src_paths.first().cloned())
}

/// Return the target root file (lib.rs / main.rs / bin/<x>.rs) that
/// `file` belongs to. Heuristic: walk up looking for one of the crate's
/// declared `src_paths`.
fn target_root_for_file(c: &CrateInfo, file: &Path) -> Option<PathBuf> {
    let canon = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let mut best: Option<PathBuf> = None;
    let mut best_len = 0usize;
    for s in &c.src_paths {
        let sc = s.canonicalize().unwrap_or_else(|_| s.clone());
        if let Some(parent) = sc.parent() {
            if canon.starts_with(parent) {
                let l = parent.as_os_str().len();
                if l >= best_len {
                    best_len = l;
                    best = Some(sc.clone());
                }
            }
        }
    }
    best
}

fn is_bin_path(p: &Path) -> bool {
    // Either `src/bin/<name>.rs` or `src/main.rs`.
    let s = p.to_string_lossy();
    s.contains("/bin/")
        || p.file_name()
            .and_then(|n| n.to_str())
            .map(|n| n == "main.rs")
            .unwrap_or(false)
}

fn compute_dst_file(lib_root: &Path, segs: &[String]) -> PathBuf {
    if segs.is_empty() {
        return lib_root.to_path_buf();
    }
    let src_dir = lib_root.parent().unwrap().to_path_buf();
    let mut p = src_dir;
    for seg in segs {
        p.push(seg);
    }
    p.set_extension("rs");
    p
}

/// Walk every segment of `<a>::<b>::<c>` and make sure:
/// 1. lib.rs has `pub mod a;`
/// 2. `src/a/mod.rs` exists and contains `pub mod b;`
/// 3. `src/a/b/mod.rs` exists and contains `pub mod c;`
///
/// For each created `mod.rs`, also create the directory and (when
/// `strict_docs`) emit a placeholder doc comment.
fn register_module_chain(
    lib_root: &Path,
    segs: &[String],
    strict_docs: bool,
    edits: &mut Vec<FileEdit>,
    created_files: &mut Vec<PathBuf>,
) {
    if segs.is_empty() {
        return;
    }
    let src_dir = lib_root.parent().unwrap().to_path_buf();

    for i in 0..segs.len() {
        let seg = &segs[i];
        let parent_file: PathBuf = if i == 0 {
            lib_root.to_path_buf()
        } else {
            // src/<seg1>/.../<seg_{i-1}>/mod.rs
            let mut p = src_dir.clone();
            for s in &segs[..i] {
                p.push(s);
            }
            p.push("mod.rs");
            p
        };

        // Make sure parent_file declares `pub mod <seg>;`.
        // If parent is a mod.rs and it doesn't exist yet, create it.
        let existed = parent_file.exists();
        let mut parent_text = if existed {
            // Maybe an earlier edit already targeted it — pull that.
            if let Some(prev) = find_edit(edits, &parent_file) {
                prev.new_contents.clone()
            } else {
                std::fs::read_to_string(&parent_file).unwrap_or_default()
            }
        } else {
            // Newly creating an intermediate mod.rs.
            if strict_docs {
                String::from("//! (auto-generated module placeholder)\n\n")
            } else {
                String::new()
            }
        };

        let needs = !parent_text
            .lines()
            .any(|l| l.trim() == format!("pub mod {};", seg) || l.trim() == format!("mod {};", seg));
        if needs {
            if !parent_text.is_empty() && !parent_text.ends_with('\n') {
                parent_text.push('\n');
            }
            if strict_docs {
                parent_text.push_str("/// (auto-generated placeholder)\n");
            }
            parent_text.push_str(&format!("pub mod {};\n", seg));
        }

        if existed {
            merge_edit(edits, parent_file.clone(), parent_text);
        } else {
            edits.push(FileEdit {
                path: parent_file.clone(),
                new_contents: parent_text,
            });
            created_files.push(parent_file.clone());
        }
    }
}

fn merge_edit(edits: &mut Vec<FileEdit>, path: PathBuf, new_contents: String) {
    let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
    if let Some(existing) = edits.iter_mut().find(|e| {
        e.path == path || e.path.canonicalize().unwrap_or_else(|_| e.path.clone()) == canon
    }) {
        existing.new_contents = new_contents;
    } else {
        edits.push(FileEdit { path, new_contents });
    }
}

fn find_edit<'a>(edits: &'a [FileEdit], path: &Path) -> Option<&'a FileEdit> {
    let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    edits.iter().find(|e| {
        e.path == path || e.path.canonicalize().unwrap_or_else(|_| e.path.clone()) == canon
    })
}

fn ensure_visibility(fn_text: &str, force_pub: bool) -> String {
    // If the fn already has a `pub` qualifier (`pub fn`, `pub(crate) fn`,
    // etc.), keep it (unless we need to bump pub(crate)→pub for cross-target).
    for line in fn_text.lines() {
        let t = line.trim_start();
        if t.starts_with("#[") || t.starts_with("///") || t.starts_with("//!") || t.is_empty() {
            continue;
        }
        if t.starts_with("pub(crate)") || t.starts_with("pub (crate)") {
            if force_pub {
                return fn_text.replacen("pub(crate)", "pub", 1);
            }
            return fn_text.to_string();
        }
        if t.starts_with("pub") {
            return fn_text.to_string();
        }
        // Private fn → at least pub(crate); pub if crossing target.
        let prefix = if force_pub { "pub " } else { "pub(crate) " };
        return prepend_before_fn(fn_text, prefix);
    }
    fn_text.to_string()
}

fn prepend_before_fn(fn_text: &str, prefix: &str) -> String {
    // Find the first non-attribute / non-doc line and prepend.
    let mut out = String::new();
    let mut inserted = false;
    for line in fn_text.lines() {
        let t = line.trim_start();
        if !inserted
            && !t.starts_with("#[")
            && !t.starts_with("///")
            && !t.starts_with("//!")
            && !t.is_empty()
        {
            // Insert the prefix before the first non-attr token on this line.
            let indent_len = line.len() - t.len();
            out.push_str(&line[..indent_len]);
            out.push_str(prefix);
            out.push_str(t);
            out.push('\n');
            inserted = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !inserted {
        out.push_str(prefix);
        out.push_str(fn_text);
    }
    out
}

fn scaffold_doc_comment(fn_text: &str) -> String {
    // Only add `/// (auto-generated placeholder)` if the fn doesn't
    // already have a doc comment.
    for line in fn_text.lines() {
        let t = line.trim_start();
        if t.starts_with("///") || t.starts_with("/**") {
            return fn_text.to_string();
        }
        if t.is_empty() || t.starts_with("//!") {
            continue;
        }
        // First real line — insert doc above it.
        let indent_len = line.len() - t.len();
        let indent = &line[..indent_len];
        let mut out = String::new();
        out.push_str(indent);
        out.push_str("/// (auto-generated placeholder)\n");
        out.push_str(fn_text);
        return out;
    }
    fn_text.to_string()
}

fn append_fn(existing: &str, fn_text: &str) -> String {
    let mut out = existing.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str(fn_text);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Update `use <src_crate>::<fn>;` lines to point at the dst location,
/// and rewrite fully-qualified call sites `<src_crate>::<fn>(...)` to
/// `<dst_qualified>(...)`.
fn rewrite_use_paths(src: &str, src_crate: &str, fn_name: &str, dst_qualified: &str) -> String {
    let from_use = format!("use {}::{};", src_crate, fn_name);
    let to_use = format!("use {};", dst_qualified);
    let mut out = src.replace(&from_use, &to_use);

    let from_call = format!("{}::{}", src_crate, fn_name);
    out = out.replace(&from_call, dst_qualified);
    out
}

fn lib_root_has_deny_missing_docs(lib_root: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(lib_root) else {
        return false;
    };
    text.lines()
        .any(|l| l.trim().contains("deny(missing_docs)"))
}

/// wb-5lgj.44: collect free-identifier calls inside `item`'s body that
/// resolve to a same-file non-pub `fn` in `src_ast`. These are the
/// sibling private fns that would become unresolved after a bin → lib
/// move. Result is sorted + deduplicated for stable error messages.
fn collect_bin_private_deps(item: &syn::ItemFn, src_ast: &syn::File) -> Vec<String> {
    // 1. Enumerate same-file non-pub fns (the candidate dep universe).
    let mut private_fn_names: BTreeSet<String> = BTreeSet::new();
    for f_item in &src_ast.items {
        if let syn::Item::Fn(other) = f_item {
            let is_pub = matches!(other.vis, syn::Visibility::Public(_));
            // The fn being moved doesn't count as its own dep.
            if !is_pub && other.sig.ident != item.sig.ident {
                private_fn_names.insert(other.sig.ident.to_string());
            }
        }
    }
    if private_fn_names.is_empty() {
        return Vec::new();
    }

    // 2. Walk the moved fn's body for single-segment Path expressions
    //    whose identifier matches a private fn name. Token-only scan
    //    (no real name resolution): conservative — flags any usage of
    //    `parse_region(...)` if a same-file private `parse_region`
    //    exists, even if the actual reference happens to resolve to
    //    something else. False positives are safe (caller adjusts);
    //    false negatives would be unsafe (silent broken move).
    struct Scan<'a> {
        candidates: &'a BTreeSet<String>,
        hits: BTreeSet<String>,
    }
    impl<'a, 'ast> syn::visit::Visit<'ast> for Scan<'a> {
        fn visit_expr_path(&mut self, p: &'ast syn::ExprPath) {
            if p.qself.is_none() && p.path.segments.len() == 1 {
                let name = p.path.segments[0].ident.to_string();
                if self.candidates.contains(&name) {
                    self.hits.insert(name);
                }
            }
            syn::visit::visit_expr_path(self, p);
        }
    }
    let mut scan = Scan {
        candidates: &private_fn_names,
        hits: BTreeSet::new(),
    };
    scan.visit_block(&item.block);

    scan.hits.into_iter().collect()
}

/// Rewrite `<crate_name>::X` → `crate::X` inside `fn_text`. Token-aware
/// on the leading segment: only matches when `<crate_name>` is followed
/// by `::`. Conservative — does not touch `<crate_name>::` strings
/// inside string literals (good enough — true literal handling would
/// require tokenizing the whole body).
fn rewrite_crate_paths(fn_text: &str, crate_name: &str) -> String {
    let pat = format!("{}::", crate_name);
    let mut out = String::with_capacity(fn_text.len());
    let bytes = fn_text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Check if at this position we have the pattern AND the
        // previous char is not part of an identifier (so we don't match
        // `mycrate` inside `not_mycrate`).
        if bytes[i..].starts_with(pat.as_bytes()) {
            let prev_ok = if i == 0 {
                true
            } else {
                let pc = bytes[i - 1];
                !(pc.is_ascii_alphanumeric() || pc == b'_')
            };
            if prev_ok {
                out.push_str("crate::");
                i += pat.len();
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Names from the Rust std prelude (2021) + a handful of very common
/// non-prelude std types. Each maps to its canonical `use std::...;`
/// path. The list is conservative; missing entries just mean the user
/// has to add the `use` themselves.
fn std_prelude_path(name: &str) -> Option<&'static str> {
    Some(match name {
        // std::path
        "Path" => "std::path::Path",
        "PathBuf" => "std::path::PathBuf",
        // std::process
        "ExitCode" => "std::process::ExitCode",
        "Command" => "std::process::Command",
        // std::collections
        "HashMap" => "std::collections::HashMap",
        "HashSet" => "std::collections::HashSet",
        "BTreeMap" => "std::collections::BTreeMap",
        "BTreeSet" => "std::collections::BTreeSet",
        "VecDeque" => "std::collections::VecDeque",
        // std::sync
        "Arc" => "std::sync::Arc",
        "Mutex" => "std::sync::Mutex",
        "RwLock" => "std::sync::RwLock",
        // std::rc / std::cell
        "Rc" => "std::rc::Rc",
        "RefCell" => "std::cell::RefCell",
        "Cell" => "std::cell::Cell",
        // std::time
        "Duration" => "std::time::Duration",
        "Instant" => "std::time::Instant",
        // std::io
        "BufReader" => "std::io::BufReader",
        "BufWriter" => "std::io::BufWriter",
        // Note: Result/Option/Vec/String/Box are in the 2021 prelude
        // already — no `use` needed.
        _ => return None,
    })
}

fn collect_std_uses(fn_text: &str) -> Vec<String> {
    // Parse the body as an item, walk it, collect every Ident.
    let parsed: syn::ItemFn = match syn::parse_str(fn_text) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let mut v = IdentCollector { names: BTreeSet::new() };
    v.visit_item_fn(&parsed);

    let mut found: BTreeSet<&'static str> = BTreeSet::new();
    for n in &v.names {
        if let Some(path) = std_prelude_path(n) {
            found.insert(path);
        }
    }
    found.into_iter().map(|p| format!("use {};", p)).collect()
}

struct IdentCollector {
    names: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for IdentCollector {
    fn visit_path(&mut self, p: &'ast syn::Path) {
        if let Some(first) = p.segments.first() {
            self.names.insert(first.ident.to_string());
        }
        syn::visit::visit_path(self, p);
    }

    fn visit_type_path(&mut self, tp: &'ast syn::TypePath) {
        if let Some(first) = tp.path.segments.first() {
            self.names.insert(first.ident.to_string());
        }
        syn::visit::visit_type_path(self, tp);
    }
}

/// For every same-crate file that still references the moved fn
/// unqualified (because the fn used to be in scope in its prior
/// location), inject `use <use_path>::<fn>;` after the existing `use`
/// block. The use path uses `crate::` for same-target moves and the
/// explicit crate name for bin→lib moves (where `crate::` would
/// resolve to the binary's own crate root).
fn inject_caller_uses(
    ws: &Workspace,
    src_file: &Path,
    dst_file: &Path,
    src_crate: &CrateInfo,
    dst_crate_key: &str,
    fn_name: &str,
    dst_module_segments: &[String],
    crosses_target_boundary: bool,
    edits: &mut Vec<FileEdit>,
) {
    let dst_lib_path = if dst_module_segments.is_empty() {
        fn_name.to_string()
    } else {
        format!("{}::{}", dst_module_segments.join("::"), fn_name)
    };

    let mut candidates: Vec<PathBuf> = Vec::new();
    candidates.push(src_file.to_path_buf());
    for file in workspace_rs_files(ws) {
        if file == src_file || file == dst_file {
            continue;
        }
        if let Some(owner) = crate_owning_file(ws, &file) {
            if owner.name == src_crate.name {
                candidates.push(file);
            }
        }
    }

    for file in candidates {
        if file == dst_file {
            continue;
        }
        let text = match find_edit(edits, &file) {
            Some(prev) => prev.new_contents.clone(),
            None => match std::fs::read_to_string(&file) {
                Ok(t) => t,
                Err(_) => continue,
            },
        };
        if !file_mentions_token(&text, fn_name) {
            continue;
        }
        // bin/main → explicit crate name; same-target lib file → `crate::`.
        let file_in_bin = is_bin_path(&file);
        let use_prefix = if file_in_bin || crosses_target_boundary {
            dst_crate_key.to_string()
        } else {
            "crate".to_string()
        };
        let use_line = format!("use {}::{};\n", use_prefix, dst_lib_path);
        if text.contains(use_line.trim_end()) {
            continue;
        }
        // Skip if the file already imports the fn (via any `use ...::<fn>;`).
        let any_existing_use = text
            .lines()
            .any(|l| l.trim().starts_with("use ") && l.trim().ends_with(&format!("::{};", fn_name)));
        if any_existing_use {
            continue;
        }
        let updated = insert_use_after_existing(&text, &use_line);
        merge_edit(edits, file, updated);
    }
}

/// Insert `use_line` (which must end in `\n`) after the last existing
/// `use ...;` line at the top of the file. If none exist, insert after
/// the file header (inner attrs + inner doc + `extern crate`) but
/// *before* any outer-doc-attached item.
fn insert_use_after_existing(text: &str, use_line: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut last_use_idx: Option<usize> = None;
    let mut first_item_idx: Option<usize> = None;
    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim_start();
        if t.starts_with("use ") {
            // If this `use` opens a brace group that doesn't close on
            // the same line, scan forward until the matching `;`.
            let mut end = i;
            let mut depth: i32 = 0;
            let mut terminated = false;
            for (k, l) in lines.iter().enumerate().skip(i) {
                for ch in l.chars() {
                    if ch == '{' {
                        depth += 1;
                    } else if ch == '}' {
                        depth -= 1;
                    } else if ch == ';' && depth == 0 {
                        end = k;
                        terminated = true;
                        break;
                    }
                }
                if terminated {
                    break;
                }
            }
            last_use_idx = Some(end);
            i = end + 1;
            continue;
        }
        // Header: blank, inner attr (#![...]), inner doc (//!),
        // regular comment (// ... but not /// or //!). Anything else
        // is the first item.
        let is_header = t.is_empty()
            || t.starts_with("#![")
            || t.starts_with("//!")
            || (t.starts_with("//") && !t.starts_with("///"));
        if !is_header && first_item_idx.is_none() {
            // Back up over any preceding outer doc / outer attrs that
            // bind to this item.
            let mut j = i;
            while j > 0 {
                let pt = lines[j - 1].trim_start();
                if pt.starts_with("///") || pt.starts_with("#[") || pt.starts_with("/**") {
                    j -= 1;
                } else {
                    break;
                }
            }
            first_item_idx = Some(j);
            break;
        }
        i += 1;
    }
    let insert_at = last_use_idx
        .map(|i| i + 1)
        .or(first_item_idx)
        .unwrap_or(lines.len());
    let mut out = String::new();
    for (i, l) in lines.iter().enumerate() {
        if i == insert_at {
            out.push_str(use_line);
        }
        out.push_str(l);
        out.push('\n');
    }
    if insert_at >= lines.len() {
        out.push_str(use_line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_crate_paths_basic() {
        let src = "fn x() { mycrate::foo::bar(); }";
        assert_eq!(
            rewrite_crate_paths(src, "mycrate"),
            "fn x() { crate::foo::bar(); }"
        );
    }

    #[test]
    fn rewrite_crate_paths_token_boundary() {
        let src = "fn x() { not_mycrate::foo(); mycrate::foo(); }";
        assert_eq!(
            rewrite_crate_paths(src, "mycrate"),
            "fn x() { not_mycrate::foo(); crate::foo(); }"
        );
    }

    #[test]
    fn collect_std_uses_for_path_and_exitcode() {
        let src = "fn x(p: &Path) -> ExitCode { let _ = PathBuf::from(\"\"); ExitCode::SUCCESS }";
        let uses = collect_std_uses(src);
        assert!(uses.iter().any(|u| u == "use std::path::Path;"), "{uses:?}");
        assert!(uses.iter().any(|u| u == "use std::path::PathBuf;"), "{uses:?}");
        assert!(uses.iter().any(|u| u == "use std::process::ExitCode;"), "{uses:?}");
    }

    #[test]
    fn ensure_visibility_lifts_private_to_pub() {
        let src = "fn helper() {}\n";
        let out = ensure_visibility(src, true);
        assert!(out.starts_with("pub fn helper"), "{out}");
    }

    #[test]
    fn ensure_visibility_lifts_pub_crate_to_pub_when_force() {
        let src = "pub(crate) fn helper() {}\n";
        let out = ensure_visibility(src, true);
        assert!(out.starts_with("pub fn helper"), "{out}");
    }

    #[test]
    fn ensure_visibility_keeps_pub_crate_without_force() {
        let src = "pub(crate) fn helper() {}\n";
        let out = ensure_visibility(src, false);
        assert!(out.starts_with("pub(crate) fn helper"), "{out}");
    }

    #[test]
    fn scaffold_doc_inserts_placeholder() {
        let src = "pub fn x() {}\n";
        let out = scaffold_doc_comment(src);
        assert!(out.contains("/// (auto-generated placeholder)"), "{out}");
    }
}
