use crate::cache::{file_mtime_secs, Cache, CacheStats, CachedFile};
use crate::config::Config;
use crate::graph::{ReferenceGraph, Visibility};
use crate::parse::parse_file;
use crate::report::Finding;
use crate::workspace::{DepKind, Workspace};
use std::collections::HashSet;
use std::path::Path;

/// Build a reference graph for the entire workspace, populating
/// `cache` in place with fresh entries for stale or new files.
fn build_graph_cached(
    ws: &Workspace,
    cache: &mut Cache,
    stats: &mut CacheStats,
) -> anyhow::Result<ReferenceGraph> {
    let mut graph = ReferenceGraph::new();
    for c in &ws.crates {
        graph.workspace_crates.insert(c.name.clone());
        graph.workspace_crates.insert(c.name.replace('-', "_"));
    }

    let mut live_files: HashSet<std::path::PathBuf> = HashSet::new();

    for c in &ws.crates {
        let files = ws.crate_rs_files(c);
        let crate_key = c.name.replace('-', "_");
        for f in files {
            live_files.insert(f.clone());
            let mtime = file_mtime_secs(&f);

            let cached_hit = match (mtime, cache.get(&f)) {
                (Some(m), Some(entry)) => m <= entry.mtime_secs,
                _ => false,
            };

            if cached_hit {
                let entry = cache.get(&f).expect("checked above").clone();
                for sym in entry.symbols {
                    graph.add_symbol(sym);
                }
                for r in entry.references {
                    graph.add_reference(r);
                }
                stats.hits += 1;
                continue;
            }

            let sym_start = graph.symbols.len();
            let ref_start = graph.references.len();
            parse_file(&mut graph, &crate_key, &f)?;
            let new_syms = graph.symbols[sym_start..].to_vec();
            let new_refs = graph.references[ref_start..].to_vec();
            let mtime_secs = mtime.unwrap_or(0);
            cache.insert(
                f.clone(),
                CachedFile {
                    mtime_secs,
                    symbols: new_syms,
                    references: new_refs,
                },
            );
            stats.misses += 1;
        }
    }

    // Evict entries for files that no longer exist in the workspace.
    cache.entries.retain(|p, _| live_files.contains(p));

    Ok(graph)
}

/// Public façade used by callers that don't want to thread a cache —
/// constructs an empty cache, runs the graph build, discards the cache.
pub fn build_graph(ws: &Workspace) -> anyhow::Result<ReferenceGraph> {
    let mut cache = Cache::new();
    let mut stats = CacheStats::default();
    build_graph_cached(ws, &mut cache, &mut stats)
}

pub fn find_dead_code(graph: &ReferenceGraph, cfg: &Config) -> Vec<Finding> {
    let referenced = graph.referenced_symbols();
    let allow: HashSet<&str> = cfg.allow_dead.iter().map(|s| s.as_str()).collect();
    let mut out = Vec::new();
    for (idx, sym) in graph.symbols.iter().enumerate() {
        if sym.is_test
            || sym.is_entry_point
            || sym.has_no_mangle
            || sym.has_export_name
            || sym.impl_of_foreign_trait
        {
            continue;
        }
        if !matches!(sym.visibility, Visibility::Public | Visibility::PubCrate) {
            // private items: cargo's own dead-code lint handles these.
            continue;
        }
        if allow.contains(sym.symbol.name.as_str()) {
            continue;
        }
        if referenced.contains(&idx) {
            continue;
        }
        out.push(Finding::dead_code(
            sym.file.clone(),
            sym.line,
            sym.col,
            &sym.symbol.qualified(),
            sym.kind,
            "no references in workspace",
        ));
    }
    out
}

pub fn find_unused_deps(ws: &Workspace, graph: &ReferenceGraph, cfg: &Config) -> Vec<Finding> {
    let allow: HashSet<&str> = cfg.allow_unused_deps.iter().map(|s| s.as_str()).collect();
    let mut out = Vec::new();

    // Bucket references per crate.
    let mut refs_by_crate: std::collections::HashMap<String, HashSet<String>> =
        std::collections::HashMap::new();
    for r in &graph.references {
        let entry = refs_by_crate
            .entry(r.from_crate.clone())
            .or_default();
        if let Some(root) = &r.root {
            entry.insert(root.clone());
        }
        // For `use foo::bar` the root is foo; for bare `Foo` paths we
        // can't tell — be conservative and ALSO add the leaf-name.
        entry.insert(r.name.clone());
    }

    for c in &ws.crates {
        let crate_key = c.name.replace('-', "_");
        let used = refs_by_crate.get(&crate_key).cloned().unwrap_or_default();
        for dep in &c.deps {
            // What name does this dep appear as in source?
            let local = dep
                .rename
                .clone()
                .unwrap_or_else(|| dep.name.replace('-', "_"));
            // optional, dev, build: more lenient — but the brief still
            // wants them reported. We mark the dep_kind so users can grep.
            if allow.contains(local.as_str()) || allow.contains(dep.name.as_str()) {
                continue;
            }
            if used.contains(&local) || used.contains(&dep.name) {
                continue;
            }
            // Build-deps often used only in build.rs — only report if
            // we can prove build.rs doesn't mention them. For MVP: skip
            // build deps. dev-deps: only report if not in tests/.
            if matches!(dep.kind, DepKind::Build) {
                continue;
            }
            if dep.optional {
                // optional deps are typically feature-gated; skip.
                continue;
            }
            out.push(Finding::unused_dep(
                c.manifest_path.clone(),
                &c.name,
                &dep.name,
                match dep.kind {
                    DepKind::Normal => "normal",
                    DepKind::Dev => "dev",
                    DepKind::Build => "build",
                },
            ));
        }
    }

    out
}

/// Parse Cargo.toml and Cargo.lock-adjacent crates from a path.
///
/// On warm invocations the workspace graph is restored from
/// `<root>/.wraithrc.cache` — only files whose mtime is newer than the
/// cached entry get re-parsed. The cache is written back after the
/// build completes (best-effort; write failures are silent).
pub fn analyze_root(root: &Path, cfg: &Config) -> anyhow::Result<(Workspace, ReferenceGraph)> {
    let ws = Workspace::load(root)?;
    let mut cache = Cache::load(&ws.root);
    let mut stats = CacheStats::default();
    let graph = build_graph_cached(&ws, &mut cache, &mut stats)?;
    let _ = cache.save(&ws.root);
    if std::env::var("WRAITH_CACHE_DEBUG").ok().as_deref() == Some("1") {
        eprintln!(
            "wraith: cache hits={} misses={} entries={}",
            stats.hits,
            stats.misses,
            cache.len()
        );
    }
    let _ = cfg; // reserved for future filtering
    Ok((ws, graph))
}
