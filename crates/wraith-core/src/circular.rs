//! Circular dependency detection — module-level (inside one crate) and
//! crate-level (across workspace). Uses Tarjan SCC via petgraph.

use crate::graph::ReferenceGraph;
use crate::report::{Finding, Severity};
use crate::workspace::Workspace;
use petgraph::algo::tarjan_scc;
use petgraph::graphmap::DiGraphMap;
use std::collections::HashSet;
use std::path::PathBuf;

/// Crate-level cycles. Rust prevents these from compiling, but workspace
/// authors hit them mid-refactor; reporting them speeds the fix.
pub fn find_crate_cycles(ws: &Workspace) -> Vec<Finding> {
    let mut g: DiGraphMap<&str, ()> = DiGraphMap::new();
    let names: HashSet<&str> = ws.crates.iter().map(|c| c.name.as_str()).collect();
    for c in &ws.crates {
        g.add_node(c.name.as_str());
    }
    for c in &ws.crates {
        for dep in &c.deps {
            if names.contains(dep.name.as_str()) {
                g.add_edge(c.name.as_str(), dep.name.as_str(), ());
            }
        }
    }
    let sccs = tarjan_scc(&g);
    let mut out = Vec::new();
    for scc in sccs {
        if scc.len() < 2 {
            continue;
        }
        let cycle: Vec<String> = scc.iter().map(|s| s.to_string()).collect();
        // attach to first crate's manifest as the finding location
        let file = ws
            .crates
            .iter()
            .find(|c| c.name == scc[0])
            .map(|c| c.manifest_path.clone())
            .unwrap_or_else(|| PathBuf::from("Cargo.toml"));
        out.push(Finding::circular(file, "crate", cycle, Severity::Error));
    }
    out
}

/// Module-level cycles inside each crate. We build a directed graph
/// where nodes are `crate::a::b` module paths, with edges drawn from
/// every recorded reference whose root segment is `crate`/`self`/`super`
/// or a same-crate qualified path. This is a coarse approximation —
/// good enough to flag classic A↔B / A→B→C→A cycles between sibling
/// modules.
pub fn find_module_cycles(ws: &Workspace, graph: &ReferenceGraph) -> Vec<Finding> {
    let mut out = Vec::new();

    // For each crate independently — module cycles don't cross crates.
    for c in &ws.crates {
        let crate_key = c.name.replace('-', "_");
        let crate_src_root = c
            .src_paths
            .first()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf());

        // Derive a module-path string from a file path relative to the
        // crate's src/ root. e.g. `src/foo/mod.rs` → "foo", `src/foo/bar.rs`
        // → "foo::bar". This handles `pub mod x;` external files which
        // the AST walker can't see.
        let path_to_mod = |f: &PathBuf| -> String {
            let Some(root) = crate_src_root.as_ref() else {
                return "crate".to_string();
            };
            let Ok(rel) = f.strip_prefix(root) else {
                return "crate".to_string();
            };
            let mut parts: Vec<String> = rel
                .components()
                .filter_map(|c| match c {
                    std::path::Component::Normal(s) => Some(s.to_string_lossy().to_string()),
                    _ => None,
                })
                .collect();
            if let Some(last) = parts.last_mut() {
                if last.ends_with(".rs") {
                    last.truncate(last.len() - 3);
                }
                if last == "mod" || last == "lib" || last == "main" {
                    parts.pop();
                }
            }
            if parts.is_empty() {
                "crate".to_string()
            } else {
                parts.join("::")
            }
        };

        // Build symbol → owning-module map for this crate, keyed by
        // file-path-derived module (so cross-file symbols resolve right).
        let mut sym_to_mod: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for s in &graph.symbols {
            if s.symbol.crate_name != crate_key {
                continue;
            }
            let m = path_to_mod(&s.file);
            sym_to_mod
                .entry(s.symbol.name.clone())
                .or_default()
                .push(m);
        }

        // Valid module paths in this crate (derived from file tree).
        // Used to disambiguate leaf-name collisions: a Reference whose
        // path walks through known modules resolves uniquely; one whose
        // path doesn't is treated as ambiguous and dropped. See wb-5lgj.24.
        let mut module_paths: HashSet<String> = HashSet::new();
        for s in &graph.symbols {
            if s.symbol.crate_name != crate_key {
                continue;
            }
            module_paths.insert(path_to_mod(&s.file));
        }
        let module_leaf_names: HashSet<String> = module_paths
            .iter()
            .filter_map(|m| m.rsplit("::").next().map(|s| s.to_string()))
            .collect();

        let file_to_mod = |f: &PathBuf| -> Option<String> { Some(path_to_mod(f)) };

        let mut node_ids: std::collections::HashMap<String, petgraph::graph::NodeIndex> =
            std::collections::HashMap::new();
        let mut pg: petgraph::Graph<String, ()> = petgraph::Graph::new();
        let ensure = |s: &str, pg: &mut petgraph::Graph<String, ()>,
                      ids: &mut std::collections::HashMap<String, petgraph::graph::NodeIndex>|
         -> petgraph::graph::NodeIndex {
            if let Some(&i) = ids.get(s) {
                return i;
            }
            let i = pg.add_node(s.to_string());
            ids.insert(s.to_string(), i);
            i
        };

        for r in &graph.references {
            if r.from_crate != crate_key {
                continue;
            }
            let Some(from_mod) = file_to_mod(&r.from_file) else {
                continue;
            };
            let same_crate = matches!(
                r.root.as_deref(),
                None | Some("crate") | Some("self") | Some("super") | Some("Self")
            );
            if !same_crate {
                continue;
            }
            let Some(targets) = sym_to_mod.get(&r.name) else {
                continue;
            };
            // Resolve to a single target module using the full path
            // segments + intermediate-segment flag. Leaf-name lookup
            // alone collapses graphs into giant SCCs because common
            // method names (new/from/render/build/run) are defined in
            // dozens of modules; without disambiguation we'd add edges
            // from each call site to every one of them. See wb-5lgj.24.
            let Some(to_mod) = resolve_target(
                &r.segments,
                r.is_path_segment,
                r.root.as_deref(),
                &r.name,
                targets,
                &sym_to_mod,
                &module_paths,
                &module_leaf_names,
            ) else {
                continue;
            };
            if to_mod == &from_mod {
                continue;
            }
            // The synthetic "crate" node represents the crate root
            // (src/lib.rs or src/main.rs). It isn't a sibling module
            // in the way that participates in inter-module cycles —
            // `mod foo;` declarations at the root plus a leaf module
            // reading a root constant would otherwise SCC-collapse
            // everything reachable from the root into one giant
            // false-positive cycle.
            if from_mod == "crate" || to_mod == "crate" {
                continue;
            }
            let a = ensure(&from_mod, &mut pg, &mut node_ids);
            let b = ensure(to_mod, &mut pg, &mut node_ids);
            pg.update_edge(a, b, ());
        }

        let sccs = petgraph::algo::tarjan_scc(&pg);
        for scc in sccs {
            if scc.len() < 2 {
                continue;
            }
            let cycle: Vec<String> = scc.iter().map(|n| pg[*n].clone()).collect();
            out.push(Finding::circular(
                c.manifest_path.clone(),
                "module",
                cycle,
                Severity::Warning,
            ));
        }
    }

    out
}

/// Pick the single owning module for a Reference, or None if ambiguous.
///
/// Rules (descending precision):
/// 1. Multi-segment paths walk through known modules. For
///    `crate::foo::bar::new`, if `foo::bar` is a real module and `new`
///    is defined there, that's the unique target.
/// 2. `is_path_segment=true` (intermediate segments like `foo` and
///    `bar` in the above) resolves only against modules; never against
///    fn/struct symbols that happen to share the name.
/// 3. Exactly one defining module → that's the target.
/// 4. `root` is a type defined in exactly one module → use it.
/// 5. Otherwise skip. Adding edges to every candidate is exactly the
///    bug that produced the 148-node SCC.
#[allow(clippy::too_many_arguments)]
fn resolve_target<'a>(
    segments: &[String],
    is_path_segment: bool,
    root: Option<&str>,
    leaf: &str,
    targets: &'a [String],
    sym_to_mod: &std::collections::HashMap<String, Vec<String>>,
    module_paths: &HashSet<String>,
    module_leaf_names: &HashSet<String>,
) -> Option<&'a String> {
    if is_path_segment {
        if !module_leaf_names.contains(leaf) {
            return None;
        }
        if let Some(m) = walk_path_to_module(segments, module_paths) {
            return targets.iter().find(|t| **t == m);
        }
        let mut matched: Option<&String> = None;
        for mp in module_paths {
            if mp.rsplit("::").next() == Some(leaf) {
                if matched.is_some() {
                    return None;
                }
                matched = Some(mp);
            }
        }
        if let Some(m) = matched {
            return targets.iter().find(|t| *t == m);
        }
        return None;
    }

    if segments.len() > 1 {
        let prefix = &segments[..segments.len() - 1];
        if let Some(m) = walk_path_to_module(prefix, module_paths) {
            for t in targets {
                if *t == m {
                    return Some(t);
                }
            }
        }
    }

    // Rule 3 only fires when the leaf is unambiguous AND we have *some*
    // path context (a root segment). A bare unqualified `foo()` with one
    // matching definition could still be a same-module call via a use
    // statement we didn't track; without any qualifier we can't tell, so
    // skip to avoid the spurious-edge family that gave us the 148-node
    // SCC. Single-match WITH a root segment is much more reliable.
    if targets.len() == 1 && root.is_some() {
        return Some(&targets[0]);
    }

    if let Some(root_seg) = root {
        if !matches!(root_seg, "crate" | "self" | "super" | "Self") {
            if let Some(root_mods) = sym_to_mod.get(root_seg) {
                if root_mods.len() == 1 {
                    let owner = &root_mods[0];
                    for t in targets {
                        if t == owner {
                            return Some(t);
                        }
                    }
                }
            }
        }
    }

    None
}

/// Resolve a sequence of path segments to a module-path string. Strips
/// leading `crate`/`self`/`super` qualifiers, then checks whether the
/// remaining joined string is a known module path.
fn walk_path_to_module(segments: &[String], module_paths: &HashSet<String>) -> Option<String> {
    let mut start = 0;
    for s in segments {
        if matches!(s.as_str(), "crate" | "self" | "super") {
            start += 1;
        } else {
            break;
        }
    }
    let rest = &segments[start..];
    if rest.is_empty() {
        return None;
    }
    let candidate = rest.join("::");
    if module_paths.contains(&candidate) {
        Some(candidate)
    } else {
        None
    }
}
