//! Safe auto-fix. Removes dead pub items and unused deps that the
//! resolver is certain about. Default --dry-run.

use crate::report::{Finding, FindingKind};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, serde::Serialize)]
pub struct FixPlan {
    pub edits: Vec<FileEdit>,
}

#[derive(Debug, serde::Serialize)]
pub struct FileEdit {
    pub file: PathBuf,
    pub kind: EditKind,
    pub description: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(tag = "edit_kind", rename_all = "kebab-case")]
pub enum EditKind {
    DeleteLines { line: usize },
    RemoveCargoDep { dep_name: String, dep_kind: String },
}

pub fn plan(findings: &[Finding]) -> FixPlan {
    let mut edits = Vec::new();
    for f in findings {
        match &f.kind {
            FindingKind::DeadCode { symbol, .. } => {
                edits.push(FileEdit {
                    file: f.file.clone(),
                    kind: EditKind::DeleteLines { line: f.line },
                    description: format!("remove dead item `{}`", symbol),
                });
            }
            FindingKind::UnusedDep {
                dep_name, dep_kind, ..
            } => {
                edits.push(FileEdit {
                    file: f.file.clone(),
                    kind: EditKind::RemoveCargoDep {
                        dep_name: dep_name.clone(),
                        dep_kind: dep_kind.clone(),
                    },
                    description: format!("remove {} dep `{}`", dep_kind, dep_name),
                });
            }
            _ => {}
        }
    }
    FixPlan { edits }
}

/// Apply a plan in-place. Returns count of edits applied.
pub fn apply(plan: &FixPlan) -> anyhow::Result<usize> {
    let mut applied = 0;

    // Group line deletions by file (delete from highest line down).
    let mut deletions_by_file: HashMap<PathBuf, Vec<usize>> = HashMap::new();
    let mut dep_removals: Vec<(PathBuf, String)> = Vec::new();

    for e in &plan.edits {
        match &e.kind {
            EditKind::DeleteLines { line } => {
                deletions_by_file
                    .entry(e.file.clone())
                    .or_default()
                    .push(*line);
            }
            EditKind::RemoveCargoDep { dep_name, .. } => {
                dep_removals.push((e.file.clone(), dep_name.clone()));
            }
        }
    }

    for (file, mut lines) in deletions_by_file {
        lines.sort();
        lines.dedup();
        applied += apply_line_deletions(&file, &lines)?;
    }
    for (manifest, dep) in dep_removals {
        applied += remove_cargo_dep(&manifest, &dep)?;
    }
    Ok(applied)
}

fn apply_line_deletions(file: &Path, lines: &[usize]) -> anyhow::Result<usize> {
    let text = std::fs::read_to_string(file)?;
    let src_lines: Vec<&str> = text.lines().collect();
    let drop_set: std::collections::HashSet<usize> =
        lines.iter().copied().collect();

    // For each target line, expand the deletion to cover the full item:
    // walk forward while brace-depth > 0 starting from that line.
    let mut deleted: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for &start in lines {
        if start == 0 || start > src_lines.len() {
            continue;
        }
        let mut depth: i32 = 0;
        let mut i = start - 1; // 0-indexed
        let mut started = false;
        while i < src_lines.len() {
            for ch in src_lines[i].chars() {
                if ch == '{' {
                    depth += 1;
                    started = true;
                } else if ch == '}' {
                    depth -= 1;
                }
            }
            deleted.insert(i);
            // For const / static — no braces; one line is enough.
            if !started && src_lines[i].trim_end().ends_with(';') {
                break;
            }
            if started && depth <= 0 {
                break;
            }
            i += 1;
        }
    }

    let kept: Vec<&str> = src_lines
        .iter()
        .enumerate()
        .filter(|(i, _)| !deleted.contains(i))
        .map(|(_, l)| *l)
        .collect();
    let mut out = kept.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    std::fs::write(file, out)?;
    let _ = drop_set;
    Ok(deleted.len())
}

fn remove_cargo_dep(manifest: &Path, dep: &str) -> anyhow::Result<usize> {
    let text = std::fs::read_to_string(manifest)?;
    let mut doc: toml::Value = toml::from_str(&text)?;
    let mut removed = 0;
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = doc.get_mut(section).and_then(|v| v.as_table_mut()) {
            if table.remove(dep).is_some() {
                removed += 1;
            }
        }
    }
    if removed > 0 {
        let new_text = toml::to_string_pretty(&doc)?;
        std::fs::write(manifest, new_text)?;
    }
    Ok(removed)
}
