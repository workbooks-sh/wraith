use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::graph::SymbolKind;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum FindingKind {
    DeadCode {
        symbol: String,
        symbol_kind: String,
        reason: String,
    },
    UnusedDep {
        crate_name: String,
        dep_name: String,
        dep_kind: String,
    },
    CircularDep {
        scope: String,
        cycle: Vec<String>,
    },
    Duplicate {
        symbol_a: String,
        symbol_b: String,
        similarity: f32,
        token_count: usize,
    },
    DuplicateCluster {
        members: Vec<ClusterMember>,
        min_similarity: f32,
        max_similarity: f32,
    },
    Complexity {
        symbol: String,
        cyclomatic: u32,
        cognitive: u32,
        threshold_cyclo: u32,
        threshold_cog: u32,
    },
    BoundaryViolation {
        from_crate: String,
        to_path: String,
        rule: String,
    },
    /// Findings sourced from fallow (TS/JS analysis) — wrapped opaquely so
    /// the integration layer (wb-5lgj.18) doesn't need wraith-core to know
    /// the fallow finding shape in detail.
    External {
        source: String,
        category: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterMember {
    pub symbol: String,
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub schema_version: u32,
    pub file: PathBuf,
    pub line: usize,
    pub col: usize,
    pub severity: Severity,
    pub kind: FindingKind,
}

impl Finding {
    fn new(file: PathBuf, line: usize, col: usize, severity: Severity, kind: FindingKind) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            file,
            line,
            col,
            severity,
            kind,
        }
    }

    pub fn dead_code(
        file: PathBuf,
        line: usize,
        col: usize,
        sym: &str,
        kind: SymbolKind,
        reason: &str,
    ) -> Self {
        Self::new(
            file,
            line,
            col,
            Severity::Warning,
            FindingKind::DeadCode {
                symbol: sym.to_string(),
                symbol_kind: kind.as_str().to_string(),
                reason: reason.to_string(),
            },
        )
    }

    pub fn unused_dep(file: PathBuf, crate_name: &str, dep: &str, dep_kind: &str) -> Self {
        Self::new(
            file,
            0,
            0,
            Severity::Warning,
            FindingKind::UnusedDep {
                crate_name: crate_name.to_string(),
                dep_name: dep.to_string(),
                dep_kind: dep_kind.to_string(),
            },
        )
    }

    pub fn circular(file: PathBuf, scope: &str, cycle: Vec<String>, severity: Severity) -> Self {
        Self::new(
            file,
            0,
            0,
            severity,
            FindingKind::CircularDep {
                scope: scope.to_string(),
                cycle,
            },
        )
    }

    pub fn duplicate(
        file: PathBuf,
        line: usize,
        col: usize,
        a: &str,
        b: &str,
        similarity: f32,
        tokens: usize,
    ) -> Self {
        Self::new(
            file,
            line,
            col,
            Severity::Warning,
            FindingKind::Duplicate {
                symbol_a: a.to_string(),
                symbol_b: b.to_string(),
                similarity,
                token_count: tokens,
            },
        )
    }

    pub fn duplicate_cluster(
        file: PathBuf,
        line: usize,
        col: usize,
        members: Vec<ClusterMember>,
        min_similarity: f32,
        max_similarity: f32,
    ) -> Self {
        Self::new(
            file,
            line,
            col,
            Severity::Warning,
            FindingKind::DuplicateCluster {
                members,
                min_similarity,
                max_similarity,
            },
        )
    }

    pub fn complexity(
        file: PathBuf,
        line: usize,
        col: usize,
        symbol: &str,
        cyclo: u32,
        cog: u32,
        t_cyclo: u32,
        t_cog: u32,
    ) -> Self {
        Self::new(
            file,
            line,
            col,
            Severity::Warning,
            FindingKind::Complexity {
                symbol: symbol.to_string(),
                cyclomatic: cyclo,
                cognitive: cog,
                threshold_cyclo: t_cyclo,
                threshold_cog: t_cog,
            },
        )
    }

    pub fn boundary(file: PathBuf, line: usize, from: &str, to: &str, rule: &str) -> Self {
        Self::new(
            file,
            line,
            0,
            Severity::Error,
            FindingKind::BoundaryViolation {
                from_crate: from.to_string(),
                to_path: to.to_string(),
                rule: rule.to_string(),
            },
        )
    }

    pub fn external(
        file: PathBuf,
        line: usize,
        col: usize,
        source: &str,
        category: &str,
        message: &str,
        severity: Severity,
    ) -> Self {
        Self::new(
            file,
            line,
            col,
            severity,
            FindingKind::External {
                source: source.to_string(),
                category: category.to_string(),
                message: message.to_string(),
            },
        )
    }

    pub fn render_human(&self) -> String {
        match &self.kind {
            FindingKind::DeadCode {
                symbol,
                symbol_kind,
                reason,
            } => format!(
                "{}:{}:{} {} (kind={}) reason=\"{}\"",
                self.file.display(),
                self.line,
                self.col,
                symbol,
                symbol_kind,
                reason
            ),
            FindingKind::UnusedDep {
                crate_name,
                dep_name,
                dep_kind,
            } => format!(
                "{}: unused {} dependency `{}` in crate `{}`",
                self.file.display(),
                dep_kind,
                dep_name,
                crate_name
            ),
            FindingKind::CircularDep { scope, cycle } => format!(
                "circular {} cycle: {}",
                scope,
                cycle.join(" -> ")
            ),
            FindingKind::Duplicate {
                symbol_a,
                symbol_b,
                similarity,
                token_count,
            } => format!(
                "{}: duplicate code — `{}` ≈ `{}` (similarity={:.2}, tokens={})",
                self.file.display(),
                symbol_a,
                symbol_b,
                similarity,
                token_count
            ),
            FindingKind::DuplicateCluster {
                members,
                min_similarity,
                max_similarity,
            } => {
                let mut s = format!(
                    "duplicate cluster (similarity {:.2}-{:.2}, {} members):",
                    min_similarity,
                    max_similarity,
                    members.len()
                );
                for m in members {
                    s.push_str(&format!("\n  {}  @ {}:{}", m.symbol, m.file, m.line));
                }
                s.push_str("\n  suggests extraction into shared utility");
                s
            }
            FindingKind::Complexity {
                symbol,
                cyclomatic,
                cognitive,
                threshold_cyclo,
                threshold_cog,
            } => format!(
                "{}:{}:{} `{}` complexity cyclo={} cog={} (thresholds {}/{})",
                self.file.display(),
                self.line,
                self.col,
                symbol,
                cyclomatic,
                cognitive,
                threshold_cyclo,
                threshold_cog
            ),
            FindingKind::BoundaryViolation {
                from_crate,
                to_path,
                rule,
            } => format!(
                "{}:{} boundary violation: `{}` imports `{}` (rule: {})",
                self.file.display(),
                self.line,
                from_crate,
                to_path,
                rule
            ),
            FindingKind::External {
                source,
                category,
                message,
            } => format!(
                "{}:{}:{} [{}/{}] {}",
                self.file.display(),
                self.line,
                self.col,
                source,
                category,
                message
            ),
        }
    }
}
