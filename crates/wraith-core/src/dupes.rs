//! Token-shingled clone detection at the fn-body level.
//!
//! Approach: tokenize each fn body via `proc_macro2::TokenStream` into a
//! sequence of normalized token strings (idents collapsed to `I`, literals
//! to `L`, keywords kept as-is). Shingle into k-grams (k=5), bucket fns
//! by shared shingles, then compute Jaccard similarity for pairs sharing
//! >= 1 shingle. Report pairs above the similarity threshold with at
//! least `min_tokens` tokens.

use crate::config::DuplicateConfig;
use crate::report::{ClusterMember, Finding};
use proc_macro2::TokenStream;
use quote::ToTokens;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use syn::visit::Visit;

const SHINGLE_K: usize = 5;

#[derive(Debug)]
struct FnFingerprint {
    #[allow(dead_code)]
    name: String,
    qualified: String,
    file: PathBuf,
    line: usize,
    col: usize,
    tokens: usize,
    shingles: HashSet<u64>,
}

struct FnCollector<'a> {
    crate_name: String,
    module_path: Vec<String>,
    file: PathBuf,
    out: &'a mut Vec<FnFingerprint>,
    min_tokens: usize,
}

fn normalize_tokens(ts: TokenStream) -> Vec<String> {
    let mut out = Vec::new();
    for tt in ts {
        match tt {
            proc_macro2::TokenTree::Ident(i) => {
                let s = i.to_string();
                // keep keywords / known control flow; collapse other idents
                if matches!(
                    s.as_str(),
                    "if" | "else"
                        | "match"
                        | "for"
                        | "while"
                        | "loop"
                        | "return"
                        | "break"
                        | "continue"
                        | "let"
                        | "mut"
                        | "ref"
                        | "fn"
                        | "impl"
                        | "self"
                        | "Self"
                        | "true"
                        | "false"
                        | "as"
                        | "in"
                        | "where"
                        | "move"
                        | "async"
                        | "await"
                        | "use"
                ) {
                    out.push(s);
                } else {
                    out.push("I".to_string());
                }
            }
            proc_macro2::TokenTree::Literal(_) => out.push("L".to_string()),
            proc_macro2::TokenTree::Punct(p) => out.push(p.as_char().to_string()),
            proc_macro2::TokenTree::Group(g) => {
                let delim = match g.delimiter() {
                    proc_macro2::Delimiter::Parenthesis => ("(", ")"),
                    proc_macro2::Delimiter::Brace => ("{", "}"),
                    proc_macro2::Delimiter::Bracket => ("[", "]"),
                    proc_macro2::Delimiter::None => ("", ""),
                };
                if !delim.0.is_empty() {
                    out.push(delim.0.to_string());
                }
                out.extend(normalize_tokens(g.stream()));
                if !delim.1.is_empty() {
                    out.push(delim.1.to_string());
                }
            }
        }
    }
    out
}

fn shingle(tokens: &[String], k: usize) -> HashSet<u64> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut out = HashSet::new();
    if tokens.len() < k {
        return out;
    }
    for w in tokens.windows(k) {
        let mut h = DefaultHasher::new();
        for t in w {
            t.hash(&mut h);
            0u8.hash(&mut h);
        }
        out.insert(h.finish());
    }
    out
}

impl<'a, 'ast> Visit<'ast> for FnCollector<'a> {
    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        if i.ident == "tests" {
            return;
        }
        self.module_path.push(i.ident.to_string());
        syn::visit::visit_item_mod(self, i);
        self.module_path.pop();
    }

    fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
        // skip test fns
        if i.attrs.iter().any(|a| {
            a.path().is_ident("test")
                || a.path().is_ident("bench")
                || a.path().is_ident("ignore")
        }) {
            return;
        }
        let mut block_ts = TokenStream::new();
        i.block.to_tokens(&mut block_ts);
        let tokens = normalize_tokens(block_ts);
        if tokens.len() < self.min_tokens {
            return;
        }
        let shingles = shingle(&tokens, SHINGLE_K);
        if shingles.is_empty() {
            return;
        }
        let name = i.sig.ident.to_string();
        let qualified = if self.module_path.is_empty() {
            format!("{}::{}", self.crate_name, name)
        } else {
            format!("{}::{}::{}", self.crate_name, self.module_path.join("::"), name)
        };
        let span = syn::spanned::Spanned::span(&i.sig.ident).start();
        self.out.push(FnFingerprint {
            name,
            qualified,
            file: self.file.clone(),
            line: span.line,
            col: span.column + 1,
            tokens: tokens.len(),
            shingles,
        });
    }
}

/// Internal: compute all above-threshold similarity edges between fns.
fn compute_edges(
    crate_files: &[(String, Vec<PathBuf>)],
    cfg: &DuplicateConfig,
) -> (Vec<FnFingerprint>, Vec<(usize, usize, f32)>) {
    let mut fps: Vec<FnFingerprint> = Vec::new();
    for (crate_name, files) in crate_files {
        for f in files {
            collect_fns(crate_name, f, cfg.min_tokens, &mut fps);
        }
    }

    let mut buckets: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, fp) in fps.iter().enumerate() {
        for &s in &fp.shingles {
            buckets.entry(s).or_default().push(i);
        }
    }

    let mut seen_pairs: HashSet<(usize, usize)> = HashSet::new();
    let mut edges: Vec<(usize, usize, f32)> = Vec::new();
    for ids in buckets.values() {
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                let (a, b) = if ids[i] < ids[j] {
                    (ids[i], ids[j])
                } else {
                    (ids[j], ids[i])
                };
                if !seen_pairs.insert((a, b)) {
                    continue;
                }
                let fa = &fps[a];
                let fb = &fps[b];
                if fa.qualified == fb.qualified {
                    continue;
                }
                let inter = fa.shingles.intersection(&fb.shingles).count();
                let union = fa.shingles.union(&fb.shingles).count();
                if union == 0 {
                    continue;
                }
                let sim = inter as f32 / union as f32;
                if sim < cfg.similarity_threshold {
                    continue;
                }
                edges.push((a, b, sim));
            }
        }
    }
    (fps, edges)
}

/// Pair-mode dupes (opt-in via `--pairs`). Canonicalizes (a, b) by lex
/// order of qualified names and dedupes so each logical pair appears
/// at most once.
pub fn find_duplicates(
    crate_files: &[(String, Vec<PathBuf>)],
    cfg: &DuplicateConfig,
) -> Vec<Finding> {
    let (fps, edges) = compute_edges(crate_files, cfg);
    let mut emitted: HashSet<(String, String)> = HashSet::new();
    let mut out = Vec::new();
    for (a, b, sim) in edges {
        let fa = &fps[a];
        let fb = &fps[b];
        let (na, nb) = if fa.qualified <= fb.qualified {
            (&fa.qualified, &fb.qualified)
        } else {
            (&fb.qualified, &fa.qualified)
        };
        if !emitted.insert((na.clone(), nb.clone())) {
            continue;
        }
        let token_count = fa.tokens.min(fb.tokens);
        out.push(Finding::duplicate(
            fa.file.clone(),
            fa.line,
            fa.col,
            na,
            nb,
            sim,
            token_count,
        ));
    }
    out
}

/// Cluster-mode dupes (default). Connected components on the similarity
/// graph; one Finding per component of size >= 2.
pub fn find_duplicate_clusters(
    crate_files: &[(String, Vec<PathBuf>)],
    cfg: &DuplicateConfig,
) -> Vec<Finding> {
    let (fps, edges) = compute_edges(crate_files, cfg);
    if fps.is_empty() {
        return Vec::new();
    }

    let n = fps.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn uf_find(parent: &mut [usize], x: usize) -> usize {
        if parent[x] == x {
            return x;
        }
        let r = uf_find(parent, parent[x]);
        parent[x] = r;
        r
    }
    fn uf_union(parent: &mut [usize], a: usize, b: usize) {
        let ra = uf_find(parent, a);
        let rb = uf_find(parent, b);
        if ra != rb {
            parent[ra] = rb;
        }
    }

    let mut touched: HashSet<usize> = HashSet::new();
    for (a, b, _sim) in &edges {
        uf_union(&mut parent, *a, *b);
        touched.insert(*a);
        touched.insert(*b);
    }

    let mut components: HashMap<usize, Vec<usize>> = HashMap::new();
    for &i in &touched {
        let r = uf_find(&mut parent, i);
        components.entry(r).or_default().push(i);
    }

    let mut comp_stats: HashMap<usize, (f32, f32)> = HashMap::new();
    for (a, _b, sim) in &edges {
        let r = uf_find(&mut parent, *a);
        let entry = comp_stats.entry(r).or_insert((*sim, *sim));
        if *sim < entry.0 {
            entry.0 = *sim;
        }
        if *sim > entry.1 {
            entry.1 = *sim;
        }
    }

    let mut out = Vec::new();
    let mut roots: Vec<usize> = components.keys().copied().collect();
    roots.sort();
    for r in roots {
        let mut ids = components.remove(&r).unwrap();
        if ids.len() < 2 {
            continue;
        }
        ids.sort_by(|&i, &j| fps[i].qualified.cmp(&fps[j].qualified));
        ids.dedup_by(|a, b| fps[*a].qualified == fps[*b].qualified);
        if ids.len() < 2 {
            continue;
        }
        let (min_s, max_s) = comp_stats.get(&r).copied().unwrap_or((0.0, 0.0));
        let members: Vec<ClusterMember> = ids
            .iter()
            .map(|&i| ClusterMember {
                symbol: fps[i].qualified.clone(),
                file: fps[i].file.display().to_string(),
                line: fps[i].line,
            })
            .collect();
        let anchor = &fps[ids[0]];
        out.push(Finding::duplicate_cluster(
            anchor.file.clone(),
            anchor.line,
            anchor.col,
            members,
            min_s,
            max_s,
        ));
    }
    out
}

fn collect_fns(crate_name: &str, file: &Path, min_tokens: usize, out: &mut Vec<FnFingerprint>) {
    let Ok(text) = std::fs::read_to_string(file) else {
        return;
    };
    let Ok(ast) = syn::parse_file(&text) else {
        return;
    };
    let mut c = FnCollector {
        crate_name: crate_name.to_string(),
        module_path: Vec::new(),
        file: file.to_path_buf(),
        out,
        min_tokens,
    };
    c.visit_file(&ast);
}
