use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SymbolKind {
    Fn,
    Struct,
    Enum,
    Trait,
    TypeAlias,
    Const,
    Static,
    Mod,
}

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolKind::Fn => "fn",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Trait => "trait",
            SymbolKind::TypeAlias => "type",
            SymbolKind::Const => "const",
            SymbolKind::Static => "static",
            SymbolKind::Mod => "mod",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Symbol {
    pub crate_name: String,
    pub module_path: Vec<String>,
    pub name: String,
}

impl Symbol {
    pub fn qualified(&self) -> String {
        if self.module_path.is_empty() {
            format!("{}::{}", self.crate_name, self.name)
        } else {
            format!(
                "{}::{}::{}",
                self.crate_name,
                self.module_path.join("::"),
                self.name
            )
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    PubCrate,
    Private,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolNode {
    pub symbol: Symbol,
    pub kind: SymbolKind,
    pub visibility: Visibility,
    pub file: PathBuf,
    pub line: usize,
    pub col: usize,
    pub is_test: bool,
    pub is_entry_point: bool,
    pub has_no_mangle: bool,
    pub has_export_name: bool,
    pub is_reexport: bool,
    pub impl_of_foreign_trait: bool,
}

/// A reference observed in source code. Free-form text — resolution
/// happens later against the symbol table by name match.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reference {
    pub from_file: PathBuf,
    pub from_crate: String,
    /// The leaf segment of the path (final ident), e.g. `foo` for `bar::foo`.
    pub name: String,
    /// The crate-or-root segment, if discoverable (e.g. `serde`, `crate`, `self`, `super`).
    pub root: Option<String>,
    /// Full path segments as written (e.g. ["crate","foo","bar","new"]).
    /// Used to disambiguate leaf-name collisions during module-cycle
    /// detection. See wb-5lgj.24.
    #[serde(default)]
    pub segments: Vec<String>,
    /// True for references synthesized from intermediate path segments
    /// (e.g. `foo` and `bar` from `crate::foo::bar::new`). These
    /// resolve only against modules, never against same-named symbols.
    #[serde(default)]
    pub is_path_segment: bool,
    pub line: usize,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ReferenceGraph {
    pub symbols: Vec<SymbolNode>,
    pub references: Vec<Reference>,
    /// Map from leaf name → list of symbol indices defining that name.
    pub by_name: HashMap<String, Vec<usize>>,
    /// Crate names present in the workspace (for resolving extern-crate references).
    pub workspace_crates: HashSet<String>,
}

impl ReferenceGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_symbol(&mut self, n: SymbolNode) {
        let idx = self.symbols.len();
        self.by_name
            .entry(n.symbol.name.clone())
            .or_default()
            .push(idx);
        self.symbols.push(n);
    }

    pub fn add_reference(&mut self, r: Reference) {
        self.references.push(r);
    }

    /// Resolve a qualified-or-leaf symbol identifier to a single symbol
    /// index. Accepts:
    /// - leaf name: `run_image` — matches if unambiguous
    /// - `crate::name`: `wavelet::run_image`
    /// - full qualified: `wavelet::module::run_image`
    pub fn resolve_symbol(&self, target: &str) -> Option<usize> {
        for (i, s) in self.symbols.iter().enumerate() {
            if s.symbol.qualified() == target {
                return Some(i);
            }
        }
        let leaf = target.rsplit("::").next().unwrap_or(target);
        let candidates = self.by_name.get(leaf)?;
        if !target.contains("::") && candidates.len() == 1 {
            return Some(candidates[0]);
        }
        // crate::leaf form
        if let Some((root, _)) = target.split_once("::") {
            let mut hit: Option<usize> = None;
            for &idx in candidates {
                let s = &self.symbols[idx];
                if s.symbol.crate_name == root {
                    if hit.is_some() {
                        return None;
                    }
                    hit = Some(idx);
                }
            }
            if hit.is_some() {
                return hit;
            }
        }
        if candidates.len() == 1 {
            return Some(candidates[0]);
        }
        None
    }

    /// All symbol indices whose leaf name + crate match the resolution
    /// rules — useful for displaying ambiguity to the agent.
    pub fn resolve_symbol_all(&self, target: &str) -> Vec<usize> {
        let mut out = Vec::new();
        for (i, s) in self.symbols.iter().enumerate() {
            if s.symbol.qualified() == target {
                out.push(i);
            }
        }
        if !out.is_empty() {
            return out;
        }
        let leaf = target.rsplit("::").next().unwrap_or(target);
        let Some(candidates) = self.by_name.get(leaf) else {
            return out;
        };
        if let Some((root, _)) = target.split_once("::") {
            for &idx in candidates {
                if self.symbols[idx].symbol.crate_name == root {
                    out.push(idx);
                }
            }
            if !out.is_empty() {
                return out;
            }
        }
        candidates.clone()
    }

    /// Find the enclosing symbol for a reference site: the symbol in
    /// the same file with the largest `line <= ref_line`. Returns None
    /// if no such symbol exists (e.g. file-level use statement).
    pub fn enclosing_symbol(&self, from_file: &std::path::Path, ref_line: usize) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None; // (idx, line)
        for (i, s) in self.symbols.iter().enumerate() {
            if s.file != from_file {
                continue;
            }
            if s.line > ref_line {
                continue;
            }
            match best {
                Some((_, prev_line)) if prev_line >= s.line => {}
                _ => best = Some((i, s.line)),
            }
        }
        best.map(|(i, _)| i)
    }

    /// Returns true if a reference resolves to the given symbol index,
    /// using the same matching semantics as `referenced_symbols`.
    fn reference_matches(&self, r: &Reference, target_idx: usize) -> bool {
        let node = &self.symbols[target_idx];
        if node.symbol.name != r.name {
            return false;
        }
        match r.root.as_deref() {
            None => true,
            Some("crate") | Some("self") | Some("super") | Some("Self") => {
                node.symbol.crate_name == r.from_crate
            }
            Some(root) => {
                root == node.symbol.crate_name || node.symbol.crate_name == r.from_crate
            }
        }
    }

    /// Direct callers of `target_idx`: symbol indices whose body contains
    /// a reference to that symbol. Uses `enclosing_symbol` to map each
    /// reference site to its containing symbol.
    pub fn direct_callers(&self, target_idx: usize) -> Vec<usize> {
        let mut seen: HashSet<usize> = HashSet::new();
        let mut out = Vec::new();
        for r in &self.references {
            if !self.reference_matches(r, target_idx) {
                continue;
            }
            let Some(caller) = self.enclosing_symbol(&r.from_file, r.line) else {
                continue;
            };
            if caller == target_idx {
                continue;
            }
            if seen.insert(caller) {
                out.push(caller);
            }
        }
        out
    }

    /// Direct callees of `caller_idx`: symbol indices referenced from
    /// within that symbol's body. Body span is approximated as the
    /// half-open `[caller.line, next_symbol_in_file.line)` range.
    pub fn direct_callees(&self, caller_idx: usize) -> Vec<usize> {
        let caller = &self.symbols[caller_idx];
        let next_line = self
            .symbols
            .iter()
            .filter(|s| s.file == caller.file && s.line > caller.line)
            .map(|s| s.line)
            .min()
            .unwrap_or(usize::MAX);
        let mut seen: HashSet<usize> = HashSet::new();
        let mut out = Vec::new();
        for r in &self.references {
            if r.from_file != caller.file {
                continue;
            }
            if r.line < caller.line || r.line >= next_line {
                continue;
            }
            let Some(candidates) = self.by_name.get(&r.name) else {
                continue;
            };
            for &idx in candidates {
                if idx == caller_idx {
                    continue;
                }
                if self.reference_matches(r, idx) && seen.insert(idx) {
                    out.push(idx);
                }
            }
        }
        out
    }

    /// Forward N-hop walk of dependents (transitive callers). Each entry
    /// in the result is `(symbol_idx, distance)`. `max_depth = None` means
    /// unlimited.
    pub fn blast_radius(&self, target_idx: usize, max_depth: Option<usize>) -> Vec<(usize, usize)> {
        let mut visited: HashSet<usize> = HashSet::new();
        visited.insert(target_idx);
        let mut out: Vec<(usize, usize)> = Vec::new();
        let mut queue: VecDeque<(usize, usize)> = VecDeque::new();
        queue.push_back((target_idx, 0));
        while let Some((idx, depth)) = queue.pop_front() {
            if let Some(max) = max_depth {
                if depth >= max {
                    continue;
                }
            }
            for caller in self.direct_callers(idx) {
                if visited.insert(caller) {
                    out.push((caller, depth + 1));
                    queue.push_back((caller, depth + 1));
                }
            }
        }
        out
    }

    /// Transitive callees of `caller_idx`. Returns `(symbol_idx, distance)`.
    pub fn transitive_callees(&self, caller_idx: usize, max_depth: Option<usize>) -> Vec<(usize, usize)> {
        let mut visited: HashSet<usize> = HashSet::new();
        visited.insert(caller_idx);
        let mut out: Vec<(usize, usize)> = Vec::new();
        let mut queue: VecDeque<(usize, usize)> = VecDeque::new();
        queue.push_back((caller_idx, 0));
        while let Some((idx, depth)) = queue.pop_front() {
            if let Some(max) = max_depth {
                if depth >= max {
                    continue;
                }
            }
            for callee in self.direct_callees(idx) {
                if visited.insert(callee) {
                    out.push((callee, depth + 1));
                    queue.push_back((callee, depth + 1));
                }
            }
        }
        out
    }

    /// Crate names that import symbols from `target_crate` (i.e. crates
    /// that have at least one reference whose `root` resolves to it).
    pub fn reverse_crate_deps(&self, target_crate: &str) -> Vec<String> {
        let mut seen: HashSet<String> = HashSet::new();
        for r in &self.references {
            if r.from_crate == target_crate {
                continue;
            }
            let hits_target = match r.root.as_deref() {
                Some(root) => root == target_crate,
                None => self
                    .by_name
                    .get(&r.name)
                    .map(|cands| {
                        cands
                            .iter()
                            .any(|&i| self.symbols[i].symbol.crate_name == target_crate)
                    })
                    .unwrap_or(false),
            };
            if hits_target {
                seen.insert(r.from_crate.clone());
            }
        }
        let mut out: Vec<String> = seen.into_iter().collect();
        out.sort();
        out
    }

    /// Symbols whose qualified module path begins with `target_module`
    /// (e.g. `wavelet::render`). Returns the set of crates that reference
    /// any of those symbols from outside.
    pub fn reverse_module_deps(&self, target_module: &str) -> Vec<String> {
        let target_indices: HashSet<usize> = self
            .symbols
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                let q = s.symbol.qualified();
                q == target_module || q.starts_with(&format!("{}::", target_module))
            })
            .map(|(i, _)| i)
            .collect();
        if target_indices.is_empty() {
            return Vec::new();
        }
        let mut seen: HashSet<String> = HashSet::new();
        for r in &self.references {
            let Some(candidates) = self.by_name.get(&r.name) else {
                continue;
            };
            for &idx in candidates {
                if target_indices.contains(&idx) && self.reference_matches(r, idx) {
                    seen.insert(r.from_crate.clone());
                    break;
                }
            }
        }
        let mut out: Vec<String> = seen.into_iter().collect();
        out.sort();
        out
    }

    /// Returns the set of symbol indices that are referenced somewhere
    /// in the workspace.
    pub fn referenced_symbols(&self) -> HashSet<usize> {
        let mut hit: HashSet<usize> = HashSet::new();
        for r in &self.references {
            let Some(candidates) = self.by_name.get(&r.name) else {
                continue;
            };
            for &idx in candidates {
                let node = &self.symbols[idx];
                // A reference matches if:
                // - it has no root → match across workspace (best-effort)
                // - root is "crate"/"self"/"super" → match within same crate
                // - root is a workspace crate name → match that crate
                // - root is "Self" / type prefix → ignore (treat as same-crate)
                match r.root.as_deref() {
                    None => hit.insert(idx),
                    Some("crate") | Some("self") | Some("super") | Some("Self") => {
                        if node.symbol.crate_name == r.from_crate {
                            hit.insert(idx)
                        } else {
                            false
                        }
                    }
                    Some(root) => {
                        if root == node.symbol.crate_name {
                            hit.insert(idx)
                        } else {
                            // local reference within same crate (e.g. mod-relative path)
                            if node.symbol.crate_name == r.from_crate {
                                hit.insert(idx)
                            } else {
                                false
                            }
                        }
                    }
                };
            }
        }
        hit
    }
}
