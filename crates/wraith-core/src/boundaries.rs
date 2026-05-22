//! Architecture boundary enforcement.
//!
//! Each boundary rule names a `from` crate (matched by crate name or by
//! a path prefix relative to the workspace root) and `allow` / `deny`
//! lists. `allow` lists explicit import path prefixes that are permitted;
//! everything else from outside the crate's own tree is a violation.
//! `deny` overrides allow.

use crate::config::BoundaryRule;
use crate::graph::ReferenceGraph;
use crate::report::Finding;
use crate::workspace::Workspace;

fn crate_matches(rule_from: &str, crate_name: &str, crate_root_rel: &str) -> bool {
    rule_from == crate_name || crate_root_rel.starts_with(rule_from)
}

pub fn find_boundary_violations(
    ws: &Workspace,
    graph: &ReferenceGraph,
    rules: &[BoundaryRule],
) -> Vec<Finding> {
    if rules.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for c in &ws.crates {
        let crate_key = c.name.replace('-', "_");
        let rel = c
            .root_dir
            .strip_prefix(&ws.root)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let applicable: Vec<&BoundaryRule> = rules
            .iter()
            .filter(|r| crate_matches(&r.from, &c.name, &rel))
            .collect();
        if applicable.is_empty() {
            continue;
        }
        for r in &graph.references {
            if r.from_crate != crate_key {
                continue;
            }
            let Some(root) = r.root.as_deref() else {
                continue;
            };
            if matches!(root, "crate" | "self" | "super" | "Self") {
                continue;
            }
            // skip primitive / std-ish roots
            if matches!(root, "std" | "core" | "alloc") {
                continue;
            }
            let path = format!("{}::{}", root, r.name);
            for rule in &applicable {
                let denied = rule.deny.iter().any(|d| path.starts_with(d));
                let allowed = rule.allow.iter().any(|a| path.starts_with(a));
                if denied {
                    out.push(Finding::boundary(
                        r.from_file.clone(),
                        r.line,
                        &c.name,
                        &path,
                        &format!("deny {}", rule.deny.join(",")),
                    ));
                } else if !rule.allow.is_empty() && !allowed {
                    out.push(Finding::boundary(
                        r.from_file.clone(),
                        r.line,
                        &c.name,
                        &path,
                        &format!("not in allow-list ({})", rule.allow.join(",")),
                    ));
                }
            }
        }
    }
    out
}
