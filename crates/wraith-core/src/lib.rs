//! wraith-core — Rust codebase analyzer engine.
//!
//! Builds a per-workspace symbol + reference graph from `syn`-parsed
//! source. Used by `wraith-cli` to find dead code, unused deps,
//! circular deps, duplicates, complexity hotspots, boundary
//! violations, and incremental audit findings.

pub mod config;
pub mod graph;
pub mod workspace;
pub mod parse;
pub mod cache;
pub mod analyze;
pub mod report;
pub mod audit;
pub mod circular;
pub mod dupes;
pub mod health;
pub mod boundaries;
pub mod fix;
pub mod refactor;
pub mod dedupe;
pub mod queries;
pub mod visibility;
pub mod diff_cluster;
pub mod extract_shared;
pub mod refactor_shared;
pub mod move_fn;
pub mod rename;
pub mod inline;
pub mod split_fn;

pub use config::Config;
pub use graph::{ReferenceGraph, Symbol, SymbolKind};
pub use workspace::{Workspace, CrateInfo};
pub use report::{Finding, FindingKind, Severity, SCHEMA_VERSION};
