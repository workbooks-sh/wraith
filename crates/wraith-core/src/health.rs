//! Cyclomatic + cognitive complexity per fn.
//!
//! Cyclomatic: 1 + count of decision points (if, else-if, match arm,
//! while, for, loop, &&, ||, ?).
//! Cognitive (Sonar-style): each control-flow construct adds 1, plus
//! +1 for each level of nesting it appears in; logical operators in
//! the same expression collapse.

use crate::config::ComplexityConfig;
use crate::report::Finding;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use syn::spanned::Spanned;
use syn::visit::Visit;

pub(crate) struct ComplexityVisitor {
    cyclo: u32,
    cog: u32,
    nesting: u32,
}

impl ComplexityVisitor {
    pub(crate) fn new() -> Self {
        Self {
            cyclo: 1,
            cog: 0,
            nesting: 0,
        }
    }
    pub(crate) fn cyclo(&self) -> u32 {
        self.cyclo
    }
    pub(crate) fn cog(&self) -> u32 {
        self.cog
    }
    fn enter(&mut self) {
        self.nesting += 1;
    }
    fn exit(&mut self) {
        if self.nesting > 0 {
            self.nesting -= 1;
        }
    }
    fn add_branch(&mut self) {
        self.cyclo += 1;
        self.cog += 1 + self.nesting;
    }
}

impl<'ast> Visit<'ast> for ComplexityVisitor {
    fn visit_expr_if(&mut self, i: &'ast syn::ExprIf) {
        self.add_branch();
        self.enter();
        syn::visit::visit_expr_if(self, i);
        self.exit();
    }
    fn visit_expr_match(&mut self, i: &'ast syn::ExprMatch) {
        // each non-default arm adds a branch
        for arm in &i.arms {
            // wildcard `_` arm is not a decision; everything else is
            let is_wildcard = matches!(arm.pat, syn::Pat::Wild(_));
            if !is_wildcard {
                self.add_branch();
            }
        }
        self.enter();
        syn::visit::visit_expr_match(self, i);
        self.exit();
    }
    fn visit_expr_while(&mut self, i: &'ast syn::ExprWhile) {
        self.add_branch();
        self.enter();
        syn::visit::visit_expr_while(self, i);
        self.exit();
    }
    fn visit_expr_for_loop(&mut self, i: &'ast syn::ExprForLoop) {
        self.add_branch();
        self.enter();
        syn::visit::visit_expr_for_loop(self, i);
        self.exit();
    }
    fn visit_expr_loop(&mut self, i: &'ast syn::ExprLoop) {
        self.add_branch();
        self.enter();
        syn::visit::visit_expr_loop(self, i);
        self.exit();
    }
    fn visit_expr_binary(&mut self, i: &'ast syn::ExprBinary) {
        if matches!(i.op, syn::BinOp::And(_) | syn::BinOp::Or(_)) {
            self.cyclo += 1;
            self.cog += 1;
        }
        syn::visit::visit_expr_binary(self, i);
    }
    fn visit_expr_try(&mut self, i: &'ast syn::ExprTry) {
        self.cyclo += 1;
        syn::visit::visit_expr_try(self, i);
    }
}

struct FnWalker<'a> {
    crate_name: String,
    module_path: Vec<String>,
    file: PathBuf,
    cfg: &'a ComplexityConfig,
    out: &'a mut Vec<Finding>,
}

impl<'a, 'ast> Visit<'ast> for FnWalker<'a> {
    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        if i.ident == "tests" {
            return;
        }
        self.module_path.push(i.ident.to_string());
        syn::visit::visit_item_mod(self, i);
        self.module_path.pop();
    }
    fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
        if i.attrs.iter().any(|a| a.path().is_ident("test")) {
            return;
        }
        let mut v = ComplexityVisitor::new();
        v.visit_block(&i.block);
        let name = i.sig.ident.to_string();
        let qualified = if self.module_path.is_empty() {
            format!("{}::{}", self.crate_name, name)
        } else {
            format!(
                "{}::{}::{}",
                self.crate_name,
                self.module_path.join("::"),
                name
            )
        };
        if v.cyclo > self.cfg.cyclomatic || v.cog > self.cfg.cognitive {
            let span = syn::spanned::Spanned::span(&i.sig.ident).start();
            self.out.push(Finding::complexity(
                self.file.clone(),
                span.line,
                span.column + 1,
                &qualified,
                v.cyclo,
                v.cog,
                self.cfg.cyclomatic,
                self.cfg.cognitive,
            ));
        }
        syn::visit::visit_item_fn(self, i);
    }
}

pub fn find_complexity_hotspots(
    crate_files: &[(String, Vec<PathBuf>)],
    cfg: &ComplexityConfig,
) -> Vec<Finding> {
    let mut out = Vec::new();
    for (crate_name, files) in crate_files {
        for f in files {
            scan(crate_name, f, cfg, &mut out);
        }
    }
    out
}

fn scan(crate_name: &str, file: &Path, cfg: &ComplexityConfig, out: &mut Vec<Finding>) {
    let Ok(text) = std::fs::read_to_string(file) else {
        return;
    };
    let Ok(ast) = syn::parse_file(&text) else {
        return;
    };
    let mut w = FnWalker {
        crate_name: crate_name.to_string(),
        module_path: Vec::new(),
        file: file.to_path_buf(),
        cfg,
        out,
    };
    w.visit_file(&ast);
}

// ---------------------------------------------------------------------------
// --show-branches: print structured decision-point tree for one fn
// ---------------------------------------------------------------------------

/// A single decision point inside a fn body, with file:line and children.
#[derive(Debug, Clone)]
pub struct BranchNode {
    pub label: String,
    pub line: usize,
    pub children: Vec<BranchNode>,
}

/// Output of `health_show_branches`: the fn's qualified name, headline
/// complexity numbers, and the tree of decision points.
#[derive(Debug, Clone)]
pub struct BranchTree {
    pub qualified: String,
    pub file: PathBuf,
    pub cyclo: u32,
    pub cog: u32,
    pub roots: Vec<BranchNode>,
}

impl BranchTree {
    /// Indented human rendering, similar to the ticket example.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "{}  (cyclo={} cog={})\n",
            self.qualified, self.cyclo, self.cog
        ));
        let file_disp = self.file.display().to_string();
        for r in &self.roots {
            render_node(r, 1, &file_disp, &mut out);
        }
        out
    }
}

fn render_node(node: &BranchNode, depth: usize, file: &str, out: &mut String) {
    let indent = "  ".repeat(depth);
    // file:line on the first child of a parent; subsequent siblings get
    // just `:line` to mirror the ticket example.
    out.push_str(&format!(
        "{}{}  @ {}:{}\n",
        indent, node.label, file, node.line
    ));
    for c in &node.children {
        render_node(c, depth + 1, file, out);
    }
}

struct BranchCollector {
    nodes_stack: Vec<Vec<BranchNode>>,
    cog: u32,
    cyclo: u32,
    nesting: u32,
}

impl BranchCollector {
    fn new() -> Self {
        Self {
            nodes_stack: vec![Vec::new()],
            cog: 0,
            cyclo: 1,
            nesting: 0,
        }
    }
    fn add_leaf(&mut self, label: String, line: usize) {
        self.cyclo += 1;
        self.cog += 1 + self.nesting;
        let last = self.nodes_stack.last_mut().unwrap();
        last.push(BranchNode {
            label,
            line,
            children: Vec::new(),
        });
    }
    fn push_scope(&mut self, label: String, line: usize) {
        self.cyclo += 1;
        self.cog += 1 + self.nesting;
        self.nesting += 1;
        self.nodes_stack.push(Vec::new());
        // store the parent record on a side-stack via label/line so we
        // can attach kids on pop. We push a sentinel into the parent
        // frame and patch its children when we pop.
        let parent_frame_idx = self.nodes_stack.len() - 2;
        self.nodes_stack[parent_frame_idx].push(BranchNode {
            label,
            line,
            children: Vec::new(),
        });
    }
    fn pop_scope(&mut self) {
        let kids = self.nodes_stack.pop().unwrap();
        if let Some(parent_frame) = self.nodes_stack.last_mut() {
            if let Some(last) = parent_frame.last_mut() {
                last.children = kids;
            }
        }
        if self.nesting > 0 {
            self.nesting -= 1;
        }
    }
    fn into_roots(mut self) -> Vec<BranchNode> {
        self.nodes_stack.pop().unwrap_or_default()
    }
}

impl<'ast> Visit<'ast> for BranchCollector {
    fn visit_expr_if(&mut self, i: &'ast syn::ExprIf) {
        let line = syn::spanned::Spanned::span(&i.if_token).start().line;
        // detect `if let`
        let label = match &*i.cond {
            syn::Expr::Let(_) => "if-let".to_string(),
            _ => "if".to_string(),
        };
        self.push_scope(label, line);
        // walk body for nested branches
        syn::visit::visit_block(self, &i.then_branch);
        self.pop_scope();
        if let Some((_else_tok, else_branch)) = &i.else_branch {
            // recurse into else (chained else-if reads as another if)
            syn::visit::visit_expr(self, else_branch);
        }
    }
    fn visit_expr_match(&mut self, i: &'ast syn::ExprMatch) {
        let line = syn::spanned::Spanned::span(&i.match_token).start().line;
        self.push_scope("match".to_string(), line);
        for arm in &i.arms {
            // visit arm body to surface nested branches; the arm itself
            // is not its own node (would clutter the tree).
            syn::visit::visit_expr(self, &arm.body);
        }
        self.pop_scope();
    }
    fn visit_expr_while(&mut self, i: &'ast syn::ExprWhile) {
        let line = syn::spanned::Spanned::span(&i.while_token).start().line;
        let label = match &*i.cond {
            syn::Expr::Let(_) => "while-let".to_string(),
            _ => "while".to_string(),
        };
        self.push_scope(label, line);
        syn::visit::visit_block(self, &i.body);
        self.pop_scope();
    }
    fn visit_expr_for_loop(&mut self, i: &'ast syn::ExprForLoop) {
        let line = syn::spanned::Spanned::span(&i.for_token).start().line;
        let pat = quote::ToTokens::to_token_stream(&i.pat).to_string();
        let expr = quote::ToTokens::to_token_stream(&i.expr).to_string();
        let label = format!("for {} in {}", pat, expr);
        self.push_scope(label, line);
        syn::visit::visit_block(self, &i.body);
        self.pop_scope();
    }
    fn visit_expr_loop(&mut self, i: &'ast syn::ExprLoop) {
        let line = syn::spanned::Spanned::span(&i.loop_token).start().line;
        self.push_scope("loop".to_string(), line);
        syn::visit::visit_block(self, &i.body);
        self.pop_scope();
    }
    fn visit_expr_binary(&mut self, i: &'ast syn::ExprBinary) {
        if matches!(i.op, syn::BinOp::And(_) | syn::BinOp::Or(_)) {
            let op = if matches!(i.op, syn::BinOp::And(_)) {
                "&&"
            } else {
                "||"
            };
            let line = syn::spanned::Spanned::span(&i.op).start().line;
            self.add_leaf(op.to_string(), line);
        }
        syn::visit::visit_expr_binary(self, i);
    }
}

/// Walk every fn in `crate_files` and, for the one whose qualified path
/// equals `fn_path` (e.g. `wavelet::run_turn` or
/// `wavelet::module::name`), return its branch tree.
pub fn health_show_branches(
    crate_files: &[(String, Vec<PathBuf>)],
    fn_path: &str,
) -> Option<BranchTree> {
    for (crate_name, files) in crate_files {
        for f in files {
            if let Some(tree) = scan_for_fn(crate_name, f, fn_path) {
                return Some(tree);
            }
        }
    }
    None
}

struct FnFinder<'a> {
    crate_name: String,
    module_path: Vec<String>,
    file: PathBuf,
    target: &'a str,
    found: Option<BranchTree>,
}

impl<'a, 'ast> Visit<'ast> for FnFinder<'a> {
    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        if i.ident == "tests" {
            return;
        }
        self.module_path.push(i.ident.to_string());
        syn::visit::visit_item_mod(self, i);
        self.module_path.pop();
    }
    fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
        if self.found.is_some() {
            return;
        }
        let name = i.sig.ident.to_string();
        let qualified = if self.module_path.is_empty() {
            format!("{}::{}", self.crate_name, name)
        } else {
            format!(
                "{}::{}::{}",
                self.crate_name,
                self.module_path.join("::"),
                name
            )
        };
        // accept either fully-qualified match or trailing-segment match
        // (`run_turn` or `crate::run_turn` etc.)
        let matches = qualified == self.target
            || qualified.ends_with(&format!("::{}", self.target))
            || name == self.target;
        if !matches {
            syn::visit::visit_item_fn(self, i);
            return;
        }
        // collect branches + complexity in one pass
        let mut bc = BranchCollector::new();
        bc.visit_block(&i.block);
        let cyclo = bc.cyclo;
        let cog = bc.cog;
        let roots = bc.into_roots();
        self.found = Some(BranchTree {
            qualified,
            file: self.file.clone(),
            cyclo,
            cog,
            roots,
        });
    }
}

fn scan_for_fn(crate_name: &str, file: &Path, target: &str) -> Option<BranchTree> {
    let text = std::fs::read_to_string(file).ok()?;
    let ast = syn::parse_file(&text).ok()?;
    let mut finder = FnFinder {
        crate_name: crate_name.to_string(),
        module_path: Vec::new(),
        file: file.to_path_buf(),
        target,
        found: None,
    };
    finder.visit_file(&ast);
    finder.found
}

// ---------------------------------------------------------------------------
// --suggest-extractions: rank extractable sub-trees within a fn body so an
// AI agent can pick one and invoke `wraith refactor extract-fn`.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ExtractionSuggestion {
    pub rank: u32,
    pub file: String,
    pub line_start: usize,
    pub line_end: usize,
    pub sub_cyclo: u32,
    pub escaping: u32,
    pub captured: u32,
    pub self_containment_score: f64,
    pub suggested_name: String,
    pub extract_fn_command: String,
    /// `"ok"` for ranges extract-fn v2 will lift cleanly, or
    /// `"v2-only-with-rewrite"` for ranges that need control-flow
    /// rewriting (early-return / ? / .await / self.) at the callsite.
    pub feasibility: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExtractionSuggestions {
    pub function: String,
    pub function_cyclo: u32,
    pub function_cog: u32,
    pub suggestions: Vec<ExtractionSuggestion>,
}

impl ExtractionSuggestions {
    pub fn render_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "# Extraction suggestions for `{}`\n\n",
            self.function
        ));
        out.push_str(&format!(
            "Function complexity: cyclo={} cog={}\n\n",
            self.function_cyclo, self.function_cog
        ));
        if self.suggestions.is_empty() {
            out.push_str("_No extractable sub-trees found._\n");
            return out;
        }
        out.push_str(
            "| rank | range | sub_cyclo | escaping | captured | score | feasibility | suggested_name |\n",
        );
        out.push_str(
            "|------|-------|-----------|----------|----------|-------|-------------|----------------|\n",
        );
        for s in &self.suggestions {
            out.push_str(&format!(
                "| {} | {}:{}..{} | {} | {} | {} | {:.2} | {} | `{}` |\n",
                s.rank,
                s.file,
                s.line_start,
                s.line_end,
                s.sub_cyclo,
                s.escaping,
                s.captured,
                s.self_containment_score,
                s.feasibility,
                s.suggested_name,
            ));
        }
        out.push_str("\n## Commands\n\n");
        for s in &self.suggestions {
            out.push_str(&format!("```\n{}\n```\n", s.extract_fn_command));
        }
        out
    }
}

/// Locate a fn by qualified path, then walk its body and rank candidate
/// extractable sub-trees. Filters out candidates that the existing
/// `extract-fn` v1 refusal patterns would reject.
pub fn health_suggest_extractions(
    crate_files: &[(String, Vec<PathBuf>)],
    fn_path: &str,
    max_suggestions: usize,
) -> Option<ExtractionSuggestions> {
    for (crate_name, files) in crate_files {
        for f in files {
            if let Some(s) = scan_for_extractions(crate_name, f, fn_path, max_suggestions) {
                return Some(s);
            }
        }
    }
    None
}

fn scan_for_extractions(
    crate_name: &str,
    file: &Path,
    target: &str,
    max_suggestions: usize,
) -> Option<ExtractionSuggestions> {
    let text = std::fs::read_to_string(file).ok()?;
    let ast = syn::parse_file(&text).ok()?;
    let mut finder = ExtractFnFinder {
        crate_name: crate_name.to_string(),
        module_path: Vec::new(),
        file: file.to_path_buf(),
        target,
        max_suggestions,
        found: None,
    };
    finder.visit_file(&ast);
    finder.found
}

struct ExtractFnFinder<'a> {
    crate_name: String,
    module_path: Vec<String>,
    file: PathBuf,
    target: &'a str,
    max_suggestions: usize,
    found: Option<ExtractionSuggestions>,
}

impl<'a, 'ast> Visit<'ast> for ExtractFnFinder<'a> {
    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        if i.ident == "tests" {
            return;
        }
        self.module_path.push(i.ident.to_string());
        syn::visit::visit_item_mod(self, i);
        self.module_path.pop();
    }
    fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
        if self.found.is_some() {
            return;
        }
        let name = i.sig.ident.to_string();
        let qualified = if self.module_path.is_empty() {
            format!("{}::{}", self.crate_name, name)
        } else {
            format!(
                "{}::{}::{}",
                self.crate_name,
                self.module_path.join("::"),
                name
            )
        };
        let matches = qualified == self.target
            || qualified.ends_with(&format!("::{}", self.target))
            || name == self.target;
        if !matches {
            syn::visit::visit_item_fn(self, i);
            return;
        }

        // Headline complexity of the whole fn.
        let mut headline = ComplexityVisitor::new();
        headline.visit_block(&i.block);

        // Collect candidate sub-trees from the top-level statements of
        // the fn body. We only look at top-level candidates inside the
        // body — nested candidates would overlap with their parent and
        // produce confusing "rank me too" duplicates for the agent.
        let mut candidates: Vec<RawCandidate> = Vec::new();
        collect_candidates(&i.block.stmts, &mut candidates);

        // Score + filter. Filter logic v2 (wb-5lgj.38): instead of
        // dropping anything that v1's coarse refusal scan flagged, ask
        // the extract-fn v2 acceptor whether each range is feasible.
        // Only refused ranges are dropped; v2-only-with-rewrite ranges
        // pass through tagged so the agent knows what to expect.
        let mut suggestions: Vec<ExtractionSuggestion> = candidates
            .into_iter()
            .filter_map(|c| {
                let probe = crate::refactor::feasibility_probe(
                    &self.file,
                    c.line_start,
                    c.line_end,
                );
                let feasibility = match probe {
                    crate::refactor::Feasibility::Ok => "ok",
                    crate::refactor::Feasibility::V2OnlyWithRewrite => "v2-only-with-rewrite",
                    crate::refactor::Feasibility::Refused(_) => return None,
                };
                let denom = (c.escaping + c.captured + 1) as f64;
                let score = c.sub_cyclo as f64 / denom;
                Some(ExtractionSuggestion {
                    rank: 0,
                    file: self.file.display().to_string(),
                    line_start: c.line_start,
                    line_end: c.line_end,
                    sub_cyclo: c.sub_cyclo,
                    escaping: c.escaping,
                    captured: c.captured,
                    self_containment_score: score,
                    suggested_name: c.suggested_name.clone(),
                    extract_fn_command: format!(
                        "wraith refactor extract-fn {}:{}..{} --name {}",
                        self.file.display(),
                        c.line_start,
                        c.line_end,
                        c.suggested_name,
                    ),
                    feasibility,
                })
            })
            .collect();

        suggestions.sort_by(|a, b| {
            b.self_containment_score
                .partial_cmp(&a.self_containment_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        suggestions.truncate(max_suggestions_or_default(self.max_suggestions));
        for (idx, s) in suggestions.iter_mut().enumerate() {
            s.rank = (idx + 1) as u32;
        }

        self.found = Some(ExtractionSuggestions {
            function: qualified,
            function_cyclo: headline.cyclo,
            function_cog: headline.cog,
            suggestions,
        });
    }
}

fn max_suggestions_or_default(n: usize) -> usize {
    if n == 0 {
        10
    } else {
        n
    }
}

struct RawCandidate {
    line_start: usize,
    line_end: usize,
    sub_cyclo: u32,
    escaping: u32,
    captured: u32,
    contains_refusal_patterns: bool,
    suggested_name: String,
}

/// Walk top-level statements; for each statement that is a candidate
/// control-flow expression (or expression-statement wrapping one),
/// produce a RawCandidate. Bindings accumulated from prior statements
/// drive the escaping/captured counts.
fn collect_candidates<'ast>(stmts: &'ast [syn::Stmt], out: &mut Vec<RawCandidate>) {
    let mut bound_so_far: BTreeSet<String> = BTreeSet::new();
    for s in stmts {
        if let Some(cand) = candidate_from_stmt(s, &bound_so_far) {
            let was_refused = cand.contains_refusal_patterns;
            out.push(cand);
            // If the top-level candidate was refused (return/?/self./etc.),
            // descend one level into its sub-bodies — match arms, if/else
            // branches, loop bodies — to surface clean inner candidates
            // that an agent can act on.
            if was_refused {
                if let syn::Stmt::Expr(expr, _) = s {
                    descend_into_expr(expr, &bound_so_far, out);
                }
            }
        }
        if let syn::Stmt::Local(local) = s {
            collect_pat_idents(&local.pat, &mut bound_so_far);
        }
    }
}

/// Walk one level deeper inside a refused candidate. For each immediate
/// sub-block (match arm, if/else block, loop body), look for a contained
/// control-flow expression-statement and treat it as a candidate.
fn descend_into_expr(
    expr: &syn::Expr,
    bound_before: &BTreeSet<String>,
    out: &mut Vec<RawCandidate>,
) {
    match expr {
        syn::Expr::Match(m) => {
            for arm in &m.arms {
                descend_collect_block_like(&arm.body, bound_before, out);
            }
        }
        syn::Expr::If(i) => {
            collect_candidates(&i.then_branch.stmts, out);
            if let Some((_, else_expr)) = &i.else_branch {
                descend_into_expr(else_expr, bound_before, out);
            }
        }
        syn::Expr::ForLoop(f) => collect_candidates(&f.body.stmts, out),
        syn::Expr::While(w) => collect_candidates(&w.body.stmts, out),
        syn::Expr::Loop(l) => collect_candidates(&l.body.stmts, out),
        syn::Expr::Block(b) => collect_candidates(&b.block.stmts, out),
        _ => {}
    }
}

fn descend_collect_block_like(
    expr: &syn::Expr,
    bound_before: &BTreeSet<String>,
    out: &mut Vec<RawCandidate>,
) {
    match expr {
        syn::Expr::Block(b) => collect_candidates(&b.block.stmts, out),
        // arm body might directly be an if / match / for / while / loop
        syn::Expr::If(_)
        | syn::Expr::Match(_)
        | syn::Expr::ForLoop(_)
        | syn::Expr::While(_)
        | syn::Expr::Loop(_) => {
            // Treat as a one-statement "block" by synthesizing a stmt view.
            let fake_stmt =
                syn::Stmt::Expr(expr.clone(), None);
            if let Some(cand) = candidate_from_stmt(&fake_stmt, bound_before) {
                let was_refused = cand.contains_refusal_patterns;
                out.push(cand);
                if was_refused {
                    descend_into_expr(expr, bound_before, out);
                }
            }
        }
        _ => {}
    }
}

fn candidate_from_stmt(s: &syn::Stmt, bound_before: &BTreeSet<String>) -> Option<RawCandidate> {
    let expr = match s {
        syn::Stmt::Expr(e, _) => e,
        _ => return None,
    };

    // Determine sub-tree extents + complexity + flow per expression kind.
    let (line_start, line_end, sub_cyclo, contains_refusal_patterns, flow, kind, ident_hint) =
        match expr {
            syn::Expr::If(i) => {
                let line_start = i.if_token.span().start().line;
                let line_end = expr.span().end().line;
                let mut cv = ComplexityVisitor::new();
                cv.visit_expr_if(i);
                let mut flow = RangeFlow::default();
                flow.visit_expr_if(i);
                let refusal = scan_refusal_expr(expr);
                (
                    line_start,
                    line_end,
                    cv.cyclo,
                    refusal,
                    flow,
                    "handle",
                    first_ident_in_expr(&i.cond),
                )
            }
            syn::Expr::Match(m) => {
                let line_start = m.match_token.span().start().line;
                let line_end = expr.span().end().line;
                let mut cv = ComplexityVisitor::new();
                cv.visit_expr_match(m);
                let mut flow = RangeFlow::default();
                flow.visit_expr_match(m);
                let refusal = scan_refusal_expr(expr);
                (
                    line_start,
                    line_end,
                    cv.cyclo,
                    refusal,
                    flow,
                    "process",
                    first_ident_in_expr(&m.expr),
                )
            }
            syn::Expr::ForLoop(f) => {
                let line_start = f.for_token.span().start().line;
                let line_end = expr.span().end().line;
                let mut cv = ComplexityVisitor::new();
                cv.visit_expr_for_loop(f);
                let mut flow = RangeFlow::default();
                flow.visit_expr_for_loop(f);
                let refusal = scan_refusal_expr(expr);
                let hint =
                    first_ident_in_expr(&f.expr).or_else(|| first_ident_in_pat(&f.pat));
                (
                    line_start,
                    line_end,
                    cv.cyclo,
                    refusal,
                    flow,
                    "process",
                    hint,
                )
            }
            syn::Expr::While(w) => {
                let line_start = w.while_token.span().start().line;
                let line_end = expr.span().end().line;
                let mut cv = ComplexityVisitor::new();
                cv.visit_expr_while(w);
                let mut flow = RangeFlow::default();
                flow.visit_expr_while(w);
                let refusal = scan_refusal_expr(expr);
                (
                    line_start,
                    line_end,
                    cv.cyclo,
                    refusal,
                    flow,
                    "process_while",
                    first_ident_in_expr(&w.cond),
                )
            }
            syn::Expr::Loop(l) => {
                let line_start = l.loop_token.span().start().line;
                let line_end = expr.span().end().line;
                let mut cv = ComplexityVisitor::new();
                cv.visit_expr_loop(l);
                let mut flow = RangeFlow::default();
                flow.visit_expr_loop(l);
                let refusal = scan_refusal_expr(expr);
                (line_start, line_end, cv.cyclo, refusal, flow, "run_loop", None)
            }
            _ => return None,
        };

    if line_end <= line_start {
        return None;
    }

    let mut captured: u32 = 0;
    let mut escaping: u32 = 0;
    for name in bound_before {
        let read = flow.reads.contains(name);
        let write = flow.writes.contains(name);
        if write {
            escaping += 1;
        } else if read {
            captured += 1;
        }
    }

    let suggested_name = suggest_name(kind, ident_hint.as_deref(), line_start);

    Some(RawCandidate {
        line_start,
        line_end,
        sub_cyclo,
        escaping,
        captured,
        contains_refusal_patterns,
        suggested_name,
    })
}

/// Refusal scan over an arbitrary expression sub-tree.
fn scan_refusal_expr(expr: &syn::Expr) -> bool {
    struct V {
        found: bool,
    }
    impl<'ast> Visit<'ast> for V {
        fn visit_expr_return(&mut self, _: &'ast syn::ExprReturn) {
            self.found = true;
        }
        fn visit_expr_break(&mut self, _: &'ast syn::ExprBreak) {
            self.found = true;
        }
        fn visit_expr_continue(&mut self, _: &'ast syn::ExprContinue) {
            self.found = true;
        }
        fn visit_expr_try(&mut self, _: &'ast syn::ExprTry) {
            self.found = true;
        }
        fn visit_expr_await(&mut self, _: &'ast syn::ExprAwait) {
            self.found = true;
        }
        fn visit_expr_field(&mut self, f: &'ast syn::ExprField) {
            if let syn::Expr::Path(p) = &*f.base {
                if p.path.is_ident("self") {
                    self.found = true;
                }
            }
            syn::visit::visit_expr_field(self, f);
        }
        fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
            if let syn::Expr::Path(p) = &*m.receiver {
                if p.path.is_ident("self") {
                    self.found = true;
                }
            }
            syn::visit::visit_expr_method_call(self, m);
        }
    }
    let mut v = V { found: false };
    v.visit_expr(expr);
    v.found
}

#[derive(Default)]
struct RangeFlow {
    reads: BTreeSet<String>,
    writes: BTreeSet<String>,
}

impl RangeFlow {
    fn lhs_ident(expr: &syn::Expr) -> Option<String> {
        match expr {
            syn::Expr::Path(p) if p.qself.is_none() && p.path.segments.len() == 1 => {
                Some(p.path.segments[0].ident.to_string())
            }
            _ => None,
        }
    }
}

impl<'ast> Visit<'ast> for RangeFlow {
    fn visit_expr_path(&mut self, i: &'ast syn::ExprPath) {
        if i.qself.is_none() && i.path.segments.len() == 1 {
            self.reads.insert(i.path.segments[0].ident.to_string());
        }
    }
    fn visit_expr_assign(&mut self, i: &'ast syn::ExprAssign) {
        if let Some(name) = Self::lhs_ident(&i.left) {
            self.writes.insert(name);
        } else {
            self.visit_expr(&i.left);
        }
        self.visit_expr(&i.right);
    }
    fn visit_expr_binary(&mut self, i: &'ast syn::ExprBinary) {
        let compound = matches!(
            i.op,
            syn::BinOp::AddAssign(_)
                | syn::BinOp::SubAssign(_)
                | syn::BinOp::MulAssign(_)
                | syn::BinOp::DivAssign(_)
                | syn::BinOp::RemAssign(_)
                | syn::BinOp::BitAndAssign(_)
                | syn::BinOp::BitOrAssign(_)
                | syn::BinOp::BitXorAssign(_)
                | syn::BinOp::ShlAssign(_)
                | syn::BinOp::ShrAssign(_)
        );
        if compound {
            if let Some(name) = Self::lhs_ident(&i.left) {
                self.writes.insert(name);
                self.visit_expr(&i.right);
                return;
            }
        }
        syn::visit::visit_expr_binary(self, i);
    }
    fn visit_expr_reference(&mut self, i: &'ast syn::ExprReference) {
        if i.mutability.is_some() {
            if let Some(name) = Self::lhs_ident(&i.expr) {
                self.writes.insert(name);
            }
        }
        syn::visit::visit_expr_reference(self, i);
    }
    fn visit_expr_method_call(&mut self, i: &'ast syn::ExprMethodCall) {
        self.visit_expr(&i.receiver);
        for a in &i.args {
            self.visit_expr(a);
        }
    }
    fn visit_expr_call(&mut self, i: &'ast syn::ExprCall) {
        if !matches!(&*i.func, syn::Expr::Path(_)) {
            self.visit_expr(&i.func);
        }
        for a in &i.args {
            self.visit_expr(a);
        }
    }
}

fn collect_pat_idents(p: &syn::Pat, out: &mut BTreeSet<String>) {
    match p {
        syn::Pat::Ident(pi) => {
            out.insert(pi.ident.to_string());
        }
        syn::Pat::Tuple(t) => {
            for el in &t.elems {
                collect_pat_idents(el, out);
            }
        }
        syn::Pat::TupleStruct(ts) => {
            for el in &ts.elems {
                collect_pat_idents(el, out);
            }
        }
        syn::Pat::Struct(ps) => {
            for f in &ps.fields {
                collect_pat_idents(&f.pat, out);
            }
        }
        syn::Pat::Reference(r) => collect_pat_idents(&r.pat, out),
        syn::Pat::Type(t) => collect_pat_idents(&t.pat, out),
        _ => {}
    }
}

fn first_ident_in_expr(expr: &syn::Expr) -> Option<String> {
    struct V {
        first: Option<String>,
    }
    impl<'ast> Visit<'ast> for V {
        fn visit_ident(&mut self, i: &'ast syn::Ident) {
            if self.first.is_none() {
                let s = i.to_string();
                if !is_keyword_or_common(&s) {
                    self.first = Some(s);
                }
            }
        }
    }
    let mut v = V { first: None };
    v.visit_expr(expr);
    v.first
}

fn first_ident_in_pat(pat: &syn::Pat) -> Option<String> {
    struct V {
        first: Option<String>,
    }
    impl<'ast> Visit<'ast> for V {
        fn visit_ident(&mut self, i: &'ast syn::Ident) {
            if self.first.is_none() {
                let s = i.to_string();
                if !is_keyword_or_common(&s) {
                    self.first = Some(s);
                }
            }
        }
    }
    let mut v = V { first: None };
    v.visit_pat(pat);
    v.first
}

fn is_keyword_or_common(s: &str) -> bool {
    matches!(
        s,
        "let" | "mut" | "true" | "false" | "self" | "Self" | "ref" | "_"
    )
}

fn suggest_name(kind: &str, ident_hint: Option<&str>, line: usize) -> String {
    match ident_hint {
        Some(ident) => format!("{}_{}", kind, ident),
        None => format!("extract_block_{}", line),
    }
}
