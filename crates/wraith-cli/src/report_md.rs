//! Markdown report rendering. Aggregates findings across all detectors
//! into a single human-readable document suitable for pasting into a
//! README or sharing with reviewers.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn rel<'a>(root: &Path, p: &'a Path) -> std::borrow::Cow<'a, str> {
    p.strip_prefix(root)
        .map(|s| s.to_string_lossy())
        .unwrap_or_else(|_| p.to_string_lossy())
}
use wraith_core::fix;
use wraith_core::report::{Finding, FindingKind, Severity};

pub struct Inputs<'a> {
    pub workspace_root: &'a Path,
    pub findings: &'a [Finding],
    pub crates_count: usize,
    pub files_count: usize,
    pub total_loc: usize,
    pub elapsed_ms: u128,
}

pub fn render(input: &Inputs<'_>) -> String {
    let mut out = String::new();

    let detector_buckets = group_by_detector(input.findings);
    let severity_hist = severity_histogram(input.findings);
    let hotspot_files = top_hotspot_files(input.findings, 10);
    let complexity_hits = top_complexity(input.findings, 10);
    let dupe_clusters = collect_dupe_clusters(input.findings);
    let cycles = collect_cycles(input.findings);
    let plan = fix::plan(input.findings);

    let total = input.findings.len();
    let auto_fixable = plan.edits.len();
    let pct = if total == 0 {
        0
    } else {
        (auto_fixable * 100) / total
    };
    let lines_removable = estimate_lines_removed(&plan);

    push_header(&mut out, input);
    push_summary(
        &mut out,
        input,
        total,
        auto_fixable,
        pct,
        lines_removable,
    );
    push_severity(&mut out, &severity_hist);
    push_detector_table(&mut out, &detector_buckets);
    push_hotspot_files(&mut out, input.workspace_root, &hotspot_files);
    push_complexity(&mut out, input.workspace_root, &complexity_hits);
    push_dupe_clusters(&mut out, &dupe_clusters);
    push_cycles(&mut out, &cycles);
    push_fix_summary(&mut out, &plan, lines_removable);
    push_limitations(&mut out);

    out
}

fn group_by_detector(findings: &[Finding]) -> BTreeMap<&'static str, usize> {
    let mut m: BTreeMap<&'static str, usize> = BTreeMap::new();
    for f in findings {
        *m.entry(detector_name(&f.kind)).or_insert(0) += 1;
    }
    m
}

fn detector_name(k: &FindingKind) -> &'static str {
    match k {
        FindingKind::DeadCode { .. } => "dead-code",
        FindingKind::UnusedDep { .. } => "unused-deps",
        FindingKind::CircularDep { .. } => "circular-deps",
        FindingKind::Duplicate { .. } => "dupes (pairs)",
        FindingKind::DuplicateCluster { .. } => "dupes (clusters)",
        FindingKind::Complexity { .. } => "health",
        FindingKind::BoundaryViolation { .. } => "boundaries",
        FindingKind::External { .. } => "fallow (ts/js)",
    }
}

fn severity_histogram(findings: &[Finding]) -> BTreeMap<&'static str, usize> {
    let mut m: BTreeMap<&'static str, usize> = BTreeMap::new();
    for f in findings {
        let key = match f.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        };
        *m.entry(key).or_insert(0) += 1;
    }
    m
}

fn top_hotspot_files(findings: &[Finding], n: usize) -> Vec<(PathBuf, usize)> {
    let mut counts: BTreeMap<PathBuf, usize> = BTreeMap::new();
    for f in findings {
        *counts.entry(f.file.clone()).or_insert(0) += 1;
    }
    let mut v: Vec<(PathBuf, usize)> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v.truncate(n);
    v
}

struct ComplexityHit {
    symbol: String,
    file: PathBuf,
    line: usize,
    cyclo: u32,
    cog: u32,
}

fn top_complexity(findings: &[Finding], n: usize) -> Vec<ComplexityHit> {
    let mut hits: Vec<ComplexityHit> = findings
        .iter()
        .filter_map(|f| match &f.kind {
            FindingKind::Complexity {
                symbol,
                cyclomatic,
                cognitive,
                ..
            } => Some(ComplexityHit {
                symbol: symbol.clone(),
                file: f.file.clone(),
                line: f.line,
                cyclo: *cyclomatic,
                cog: *cognitive,
            }),
            _ => None,
        })
        .collect();
    hits.sort_by(|a, b| b.cyclo.cmp(&a.cyclo).then(b.cog.cmp(&a.cog)));
    hits.truncate(n);
    hits
}

struct ClusterSummary {
    representative: String,
    member_count: usize,
    min_sim: f32,
    max_sim: f32,
}

fn collect_dupe_clusters(findings: &[Finding]) -> Vec<ClusterSummary> {
    let mut clusters: Vec<ClusterSummary> = findings
        .iter()
        .filter_map(|f| match &f.kind {
            FindingKind::DuplicateCluster {
                members,
                min_similarity,
                max_similarity,
                ..
            } => Some(ClusterSummary {
                representative: members
                    .first()
                    .map(|m| m.symbol.clone())
                    .unwrap_or_default(),
                member_count: members.len(),
                min_sim: *min_similarity,
                max_sim: *max_similarity,
            }),
            _ => None,
        })
        .collect();
    clusters.sort_by(|a, b| b.member_count.cmp(&a.member_count));
    clusters
}

struct CycleSummary {
    scope: String,
    cycle: Vec<String>,
}

fn collect_cycles(findings: &[Finding]) -> Vec<CycleSummary> {
    findings
        .iter()
        .filter_map(|f| match &f.kind {
            FindingKind::CircularDep { scope, cycle } => Some(CycleSummary {
                scope: scope.clone(),
                cycle: cycle.clone(),
            }),
            _ => None,
        })
        .collect()
}

fn estimate_lines_removed(plan: &fix::FixPlan) -> usize {
    plan.edits
        .iter()
        .filter(|e| matches!(e.kind, fix::EditKind::DeleteLines { .. }))
        .count()
}

fn push_header(out: &mut String, input: &Inputs<'_>) {
    let root = input
        .workspace_root
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| input.workspace_root.display().to_string());
    out.push_str(&format!("# Wraith report — `{}`\n\n", root));
    out.push_str(&format!(
        "_Generated by `wraith report` in {} ms._\n\n",
        input.elapsed_ms
    ));
}

fn push_summary(
    out: &mut String,
    input: &Inputs<'_>,
    total: usize,
    auto_fixable: usize,
    pct: usize,
    lines_removable: usize,
) {
    out.push_str("## Summary\n\n");
    out.push_str(&format!(
        "- **Crates analyzed**: {}\n",
        input.crates_count
    ));
    out.push_str(&format!("- **Source files**: {}\n", input.files_count));
    out.push_str(&format!("- **Lines of code**: {}\n", input.total_loc));
    out.push_str(&format!("- **Findings**: {}\n", total));
    out.push_str(&format!(
        "- **Auto-fixable**: {} ({}%) — `wraith fix --apply` removes ~{} lines\n\n",
        auto_fixable, pct, lines_removable
    ));
}

fn push_severity(out: &mut String, hist: &BTreeMap<&str, usize>) {
    out.push_str("## Severity\n\n");
    out.push_str("| severity | count |\n|---|---:|\n");
    for level in ["error", "warning", "info"] {
        out.push_str(&format!(
            "| {} | {} |\n",
            level,
            hist.get(level).copied().unwrap_or(0)
        ));
    }
    out.push('\n');
}

fn push_detector_table(out: &mut String, buckets: &BTreeMap<&str, usize>) {
    out.push_str("## By detector\n\n");
    out.push_str("| detector | findings | auto-fixable |\n|---|---:|---|\n");
    let auto_ok = ["dead-code", "unused-deps"];
    let partial = ["health", "dupes (clusters)", "dupes (pairs)"];
    for (name, count) in buckets {
        let fix_state = if auto_ok.contains(name) {
            "✓"
        } else if partial.contains(name) {
            "partial — via `wraith refactor extract-fn`"
        } else {
            "manual"
        };
        out.push_str(&format!("| `{}` | {} | {} |\n", name, count, fix_state));
    }
    out.push('\n');
}

fn push_hotspot_files(out: &mut String, root: &Path, files: &[(PathBuf, usize)]) {
    if files.is_empty() {
        return;
    }
    out.push_str("## Top hotspot files\n\n");
    out.push_str("| file | findings |\n|---|---:|\n");
    for (f, c) in files {
        out.push_str(&format!("| `{}` | {} |\n", rel(root, f), c));
    }
    out.push('\n');
}

fn push_complexity(out: &mut String, root: &Path, hits: &[ComplexityHit]) {
    if hits.is_empty() {
        return;
    }
    out.push_str("## Top complexity hotspots\n\n");
    out.push_str("| function | cyclomatic | cognitive | location |\n|---|---:|---:|---|\n");
    for h in hits {
        out.push_str(&format!(
            "| `{}` | {} | {} | `{}:{}` |\n",
            h.symbol,
            h.cyclo,
            h.cog,
            rel(root, &h.file),
            h.line
        ));
    }
    out.push('\n');
}

fn push_dupe_clusters(out: &mut String, clusters: &[ClusterSummary]) {
    if clusters.is_empty() {
        return;
    }
    out.push_str("## Largest duplicate clusters\n\n");
    out.push_str("| representative | members | similarity range |\n|---|---:|---|\n");
    for c in clusters.iter().take(10) {
        out.push_str(&format!(
            "| `{}` | {} | {:.2}–{:.2} |\n",
            c.representative, c.member_count, c.min_sim, c.max_sim
        ));
    }
    out.push('\n');
}

fn push_cycles(out: &mut String, cycles: &[CycleSummary]) {
    if cycles.is_empty() {
        return;
    }
    out.push_str("## Circular dependencies\n\n");
    for (i, c) in cycles.iter().enumerate() {
        let chain = if c.cycle.is_empty() {
            "(empty)".to_string()
        } else {
            format!("{} → {}", c.cycle.join(" → "), c.cycle[0])
        };
        out.push_str(&format!(
            "{}. **{}** ({} nodes): `{}`\n",
            i + 1,
            c.scope,
            c.cycle.len(),
            chain
        ));
    }
    out.push('\n');
}

fn push_fix_summary(out: &mut String, plan: &fix::FixPlan, lines_removable: usize) {
    out.push_str("## What `wraith fix --apply` would do\n\n");
    if plan.edits.is_empty() {
        out.push_str("Nothing — no auto-fixable findings.\n\n");
        return;
    }
    let mut files: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();
    let mut dep_removals = 0usize;
    let mut line_removals = 0usize;
    for e in &plan.edits {
        files.insert(e.file.clone());
        match e.kind {
            fix::EditKind::DeleteLines { .. } => line_removals += 1,
            fix::EditKind::RemoveCargoDep { .. } => dep_removals += 1,
        }
    }
    out.push_str(&format!(
        "Applying `wraith fix --apply` would:\n\n\
         - Apply **{} edits** across **{} files**\n\
         - Remove **{} dead pub items** (~{} lines)\n\
         - Remove **{} unused dependenc{}** from `Cargo.toml`\n\
         - Preserve **all `Cargo.toml` metadata** (`[package]`, `[features]`, comments)\n\
         - Make **zero edits** outside `[dependencies]` / `[dev-dependencies]` / `[build-dependencies]`\n\n",
        plan.edits.len(),
        files.len(),
        line_removals,
        lines_removable,
        dep_removals,
        if dep_removals == 1 { "y" } else { "ies" }
    ));
}

// =====================================================================
// Diff-mode report (wraith report --since=<ref>)
// =====================================================================

pub struct DiffInputs<'a> {
    pub workspace_root: &'a Path,
    pub base_ref: String,
    pub base_sha: String,
    pub head_sha: String,
    pub base_findings: &'a [Finding],
    pub head_findings: &'a [Finding],
    pub base_loc: usize,
    pub head_loc: usize,
    pub base_files: usize,
    pub head_files: usize,
    pub loc_added: usize,
    pub loc_removed: usize,
    pub files_changed: usize,
    pub elapsed_ms: u128,
}

/// Normalize a finding's file path to be relative to a known workspace
/// root. Used for cross-snapshot identity: base findings live inside a
/// temp worktree (`.../wraith-diff/<sha>/packages/<crate>/...`) while
/// head findings live at the live path (`.../packages/<crate>/...`).
/// We normalize both to `<crate>/src/...` so they match.
fn normalize_path_relative_to(p: &Path, workspace_root: &Path) -> String {
    let s = p.display().to_string();
    // Try direct strip first (works for HEAD findings).
    if let Ok(rel) = p.strip_prefix(workspace_root) {
        return rel.display().to_string();
    }
    // For worktree paths: find the workspace root's basename in the path
    // and take everything after.
    let root_name = workspace_root
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    if !root_name.is_empty() {
        let needle = format!("/{}/", root_name);
        if let Some(idx) = s.rfind(&needle) {
            return s[idx + needle.len()..].to_string();
        }
    }
    s
}

/// Identity tuple for finding diff. Workspace_root is captured so
/// cross-snapshot file paths normalize to the same string.
fn finding_identity_for(f: &Finding, workspace_root: &Path) -> String {
    let kind = match &f.kind {
        FindingKind::DeadCode { symbol, .. } => format!("dead:{}", symbol),
        FindingKind::UnusedDep {
            crate_name,
            dep_name,
            ..
        } => format!("unuseddep:{}:{}", crate_name, dep_name),
        FindingKind::CircularDep { cycle, .. } => {
            let mut nodes = cycle.clone();
            nodes.sort();
            format!("cycle:{}", nodes.join("->"))
        }
        FindingKind::Duplicate {
            symbol_a, symbol_b, ..
        } => {
            let mut pair = vec![symbol_a.clone(), symbol_b.clone()];
            pair.sort();
            format!("dupe:{}", pair.join("≈"))
        }
        FindingKind::DuplicateCluster { .. } => {
            // Cluster identity = sorted member symbols.
            format!("dupecluster:{}:{}", f.file.display(), f.line)
        }
        FindingKind::Complexity { symbol, .. } => format!("complexity:{}", symbol),
        FindingKind::BoundaryViolation {
            from_crate, to_path, ..
        } => format!("boundary:{}->{}", from_crate, to_path),
        FindingKind::External {
            source,
            category,
            message,
        } => format!("external:{}:{}:{}", source, category, message),
    };
    // Anchor on file too — same symbol in different files is a different finding.
    format!(
        "{}|{}",
        normalize_path_relative_to(&f.file, workspace_root),
        kind
    )
}

pub fn render_diff(input: &DiffInputs<'_>) -> String {
    use std::collections::BTreeMap;
    use std::collections::HashMap;

    let mut head_by_id: HashMap<String, &Finding> = HashMap::new();
    for f in input.head_findings {
        head_by_id.insert(finding_identity_for(f, input.workspace_root), f);
    }
    let mut base_by_id: HashMap<String, &Finding> = HashMap::new();
    for f in input.base_findings {
        base_by_id.insert(finding_identity_for(f, input.workspace_root), f);
    }

    let resolved: Vec<&Finding> = input
        .base_findings
        .iter()
        .filter(|f| !head_by_id.contains_key(&finding_identity_for(f, input.workspace_root)))
        .collect();
    let introduced: Vec<&Finding> = input
        .head_findings
        .iter()
        .filter(|f| !base_by_id.contains_key(&finding_identity_for(f, input.workspace_root)))
        .collect();
    let still_present: usize = input
        .head_findings
        .iter()
        .filter(|f| base_by_id.contains_key(&finding_identity_for(f, input.workspace_root)))
        .count();

    let by_kind = |xs: &[&Finding]| -> BTreeMap<&'static str, usize> {
        let mut m: BTreeMap<&'static str, usize> = BTreeMap::new();
        for f in xs {
            *m.entry(detector_name(&f.kind)).or_insert(0) += 1;
        }
        m
    };
    let resolved_by_kind = by_kind(&resolved);
    let introduced_by_kind = by_kind(&introduced);

    let mut out = String::new();
    let root = input
        .workspace_root
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| input.workspace_root.display().to_string());
    out.push_str(&format!(
        "# Wraith diff report — `{}`\n\n_`{} → HEAD` ({}..{}). Generated by `wraith report --since={}` in {} ms._\n\n",
        root, input.base_ref, input.base_sha, input.head_sha, input.base_ref, input.elapsed_ms
    ));

    // Top-of-report wins summary.
    out.push_str("## At a glance\n\n");
    let net_findings = input.head_findings.len() as i64 - input.base_findings.len() as i64;
    let net_loc = input.head_loc as i64 - input.base_loc as i64;
    out.push_str("| metric | before | after | delta |\n|---|---:|---:|---:|\n");
    out.push_str(&format!(
        "| Source files | {} | {} | {:+} |\n",
        input.base_files,
        input.head_files,
        input.head_files as i64 - input.base_files as i64
    ));
    out.push_str(&format!(
        "| Lines of code | {} | {} | {:+} |\n",
        input.base_loc, input.head_loc, net_loc
    ));
    out.push_str(&format!(
        "| Total findings | {} | {} | {:+} |\n",
        input.base_findings.len(),
        input.head_findings.len(),
        net_findings
    ));
    out.push_str(&format!(
        "| Findings resolved | — | — | **{}** |\n",
        resolved.len()
    ));
    out.push_str(&format!(
        "| Findings introduced | — | — | {} |\n",
        introduced.len()
    ));
    out.push_str(&format!(
        "| Findings unchanged | — | — | {} |\n\n",
        still_present
    ));

    out.push_str(&format!(
        "**Git diff** (path-scoped): {} files changed, +{} / −{} lines.\n\n",
        input.files_changed, input.loc_added, input.loc_removed
    ));

    // Per-detector resolved / introduced.
    out.push_str("## Findings resolved (fixed since base)\n\n");
    if resolved.is_empty() {
        out.push_str("None — base findings all still present at HEAD.\n\n");
    } else {
        out.push_str("| detector | count |\n|---|---:|\n");
        for (k, c) in &resolved_by_kind {
            out.push_str(&format!("| `{}` | {} |\n", k, c));
        }
        out.push_str("\n<details><summary>Show resolved findings</summary>\n\n");
        for f in resolved.iter().take(50) {
            out.push_str(&format!("- {}\n", f.render_human()));
        }
        if resolved.len() > 50 {
            out.push_str(&format!("\n_…and {} more_\n", resolved.len() - 50));
        }
        out.push_str("\n</details>\n\n");
    }

    out.push_str("## Findings introduced (new at HEAD)\n\n");
    if introduced.is_empty() {
        out.push_str("None — HEAD adds no new findings beyond what was at the base.\n\n");
    } else {
        out.push_str("| detector | count |\n|---|---:|\n");
        for (k, c) in &introduced_by_kind {
            out.push_str(&format!("| `{}` | {} |\n", k, c));
        }
        out.push_str("\n<details><summary>Show introduced findings</summary>\n\n");
        for f in introduced.iter().take(50) {
            out.push_str(&format!("- {}\n", f.render_human()));
        }
        if introduced.len() > 50 {
            out.push_str(&format!("\n_…and {} more_\n", introduced.len() - 50));
        }
        out.push_str("\n</details>\n\n");
    }

    // Net-win narrative.
    out.push_str("## Net effect\n\n");
    if resolved.len() > introduced.len() {
        out.push_str(&format!(
            "**Net win**: {} findings resolved, {} introduced (net −{}).\n\n",
            resolved.len(),
            introduced.len(),
            resolved.len() - introduced.len()
        ));
    } else if introduced.len() > resolved.len() {
        out.push_str(&format!(
            "**Net regression**: {} introduced vs {} resolved (net +{}).\n\n",
            introduced.len(),
            resolved.len(),
            introduced.len() - resolved.len()
        ));
    } else {
        out.push_str(&format!(
            "**Net flat**: {} resolved, {} introduced.\n\n",
            resolved.len(),
            introduced.len()
        ));
    }

    if net_loc < 0 {
        out.push_str(&format!(
            "Codebase is **{} lines smaller** at HEAD ({} → {}).\n",
            -net_loc, input.base_loc, input.head_loc
        ));
    } else if net_loc > 0 {
        out.push_str(&format!(
            "Codebase grew by **{} lines** ({} → {}).\n",
            net_loc, input.base_loc, input.head_loc
        ));
    } else {
        out.push_str("LOC unchanged.\n");
    }

    out
}

fn push_limitations(out: &mut String) {
    out.push_str("## Known limitations\n\n");
    out.push_str(
        "- **Resolver precision**: cross-crate references use path-prefix \
         resolution; ambiguous leaf names are dropped (zero false positives, \
         small false-negative risk).\n\
         - **`#[derive(Trait)]`**: workspace-local derive macros are kept alive \
         via a derive-records-reference rule.\n\
         - **Dupes detector**: token-shingled similarity at the fn-body level. \
         Doesn't detect type-only or trait-impl duplication.\n\
         - **Complexity**: cyclomatic + cognitive counts. Doesn't yet score \
         async-ness or generic bounds.\n\
         - **fallow integration (TS/JS)**: runs the vendored `fallow` binary \
         as a subprocess (see `crates/wraith-cli/src/fallow.rs`). \
         Unified `Finding` schema; same severity buckets.\n",
    );
}
