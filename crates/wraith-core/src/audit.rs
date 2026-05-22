use crate::analyze::{find_dead_code, find_unused_deps};
use crate::config::Config;
use crate::report::Finding;
use crate::workspace::Workspace;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Returns paths (relative to git root) reported by `git diff --name-only`
/// plus any untracked .rs files. Falls back to empty if git is missing.
pub fn changed_files(root: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();

    let runs = [
        vec!["diff", "--name-only", "HEAD"],
        vec!["diff", "--name-only", "--cached"],
        vec!["ls-files", "--others", "--exclude-standard"],
    ];
    for args in runs {
        let out = Command::new("git")
            .args(&args)
            .current_dir(root)
            .output();
        let Ok(out) = out else { continue };
        if !out.status.success() {
            continue;
        }
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let l = line.trim();
            if l.is_empty() {
                continue;
            }
            paths.push(root.join(l));
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

/// The set of crate names impacted by the given changed paths.
pub fn impacted_crates(ws: &Workspace, changed: &[PathBuf]) -> HashSet<String> {
    let mut out = HashSet::new();
    for ch in changed {
        for c in &ws.crates {
            if ch.starts_with(&c.root_dir) {
                out.insert(c.name.clone());
                break;
            }
        }
    }
    out
}

/// Run a subset audit: dead-code + unused-deps on impacted crates only.
pub fn run_audit(ws: &Workspace, cfg: &Config) -> anyhow::Result<Vec<Finding>> {
    let changed = changed_files(&ws.root);
    let impacted = impacted_crates(ws, &changed);

    let graph = crate::analyze::build_graph(ws)?;
    let mut dead = find_dead_code(&graph, cfg);
    let mut unused = find_unused_deps(ws, &graph, cfg);

    if !impacted.is_empty() {
        // Filter dead findings to those whose file is inside an impacted crate.
        dead.retain(|f| {
            ws.crates.iter().any(|c| {
                impacted.contains(&c.name) && f.file.starts_with(&c.root_dir)
            })
        });
        unused.retain(|f| {
            ws.crates.iter().any(|c| impacted.contains(&c.name) && f.file == c.manifest_path)
        });
    }

    let mut all = dead;
    all.extend(unused);
    Ok(all)
}
