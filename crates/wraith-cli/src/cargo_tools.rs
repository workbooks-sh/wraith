//! `wraith deps` family: crate-graph logistics that live a layer above
//! wraith-core's syntactic analyzer. We parse `Cargo.lock` directly and
//! optionally shell out to `cargo-audit` / `cargo-bloat`.
//!
//! All subcommands emit our standard `Finding` schema via the `External`
//! variant (source = "wraith-deps", category = "duplicates" | "audit" |
//! "unused-features" | "size") so downstream tooling stays uniform.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use wraith_core::report::{Finding, Severity};

#[derive(Copy, Clone, Debug)]
pub enum DepsFormat {
    Human,
    Json,
    Md,
}

// ───────────────────────── Cargo.lock parsing ─────────────────────────

#[derive(Debug, Deserialize)]
struct LockFile {
    #[serde(default)]
    package: Vec<LockPackage>,
}

#[derive(Debug, Deserialize, Clone)]
struct LockPackage {
    name: String,
    version: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    dependencies: Vec<String>,
}

fn load_lockfile(root: &Path) -> Result<LockFile> {
    let path = root.join("Cargo.lock");
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;
    let lock: LockFile = toml::from_str(&text)
        .with_context(|| format!("parsing {}", path.display()))?;
    Ok(lock)
}

// ───────────────────────── duplicates ─────────────────────────

#[derive(Debug, Serialize)]
pub struct DuplicateRecord {
    pub crate_name: String,
    pub versions: Vec<String>,
    pub callers: BTreeMap<String, Vec<String>>, // version -> sorted dependents
}

pub fn duplicates(root: &Path) -> Result<Vec<DuplicateRecord>> {
    let lock = load_lockfile(root)?;

    // Group by name; only registry/git packages — skip workspace-path crates
    // which are inherently single-version.
    let mut by_name: BTreeMap<String, Vec<&LockPackage>> = BTreeMap::new();
    for pkg in &lock.package {
        if pkg.source.is_none() {
            // path-only (workspace member) — never a duplicate by definition
            continue;
        }
        by_name.entry(pkg.name.clone()).or_default().push(pkg);
    }

    let mut out = Vec::new();
    for (name, pkgs) in by_name {
        if pkgs.len() < 2 {
            continue;
        }
        let versions: Vec<String> = pkgs.iter().map(|p| p.version.clone()).collect();
        let mut callers: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for v in &versions {
            callers.insert(v.clone(), Vec::new());
        }
        // dependency strings look like `name`, `name 1.2.3`, or `name 1.2.3 (registry+…)`.
        for other in &lock.package {
            for dep in &other.dependencies {
                let mut it = dep.split_whitespace();
                let dep_name = match it.next() {
                    Some(n) => n,
                    None => continue,
                };
                if dep_name != name {
                    continue;
                }
                let dep_ver = it.next();
                match dep_ver {
                    Some(v) => {
                        if let Some(list) = callers.get_mut(v) {
                            list.push(format!("{}@{}", other.name, other.version));
                        }
                    }
                    None => {
                        // unversioned dep ref — pinned by lockfile resolution; we
                        // can't disambiguate, attribute to all versions.
                        for list in callers.values_mut() {
                            list.push(format!("{}@{}", other.name, other.version));
                        }
                    }
                }
            }
        }
        for list in callers.values_mut() {
            list.sort();
            list.dedup();
        }
        out.push(DuplicateRecord {
            crate_name: name,
            versions,
            callers,
        });
    }
    Ok(out)
}

#[allow(dead_code)]
pub fn duplicates_findings(records: &[DuplicateRecord]) -> Vec<Finding> {
    records
        .iter()
        .map(|r| {
            let caller_summary: Vec<String> = r
                .callers
                .iter()
                .map(|(v, list)| format!("{}={}", v, list.len()))
                .collect();
            let msg = format!(
                "`{}` resolved at {} versions ({}); callers: [{}]",
                r.crate_name,
                r.versions.len(),
                r.versions.join(", "),
                caller_summary.join(", ")
            );
            Finding::external(
                PathBuf::from("Cargo.lock"),
                0,
                0,
                "wraith-deps",
                "duplicates",
                &msg,
                Severity::Warning,
            )
        })
        .collect()
}

pub fn print_duplicates(records: &[DuplicateRecord], format: DepsFormat) {
    match format {
        DepsFormat::Json => {
            println!("{}", serde_json::to_string_pretty(records).unwrap());
        }
        DepsFormat::Md => {
            println!("# Duplicate dependencies\n");
            if records.is_empty() {
                println!("None.");
                return;
            }
            println!("| crate | versions | callers (count per version) |");
            println!("| --- | --- | --- |");
            for r in records {
                let caller_summary: Vec<String> = r
                    .callers
                    .iter()
                    .map(|(v, list)| format!("{}: {}", v, list.len()))
                    .collect();
                println!(
                    "| `{}` | {} | {} |",
                    r.crate_name,
                    r.versions.join(", "),
                    caller_summary.join("; ")
                );
            }
        }
        DepsFormat::Human => {
            if records.is_empty() {
                println!("no duplicate dependencies.");
                return;
            }
            for r in records {
                println!(
                    "{} → {} versions: {}",
                    r.crate_name,
                    r.versions.len(),
                    r.versions.join(", ")
                );
                for (v, list) in &r.callers {
                    println!("  {} ← {} caller(s)", v, list.len());
                    for c in list {
                        println!("    {}", c);
                    }
                }
            }
            println!("\n{} duplicate(s).", records.len());
        }
    }
}

// ───────────────────────── audit (cargo-audit shim) ─────────────────────────

#[derive(Debug, Deserialize)]
struct CargoAuditOutput {
    #[serde(default)]
    vulnerabilities: AuditVulns,
}

#[derive(Debug, Deserialize, Default)]
struct AuditVulns {
    #[serde(default)]
    list: Vec<AuditVuln>,
}

#[derive(Debug, Deserialize)]
struct AuditVuln {
    advisory: AuditAdvisory,
    package: AuditPackage,
}

#[derive(Debug, Deserialize)]
struct AuditAdvisory {
    id: String,
    title: String,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AuditPackage {
    name: String,
    version: String,
}

#[derive(Debug, Serialize)]
pub struct AuditRecord {
    pub crate_name: String,
    pub version: String,
    pub advisory_id: String,
    pub title: String,
    pub url: Option<String>,
}

pub fn audit(root: &Path) -> Result<Vec<AuditRecord>> {
    if which("cargo-audit").is_none() {
        return Err(anyhow!(
            "cargo-audit not found on PATH. Install with `cargo install cargo-audit` and re-run."
        ));
    }
    let out = Command::new("cargo-audit")
        .arg("audit")
        .arg("--json")
        .current_dir(root)
        .output()
        .context("invoking cargo-audit")?;
    // cargo-audit exits non-zero when vulns found; we still parse stdout.
    let stdout = String::from_utf8_lossy(&out.stdout);
    if stdout.trim().is_empty() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow!(
            "cargo-audit produced no JSON output. stderr: {}",
            stderr
        ));
    }
    parse_audit_json(&stdout)
}

pub fn parse_audit_json(s: &str) -> Result<Vec<AuditRecord>> {
    let parsed: CargoAuditOutput =
        serde_json::from_str(s).context("parsing cargo-audit JSON output")?;
    Ok(parsed
        .vulnerabilities
        .list
        .into_iter()
        .map(|v| AuditRecord {
            crate_name: v.package.name,
            version: v.package.version,
            advisory_id: v.advisory.id,
            title: v.advisory.title,
            url: v.advisory.url,
        })
        .collect())
}

#[allow(dead_code)]
pub fn audit_findings(records: &[AuditRecord]) -> Vec<Finding> {
    records
        .iter()
        .map(|r| {
            let msg = format!(
                "{} {}@{}: {}{}",
                r.advisory_id,
                r.crate_name,
                r.version,
                r.title,
                r.url.as_ref().map(|u| format!(" ({})", u)).unwrap_or_default()
            );
            Finding::external(
                PathBuf::from("Cargo.lock"),
                0,
                0,
                "wraith-deps",
                "audit",
                &msg,
                Severity::Error,
            )
        })
        .collect()
}

pub fn print_audit(records: &[AuditRecord], format: DepsFormat) {
    match format {
        DepsFormat::Json => {
            println!("{}", serde_json::to_string_pretty(records).unwrap());
        }
        DepsFormat::Md => {
            println!("# Security audit\n");
            if records.is_empty() {
                println!("No known advisories.");
                return;
            }
            println!("| advisory | crate | version | title |");
            println!("| --- | --- | --- | --- |");
            for r in records {
                println!(
                    "| `{}` | `{}` | {} | {} |",
                    r.advisory_id, r.crate_name, r.version, r.title
                );
            }
        }
        DepsFormat::Human => {
            if records.is_empty() {
                println!("no advisories.");
                return;
            }
            for r in records {
                println!(
                    "{}  {}@{}  {}",
                    r.advisory_id, r.crate_name, r.version, r.title
                );
                if let Some(u) = &r.url {
                    println!("  {}", u);
                }
            }
            println!("\n{} advisor{}.", records.len(), if records.len() == 1 { "y" } else { "ies" });
        }
    }
}

// ───────────────────────── unused-features ─────────────────────────
//
// v1 heuristic: for each workspace crate, read its `[dependencies]` and
// `[dependencies.<name>] features = […]`. For each enabled feature, grep the
// crate's `src/` for any string that mentions either the feature name or
// known gated re-exports. If no hit → flag with `verify-manually` because
// the gated API surface can rename items.
//
// Conservative on purpose: this is a hint, not a fixer.

#[derive(Debug, Serialize)]
pub struct UnusedFeatureRecord {
    pub crate_name: String,
    pub dep_name: String,
    pub feature: String,
    pub note: String, // always "verify-manually" at v1
}

pub fn unused_features(root: &Path) -> Result<Vec<UnusedFeatureRecord>> {
    let mut out = Vec::new();
    // Walk workspace crates via cargo_metadata so we honor [workspace] members.
    let meta = cargo_metadata::MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .no_deps()
        .exec()
        .context("running cargo metadata")?;

    for pkg in &meta.packages {
        let manifest_path = PathBuf::from(pkg.manifest_path.as_str());
        let manifest_dir = manifest_path.parent().unwrap_or(root).to_path_buf();
        let src_dir = manifest_dir.join("src");

        // Index source text once per crate
        let src_blob = collect_src_text(&src_dir);

        for dep in &pkg.dependencies {
            if dep.features.is_empty() {
                continue;
            }
            let dep_underscore = dep.name.replace('-', "_");
            for feature in &dep.features {
                // Heuristic hit: either the feature name literal appears in
                // source, or there's any `use <dep>::` usage at all (we can't
                // tell which feature a given symbol came from without the
                // dep's own feature gates — so any usage = "verify manually").
                let feat_hit = src_blob.contains(feature);
                let dep_hit = src_blob.contains(&format!("{}::", dep_underscore))
                    || src_blob.contains(&format!("use {}", dep_underscore));
                if feat_hit || dep_hit {
                    continue;
                }
                out.push(UnusedFeatureRecord {
                    crate_name: pkg.name.clone(),
                    dep_name: dep.name.clone(),
                    feature: feature.clone(),
                    note: "verify-manually".to_string(),
                });
            }
        }
    }
    Ok(out)
}

fn collect_src_text(dir: &Path) -> String {
    let mut buf = String::new();
    if !dir.exists() {
        return buf;
    }
    for entry in walkdir::WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                if let Ok(text) = std::fs::read_to_string(path) {
                    buf.push_str(&text);
                    buf.push('\n');
                }
            }
        }
    }
    buf
}

#[allow(dead_code)]
pub fn unused_features_findings(records: &[UnusedFeatureRecord]) -> Vec<Finding> {
    records
        .iter()
        .map(|r| {
            let msg = format!(
                "crate `{}` enables feature `{}` of `{}` but no usage detected ({})",
                r.crate_name, r.feature, r.dep_name, r.note
            );
            Finding::external(
                PathBuf::from("Cargo.toml"),
                0,
                0,
                "wraith-deps",
                "unused-features",
                &msg,
                Severity::Info,
            )
        })
        .collect()
}

pub fn print_unused_features(records: &[UnusedFeatureRecord], format: DepsFormat) {
    match format {
        DepsFormat::Json => {
            println!("{}", serde_json::to_string_pretty(records).unwrap());
        }
        DepsFormat::Md => {
            println!("# Possibly-unused features\n");
            println!("> v1 heuristic — flagged candidates need manual verification.\n");
            if records.is_empty() {
                println!("None flagged.");
                return;
            }
            println!("| crate | dep | feature | note |");
            println!("| --- | --- | --- | --- |");
            for r in records {
                println!(
                    "| `{}` | `{}` | `{}` | {} |",
                    r.crate_name, r.dep_name, r.feature, r.note
                );
            }
        }
        DepsFormat::Human => {
            if records.is_empty() {
                println!("no unused-feature candidates.");
                return;
            }
            for r in records {
                println!(
                    "{}: dep `{}` feature `{}` ({})",
                    r.crate_name, r.dep_name, r.feature, r.note
                );
            }
            println!("\n{} candidate(s).", records.len());
        }
    }
}

// ───────────────────────── size (cargo-bloat shim) ─────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct SizeRecord {
    pub crate_name: String,
    pub size_bytes: u64,
    pub percent: f64,
}

#[derive(Debug, Deserialize)]
struct BloatOutput {
    #[serde(default)]
    crates: Vec<BloatCrate>,
}

#[derive(Debug, Deserialize)]
struct BloatCrate {
    name: String,
    size: u64,
    #[serde(default)]
    #[allow(dead_code)]
    text: Option<String>,
}

pub fn size(root: &Path, release: bool) -> Result<Vec<SizeRecord>> {
    if which("cargo-bloat").is_none() {
        return Err(anyhow!(
            "cargo-bloat not found on PATH. Install with `cargo install cargo-bloat` and re-run."
        ));
    }
    let mut cmd = Command::new("cargo-bloat");
    cmd.arg("bloat")
        .arg("--message-format=json")
        .arg("--crates")
        .current_dir(root);
    if release {
        cmd.arg("--release");
    }
    let out = cmd.output().context("invoking cargo-bloat")?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if stdout.trim().is_empty() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow!("cargo-bloat produced no output. stderr: {}", stderr));
    }
    parse_bloat_json(&stdout)
}

pub fn parse_bloat_json(s: &str) -> Result<Vec<SizeRecord>> {
    // cargo-bloat emits a single JSON object — try that first; some versions
    // emit jsonl. Try both.
    if let Ok(b) = serde_json::from_str::<BloatOutput>(s) {
        return Ok(into_size_records(b));
    }
    // jsonl fallback: last non-empty line is the summary.
    for line in s.lines().rev() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(b) = serde_json::from_str::<BloatOutput>(line) {
            return Ok(into_size_records(b));
        }
    }
    Err(anyhow!("could not parse cargo-bloat output"))
}

fn into_size_records(b: BloatOutput) -> Vec<SizeRecord> {
    let total: u64 = b.crates.iter().map(|c| c.size).sum();
    b.crates
        .into_iter()
        .map(|c| SizeRecord {
            crate_name: c.name,
            size_bytes: c.size,
            percent: if total > 0 {
                (c.size as f64 / total as f64) * 100.0
            } else {
                0.0
            },
        })
        .collect()
}

#[allow(dead_code)]
pub fn size_findings(records: &[SizeRecord]) -> Vec<Finding> {
    // Surface each crate as an Info finding — useful in aggregated reports.
    records
        .iter()
        .map(|r| {
            let msg = format!(
                "`{}` contributes {} bytes ({:.2}%) to the final binary",
                r.crate_name, r.size_bytes, r.percent
            );
            Finding::external(
                PathBuf::from("target"),
                0,
                0,
                "wraith-deps",
                "size",
                &msg,
                Severity::Info,
            )
        })
        .collect()
}

pub fn print_size(records: &[SizeRecord], format: DepsFormat) {
    match format {
        DepsFormat::Json => {
            println!("{}", serde_json::to_string_pretty(records).unwrap());
        }
        DepsFormat::Md => {
            println!("# Binary size by crate\n");
            if records.is_empty() {
                println!("No data.");
                return;
            }
            println!("| crate | bytes | % |");
            println!("| --- | ---: | ---: |");
            for r in records {
                println!(
                    "| `{}` | {} | {:.2} |",
                    r.crate_name, r.size_bytes, r.percent
                );
            }
        }
        DepsFormat::Human => {
            if records.is_empty() {
                println!("no size data.");
                return;
            }
            for r in records {
                println!(
                    "{:>10}  {:>6.2}%  {}",
                    r.size_bytes, r.percent, r.crate_name
                );
            }
        }
    }
}

// ───────────────────────── PATH lookup ─────────────────────────

fn which(bin: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

// Re-export so the main dispatcher can route by category from one place.
#[allow(dead_code)]
pub fn all_categories() -> BTreeSet<&'static str> {
    ["duplicates", "audit", "unused-features", "size"]
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TINY_LOCK: &str = r#"
version = 3

[[package]]
name = "myws"
version = "0.1.0"
dependencies = [
 "serde 1.0.190",
 "other",
]

[[package]]
name = "other"
version = "0.1.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
dependencies = [
 "serde 1.0.219",
]

[[package]]
name = "serde"
version = "1.0.190"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "serde"
version = "1.0.219"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;

    #[test]
    fn duplicates_detects_two_serde_versions() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Cargo.lock"), TINY_LOCK).unwrap();
        let recs = duplicates(tmp.path()).unwrap();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].crate_name, "serde");
        assert_eq!(recs[0].versions.len(), 2);
        assert!(recs[0].versions.contains(&"1.0.190".to_string()));
        assert!(recs[0].versions.contains(&"1.0.219".to_string()));
        // callers for 1.0.190 includes myws@0.1.0
        let callers_190 = recs[0].callers.get("1.0.190").unwrap();
        assert!(
            callers_190.iter().any(|c| c.starts_with("myws@")),
            "expected myws as a caller of serde 1.0.190, got {:?}",
            callers_190
        );
        // callers for 1.0.219 includes other@0.1.0
        let callers_219 = recs[0].callers.get("1.0.219").unwrap();
        assert!(
            callers_219.iter().any(|c| c.starts_with("other@")),
            "expected other as a caller of serde 1.0.219, got {:?}",
            callers_219
        );
    }

    #[test]
    fn duplicates_empty_when_single_version() {
        let lock = r#"
version = 3
[[package]]
name = "serde"
version = "1.0.219"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Cargo.lock"), lock).unwrap();
        let recs = duplicates(tmp.path()).unwrap();
        assert!(recs.is_empty());
    }

    #[test]
    fn parse_audit_json_extracts_advisory() {
        let stub = r#"{
          "vulnerabilities": {
            "list": [
              {
                "advisory": {
                  "id": "RUSTSEC-2020-0071",
                  "title": "Potential segfault in localtime_r invocations",
                  "url": "https://rustsec.org/advisories/RUSTSEC-2020-0071"
                },
                "package": {
                  "name": "time",
                  "version": "0.1.43"
                }
              }
            ]
          }
        }"#;
        let recs = parse_audit_json(stub).unwrap();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].advisory_id, "RUSTSEC-2020-0071");
        assert_eq!(recs[0].crate_name, "time");
        assert_eq!(recs[0].version, "0.1.43");
    }

    #[test]
    fn parse_audit_json_empty_when_no_vulns() {
        let stub = r#"{ "vulnerabilities": { "list": [] } }"#;
        let recs = parse_audit_json(stub).unwrap();
        assert!(recs.is_empty());
    }

    #[test]
    fn parse_bloat_json_computes_percent() {
        let stub = r#"{
          "crates": [
            { "name": "std", "size": 300 },
            { "name": "serde", "size": 100 }
          ]
        }"#;
        let recs = parse_bloat_json(stub).unwrap();
        assert_eq!(recs.len(), 2);
        let std_rec = recs.iter().find(|r| r.crate_name == "std").unwrap();
        assert_eq!(std_rec.size_bytes, 300);
        assert!((std_rec.percent - 75.0).abs() < 0.01);
    }
}
