use crate::health_types::{
    COGNITIVE_EXTRACTION_THRESHOLD, Confidence, ContributingFactor, EffortEstimate,
    EvidenceFunction, FileHealthScore, HotspotEntry, RecommendationCategory, RefactoringTarget,
    TargetEvidence, TargetThresholds,
};

/// Auxiliary data used by `compute_refactoring_targets` to generate evidence and apply rules.
pub(super) struct TargetAuxData<'a> {
    pub circular_files: &'a rustc_hash::FxHashSet<std::path::PathBuf>,
    pub top_complex_fns: &'a rustc_hash::FxHashMap<std::path::PathBuf, Vec<(String, u32, u16)>>,
    pub entry_points: &'a rustc_hash::FxHashSet<std::path::PathBuf>,
    pub value_export_counts: &'a rustc_hash::FxHashMap<std::path::PathBuf, usize>,
    pub unused_export_names: &'a rustc_hash::FxHashMap<std::path::PathBuf, Vec<String>>,
    pub cycle_members: &'a rustc_hash::FxHashMap<std::path::PathBuf, Vec<std::path::PathBuf>>,
}

impl<'a> From<&'a super::scoring::FileScoreOutput> for TargetAuxData<'a> {
    fn from(output: &'a super::scoring::FileScoreOutput) -> Self {
        Self {
            circular_files: &output.circular_files,
            top_complex_fns: &output.top_complex_fns,
            entry_points: &output.entry_points,
            value_export_counts: &output.value_export_counts,
            unused_export_names: &output.unused_export_names,
            cycle_members: &output.cycle_members,
        }
    }
}

/// Adaptive thresholds derived from the project's metric distribution.
///
/// Replaces hardcoded constants (fan_in=20, fan_out=30) with percentile-based
/// values that adapt to the codebase size. Floors prevent degenerate thresholds
/// in small projects.
#[expect(
    clippy::struct_field_names,
    reason = "fan_in/fan_out prefix clarifies the metric"
)]
struct DistributionThresholds {
    /// Fan-in saturation point for priority formula (p95, floor 5).
    fan_in_p95: f64,
    /// Fan-in "moderate" threshold for contributing factors and rule 3 (p75, floor 3).
    fan_in_p75: f64,
    /// Fan-in "low" ceiling for effort estimation (p25, floor 2).
    fan_in_p25: usize,
    /// Fan-out saturation point for priority formula (p95, floor 8).
    fan_out_p95: f64,
    /// Fan-out "high" threshold for contributing factors and rule 6 (p90, floor 5).
    fan_out_p90: usize,
}

/// Compute percentile-based thresholds from the file score distribution.
#[expect(
    clippy::cast_possible_truncation,
    reason = "percentile values are bounded by fan-in/fan-out counts"
)]
fn compute_thresholds(file_scores: &[FileHealthScore]) -> DistributionThresholds {
    if file_scores.is_empty() {
        return DistributionThresholds {
            fan_in_p95: 5.0,
            fan_in_p75: 3.0,
            fan_in_p25: 2,
            fan_out_p95: 8.0,
            fan_out_p90: 5,
        };
    }

    let mut fan_ins: Vec<usize> = file_scores.iter().map(|s| s.fan_in).collect();
    let mut fan_outs: Vec<usize> = file_scores.iter().map(|s| s.fan_out).collect();
    fan_ins.sort_unstable();
    fan_outs.sort_unstable();

    DistributionThresholds {
        fan_in_p95: percentile_usize(&fan_ins, 0.95).max(5.0),
        fan_in_p75: percentile_usize(&fan_ins, 0.75).max(3.0),
        fan_in_p25: (percentile_usize(&fan_ins, 0.25) as usize).max(2),
        fan_out_p95: percentile_usize(&fan_outs, 0.95).max(8.0),
        fan_out_p90: (percentile_usize(&fan_outs, 0.90) as usize).max(5),
    }
}

/// Compute a percentile value from a sorted slice of usize values.
#[expect(
    clippy::cast_possible_truncation,
    reason = "index from percentile of slice length is bounded by slice length"
)]
fn percentile_usize(sorted: &[usize], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (sorted.len() as f64 * p).ceil() as usize;
    let idx = idx.min(sorted.len()) - 1;
    sorted[idx] as f64
}

/// Compute the refactoring priority score for a file.
///
/// Formula (avoids double-counting with MI):
/// ```text
/// priority = min(density, 1) * 30 + hotspot_boost * 25 + dead_code * 20 + fan_in_norm * 15 + fan_out_norm * 10
/// ```
/// Fan-in and fan-out normalization uses adaptive percentile-based thresholds.
/// All inputs are clamped to \[0, 1\] so each weight is a true percentage share.
fn compute_target_priority(
    score: &FileHealthScore,
    hotspot_score: Option<f64>,
    thresholds: &DistributionThresholds,
) -> f64 {
    // Normalize all inputs to [0, 1] so each weight is a true percentage share.
    let density_norm = score.complexity_density.min(1.0);
    let fan_in_norm = (score.fan_in as f64 / thresholds.fan_in_p95).min(1.0);
    let fan_out_norm = (score.fan_out as f64 / thresholds.fan_out_p95).min(1.0);
    let hotspot_boost = hotspot_score.map_or(0.0, |s| s / 100.0);

    #[expect(
        clippy::suboptimal_flops,
        reason = "formula matches documented specification"
    )]
    let priority = density_norm * 30.0
        + hotspot_boost * 25.0
        + score.dead_code_ratio * 20.0
        + fan_in_norm * 15.0
        + fan_out_norm * 10.0;

    (priority.clamp(0.0, 100.0) * 10.0).round() / 10.0
}

/// Compute refactoring targets by applying rules to file scores and auxiliary data.
///
/// Rules are evaluated in priority order; first match determines the category and
/// recommendation. All contributing factors are collected regardless of which rule wins.
/// Files matching no rule are skipped.
///
/// Targets are sorted by efficiency (priority / effort) descending to surface quick wins first.
#[expect(
    clippy::cast_possible_truncation,
    reason = "f64 percentile and ratio values are bounded by collection sizes"
)]
#[expect(
    clippy::too_many_lines,
    reason = "target computation applies 7 refactoring rules sequentially"
)]
pub(super) fn compute_refactoring_targets(
    file_scores: &[FileHealthScore],
    aux: &TargetAuxData,
    hotspots: &[HotspotEntry],
) -> (Vec<RefactoringTarget>, TargetThresholds) {
    // Compute adaptive thresholds from the project's distribution
    let thresholds = compute_thresholds(file_scores);

    // Build hotspot lookup by path for O(1) access
    let hotspot_map: rustc_hash::FxHashMap<&std::path::Path, &HotspotEntry> =
        hotspots.iter().map(|h| (h.path.as_path(), h)).collect();

    let mut targets = Vec::new();

    for score in file_scores {
        let hotspot = hotspot_map.get(score.path.as_path());
        let hotspot_score = hotspot.map(|h| h.score);
        let is_circular = aux.circular_files.contains(&score.path);
        let is_entry = aux.entry_points.contains(&score.path);
        let top_fns = aux.top_complex_fns.get(&score.path);
        let value_exports = aux
            .value_export_counts
            .get(&score.path)
            .copied()
            .unwrap_or(0);

        // Collect all contributing factors (using adaptive thresholds)
        let mut factors = Vec::new();

        if score.complexity_density > 0.3 {
            factors.push(ContributingFactor {
                metric: "complexity_density",
                value: score.complexity_density,
                threshold: 0.3,
                detail: format!("density {:.2} exceeds 0.3", score.complexity_density),
            });
        }
        if score.fan_in as f64 >= thresholds.fan_in_p75 {
            factors.push(ContributingFactor {
                metric: "fan_in",
                value: score.fan_in as f64,
                threshold: thresholds.fan_in_p75,
                detail: format!("{} files depend on this", score.fan_in),
            });
        }
        if score.dead_code_ratio >= 0.5 && value_exports >= 3 {
            let unused_count = (score.dead_code_ratio * value_exports as f64)
                .round()
                .min(value_exports as f64) as usize;
            factors.push(ContributingFactor {
                metric: "dead_code_ratio",
                value: score.dead_code_ratio,
                threshold: 0.5,
                detail: format!(
                    "{} unused of {} value exports ({:.0}%)",
                    unused_count,
                    value_exports,
                    score.dead_code_ratio * 100.0
                ),
            });
        }
        if score.fan_out >= thresholds.fan_out_p90 {
            factors.push(ContributingFactor {
                metric: "fan_out",
                value: score.fan_out as f64,
                threshold: thresholds.fan_out_p90 as f64,
                detail: format!("imports {} modules", score.fan_out),
            });
        }
        if is_circular {
            factors.push(ContributingFactor {
                metric: "circular_dependency",
                value: 1.0,
                threshold: 1.0,
                detail: "participates in an import cycle".into(),
            });
        }
        if let Some(h) = hotspot
            && h.score >= 30.0
        {
            factors.push(ContributingFactor {
                metric: "hotspot_score",
                value: h.score,
                threshold: 30.0,
                detail: format!(
                    "hotspot score {:.0} ({} commits, {} trend)",
                    h.score,
                    h.commits,
                    match h.trend {
                        fallow_core::churn::ChurnTrend::Accelerating => "accelerating",
                        fallow_core::churn::ChurnTrend::Cooling => "cooling",
                        fallow_core::churn::ChurnTrend::Stable => "stable",
                    }
                ),
            });
        }
        if let Some(fns) = top_fns
            && let Some((name, _, cog)) = fns.first()
            && *cog >= COGNITIVE_EXTRACTION_THRESHOLD
        {
            factors.push(ContributingFactor {
                metric: "cognitive_complexity",
                value: f64::from(*cog),
                threshold: f64::from(COGNITIVE_EXTRACTION_THRESHOLD),
                detail: format!("{name} has cognitive complexity {cog}"),
            });
        }
        if score.crap_above_threshold >= 2 && score.crap_max >= super::scoring::CRAP_THRESHOLD {
            factors.push(ContributingFactor {
                metric: "crap_max",
                value: score.crap_max,
                threshold: super::scoring::CRAP_THRESHOLD,
                detail: format!(
                    "{} functions with untested complexity risk",
                    score.crap_above_threshold,
                ),
            });
        }

        // Skip if no factors triggered
        if factors.is_empty() {
            continue;
        }

        // Evaluate rules in priority order — first match determines category + recommendation
        let matched = try_match_rules(
            score,
            hotspot.copied(),
            is_circular,
            is_entry,
            top_fns,
            value_exports,
            &thresholds,
        );

        let Some((category, recommendation)) = matched else {
            continue;
        };

        let priority = compute_target_priority(score, hotspot_score, &thresholds);
        let effort = compute_effort_estimate(score, &thresholds);
        let confidence = confidence_for_category(&category);
        let efficiency = (priority / effort.numeric() * 10.0).round() / 10.0;
        let evidence = build_evidence(
            &category,
            &score.path,
            aux.unused_export_names,
            top_fns,
            aux.cycle_members,
        );

        targets.push(RefactoringTarget {
            path: score.path.clone(),
            priority,
            efficiency,
            recommendation,
            category,
            effort,
            confidence,
            factors,
            evidence,
        });
    }

    // Sort by efficiency descending (quick wins first), break ties by priority desc, then path
    targets.sort_by(|a, b| {
        b.efficiency
            .partial_cmp(&a.efficiency)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.priority
                    .partial_cmp(&a.priority)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.path.cmp(&b.path))
    });

    let exported_thresholds = TargetThresholds {
        fan_in_p95: thresholds.fan_in_p95,
        fan_in_p75: thresholds.fan_in_p75,
        fan_out_p95: thresholds.fan_out_p95,
        fan_out_p90: thresholds.fan_out_p90,
    };

    (targets, exported_thresholds)
}

/// Try to match a file against refactoring rules in priority order.
///
/// Returns the first matching `(category, recommendation)`, or `None` if no rule matches.
#[expect(
    clippy::cast_possible_truncation,
    reason = "threshold values and export counts are bounded by project size"
)]
fn try_match_rules(
    score: &FileHealthScore,
    hotspot: Option<&HotspotEntry>,
    is_circular: bool,
    is_entry: bool,
    top_fns: Option<&Vec<(String, u32, u16)>>,
    value_exports: usize,
    thresholds: &DistributionThresholds,
) -> Option<(RecommendationCategory, String)> {
    // Rule 1: Urgent churn + complexity
    if let Some(h) = hotspot
        && h.score >= 50.0
        && matches!(h.trend, fallow_core::churn::ChurnTrend::Accelerating)
        && score.complexity_density > 0.5
    {
        return Some((
            RecommendationCategory::UrgentChurnComplexity,
            "Actively-changing file with growing complexity \u{2014} stabilize before adding features".into(),
        ));
    }

    // Rule 2: Circular dependency with high fan-in
    if is_circular && score.fan_in >= 5 {
        return Some((
            RecommendationCategory::BreakCircularDependency,
            format!(
                "Break import cycle \u{2014} {} files depend on this, changes cascade through the cycle",
                score.fan_in
            ),
        ));
    }

    // Rule 3: Split high-impact file (adaptive fan-in thresholds)
    let fan_in_high = thresholds.fan_in_p95 as usize;
    let fan_in_moderate = thresholds.fan_in_p75 as usize;
    if score.complexity_density > 0.3
        && (score.fan_in >= fan_in_high
            || (score.fan_in >= fan_in_moderate && score.function_count >= 5))
    {
        return Some((
            RecommendationCategory::SplitHighImpact,
            format!(
                "Split high-impact file ({} LOC) \u{2014} {} dependents amplify every change",
                score.lines, score.fan_in
            ),
        ));
    }

    // Rule 4: Remove dead code (gate: >=3 value exports)
    if score.dead_code_ratio >= 0.5 && value_exports >= 3 {
        let unused_count = (score.dead_code_ratio * value_exports as f64).round() as usize;
        return Some((
            RecommendationCategory::RemoveDeadCode,
            format!(
                "Remove {} unused exports to reduce surface area ({:.0}% dead)",
                unused_count,
                score.dead_code_ratio * 100.0
            ),
        ));
    }

    // Rule 5: Extract complex functions above cognitive extraction threshold
    if let Some(fns) = top_fns {
        let high: Vec<&(String, u32, u16)> = fns
            .iter()
            .filter(|(_, _, cog)| *cog >= COGNITIVE_EXTRACTION_THRESHOLD)
            .collect();
        if !high.is_empty() {
            let desc = match high.len() {
                1 => format!(
                    "Extract {} (cognitive: {}) in {}-LOC file into smaller functions",
                    high[0].0, high[0].2, score.lines
                ),
                _ => format!(
                    "Extract {} (cognitive: {}) and {} (cognitive: {}) in {}-LOC file into smaller functions",
                    high[0].0, high[0].2, high[1].0, high[1].2, score.lines
                ),
            };
            return Some((RecommendationCategory::ExtractComplexFunctions, desc));
        }
    }

    // Rule 6: Extract dependencies (not for entry points, adaptive fan-out threshold)
    if !is_entry && score.fan_out >= thresholds.fan_out_p90 && score.maintainability_index < 60.0 {
        return Some((
            RecommendationCategory::ExtractDependencies,
            format!(
                "Reduce coupling \u{2014} {}-LOC file imports {} modules, limiting testability",
                score.lines, score.fan_out
            ),
        ));
    }

    // Rule 7: High untested complexity risk (multiple high-CRAP functions)
    if score.crap_above_threshold >= 2 && score.complexity_density > 0.3 {
        return Some((
            RecommendationCategory::AddTestCoverage,
            format!(
                "{} complex functions lack test coverage path, add tests before modifying",
                score.crap_above_threshold
            ),
        ));
    }

    // Rule 8: Circular dependency (low fan-in fallback)
    if is_circular {
        return Some((
            RecommendationCategory::BreakCircularDependency,
            "Break import cycle to reduce change cascade risk".into(),
        ));
    }

    None
}

/// Map recommendation category to confidence level based on data source reliability.
const fn confidence_for_category(category: &RecommendationCategory) -> Confidence {
    match category {
        // Deterministic: graph analysis (dead code, cycles) + AST analysis (complexity)
        RecommendationCategory::RemoveDeadCode
        | RecommendationCategory::BreakCircularDependency
        | RecommendationCategory::ExtractComplexFunctions
        | RecommendationCategory::AddTestCoverage => Confidence::High,
        // Heuristic thresholds (fan-in/fan-out coupling)
        RecommendationCategory::SplitHighImpact | RecommendationCategory::ExtractDependencies => {
            Confidence::Medium
        }
        // Depends on git history quality
        RecommendationCategory::UrgentChurnComplexity => Confidence::Low,
    }
}

/// Compute effort estimate based on file size, function count, and fan-in.
///
/// Uses adaptive thresholds for fan-in.
#[expect(
    clippy::cast_possible_truncation,
    reason = "percentile threshold values are bounded by project size"
)]
fn compute_effort_estimate(
    score: &FileHealthScore,
    thresholds: &DistributionThresholds,
) -> EffortEstimate {
    let fan_in_high = thresholds.fan_in_p95 as usize;
    if score.lines >= 500
        || score.fan_in >= fan_in_high
        || (score.function_count >= 15 && score.complexity_density > 0.5)
    {
        EffortEstimate::High
    } else if score.lines < 100 && score.function_count <= 3 && score.fan_in < thresholds.fan_in_p25
    {
        EffortEstimate::Low
    } else {
        EffortEstimate::Medium
    }
}

/// Build structured evidence for a refactoring target based on its category.
fn build_evidence(
    category: &RecommendationCategory,
    path: &std::path::Path,
    unused_export_names: &rustc_hash::FxHashMap<std::path::PathBuf, Vec<String>>,
    top_fns: Option<&Vec<(String, u32, u16)>>,
    cycle_members: &rustc_hash::FxHashMap<std::path::PathBuf, Vec<std::path::PathBuf>>,
) -> Option<TargetEvidence> {
    match category {
        RecommendationCategory::RemoveDeadCode => {
            let exports = unused_export_names.get(path).cloned().unwrap_or_default();
            if exports.is_empty() {
                None
            } else {
                Some(TargetEvidence {
                    unused_exports: exports,
                    complex_functions: vec![],
                    cycle_path: vec![],
                })
            }
        }
        RecommendationCategory::ExtractComplexFunctions => {
            let functions = top_fns
                .map(|fns| {
                    fns.iter()
                        .filter(|(_, _, cog)| *cog >= COGNITIVE_EXTRACTION_THRESHOLD)
                        .map(|(name, line, cog)| EvidenceFunction {
                            name: name.clone(),
                            line: *line,
                            cognitive: *cog,
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if functions.is_empty() {
                None
            } else {
                Some(TargetEvidence {
                    unused_exports: vec![],
                    complex_functions: functions,
                    cycle_path: vec![],
                })
            }
        }
        RecommendationCategory::BreakCircularDependency => {
            let members = cycle_members
                .get(path)
                .map(|files| {
                    files
                        .iter()
                        .map(|f| f.to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if members.is_empty() {
                None
            } else {
                Some(TargetEvidence {
                    unused_exports: vec![],
                    complex_functions: vec![],
                    cycle_path: members,
                })
            }
        }
        RecommendationCategory::AddTestCoverage => {
            // Reuse top complex functions as evidence: these are the functions
            // that need test coverage most urgently (highest cognitive complexity).
            let functions = top_fns
                .map(|fns| {
                    fns.iter()
                        .map(|(name, line, cog)| EvidenceFunction {
                            name: name.clone(),
                            line: *line,
                            cognitive: *cog,
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if functions.is_empty() {
                None
            } else {
                Some(TargetEvidence {
                    unused_exports: vec![],
                    complex_functions: functions,
                    cycle_path: vec![],
                })
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- helpers ---

    fn make_score(overrides: impl FnOnce(&mut FileHealthScore)) -> FileHealthScore {
        let mut s = FileHealthScore {
            path: std::path::PathBuf::from("/src/foo.ts"),
            fan_in: 0,
            fan_out: 0,
            dead_code_ratio: 0.0,
            complexity_density: 0.0,
            maintainability_index: 100.0,
            total_cyclomatic: 0,
            total_cognitive: 0,
            function_count: 1,
            lines: 100,
            crap_max: 0.0,
            crap_above_threshold: 0,
        };
        overrides(&mut s);
        s
    }

    /// Default thresholds matching the old hardcoded values for test compatibility.
    fn default_thresholds() -> DistributionThresholds {
        DistributionThresholds {
            fan_in_p95: 20.0,
            fan_in_p75: 10.0,
            fan_in_p25: 5,
            fan_out_p95: 30.0,
            fan_out_p90: 15,
        }
    }

    // --- compute_thresholds ---

    #[test]
    fn thresholds_empty_scores_use_floors() {
        let t = compute_thresholds(&[]);
        assert!((t.fan_in_p95 - 5.0).abs() < f64::EPSILON);
        assert!((t.fan_in_p75 - 3.0).abs() < f64::EPSILON);
        assert_eq!(t.fan_in_p25, 2);
        assert!((t.fan_out_p95 - 8.0).abs() < f64::EPSILON);
        assert_eq!(t.fan_out_p90, 5);
    }

    #[test]
    fn thresholds_floors_prevent_degenerate_values() {
        // All files have fan_in=1, fan_out=1 — floors should kick in
        let scores: Vec<FileHealthScore> = (0..10)
            .map(|i| {
                make_score(|s| {
                    s.path = std::path::PathBuf::from(format!("/src/{i}.ts"));
                    s.fan_in = 1;
                    s.fan_out = 1;
                })
            })
            .collect();
        let t = compute_thresholds(&scores);
        assert!(t.fan_in_p95 >= 5.0, "floor should apply: {}", t.fan_in_p95);
        assert!(t.fan_in_p75 >= 3.0, "floor should apply: {}", t.fan_in_p75);
        assert!(
            t.fan_out_p95 >= 8.0,
            "floor should apply: {}",
            t.fan_out_p95
        );
        assert!(t.fan_out_p90 >= 5, "floor should apply: {}", t.fan_out_p90);
    }

    #[test]
    fn thresholds_adapt_to_large_project() {
        // Simulate a project with varied fan_in distribution including high values
        let mut scores: Vec<FileHealthScore> = (0..80)
            .map(|i| {
                make_score(|s| {
                    s.path = std::path::PathBuf::from(format!("/src/{i}.ts"));
                    s.fan_in = i % 5; // 0..4
                    s.fan_out = i % 8; // 0..7
                })
            })
            .collect();
        // Top 20% have higher fan_in — enough to push p95 above floors
        for i in 80..100 {
            scores.push(make_score(|s| {
                s.path = std::path::PathBuf::from(format!("/src/{i}.ts"));
                s.fan_in = 15 + (i - 80); // 15..34
                s.fan_out = 10 + (i - 80); // 10..29
            }));
        }
        let t = compute_thresholds(&scores);
        // p95 should reflect the distribution, not just the floor
        assert!(
            t.fan_in_p95 > 5.0,
            "p95 should exceed floor: {}",
            t.fan_in_p95
        );
        assert!(
            t.fan_out_p95 > 8.0,
            "p95 should exceed floor: {}",
            t.fan_out_p95
        );
    }

    // --- compute_target_priority ---

    #[test]
    fn target_priority_all_zero() {
        let score = make_score(|_| {});
        let t = default_thresholds();
        let priority = compute_target_priority(&score, None, &t);
        assert!((priority).abs() < f64::EPSILON);
    }

    #[test]
    fn target_priority_max_all_inputs() {
        let score = make_score(|s| {
            s.complexity_density = 2.0; // clamped to 1.0
            s.fan_in = 40; // clamped to 1.0
            s.fan_out = 60; // clamped to 1.0
            s.dead_code_ratio = 1.0;
        });
        let t = default_thresholds();
        let priority = compute_target_priority(&score, Some(100.0), &t);
        assert!((priority - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn target_priority_complexity_density_weight() {
        // density=1.0, all else zero -> 30 points
        let score = make_score(|s| s.complexity_density = 1.0);
        let t = default_thresholds();
        let priority = compute_target_priority(&score, None, &t);
        assert!((priority - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn target_priority_hotspot_weight() {
        // hotspot_score=100 -> boost=1.0 -> 25 points
        let score = make_score(|_| {});
        let t = default_thresholds();
        let priority = compute_target_priority(&score, Some(100.0), &t);
        assert!((priority - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn target_priority_dead_code_weight() {
        // dead_code_ratio=1.0 -> 20 points
        let score = make_score(|s| s.dead_code_ratio = 1.0);
        let t = default_thresholds();
        let priority = compute_target_priority(&score, None, &t);
        assert!((priority - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn target_priority_fan_in_weight() {
        // fan_in=20 (== p95) -> norm=1.0 -> 15 points
        let score = make_score(|s| s.fan_in = 20);
        let t = default_thresholds();
        let priority = compute_target_priority(&score, None, &t);
        assert!((priority - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn target_priority_fan_out_weight() {
        // fan_out=30 (== p95) -> norm=1.0 -> 10 points
        let score = make_score(|s| s.fan_out = 30);
        let t = default_thresholds();
        let priority = compute_target_priority(&score, None, &t);
        assert!((priority - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn target_priority_adapts_to_thresholds() {
        let score = make_score(|s| s.fan_in = 10);
        // With threshold=20: norm=0.5 -> 7.5 points
        let t_default = default_thresholds();
        let p1 = compute_target_priority(&score, None, &t_default);
        // With threshold=10: norm=1.0 -> 15 points
        let t_small = DistributionThresholds {
            fan_in_p95: 10.0,
            ..default_thresholds()
        };
        let p2 = compute_target_priority(&score, None, &t_small);
        assert!(
            p2 > p1,
            "smaller project threshold should yield higher priority"
        );
    }

    // --- confidence ---

    #[test]
    fn confidence_mapping() {
        assert!(matches!(
            confidence_for_category(&RecommendationCategory::RemoveDeadCode),
            Confidence::High
        ));
        assert!(matches!(
            confidence_for_category(&RecommendationCategory::BreakCircularDependency),
            Confidence::High
        ));
        assert!(matches!(
            confidence_for_category(&RecommendationCategory::ExtractComplexFunctions),
            Confidence::High
        ));
        assert!(matches!(
            confidence_for_category(&RecommendationCategory::SplitHighImpact),
            Confidence::Medium
        ));
        assert!(matches!(
            confidence_for_category(&RecommendationCategory::ExtractDependencies),
            Confidence::Medium
        ));
        assert!(matches!(
            confidence_for_category(&RecommendationCategory::UrgentChurnComplexity),
            Confidence::Low
        ));
        assert!(matches!(
            confidence_for_category(&RecommendationCategory::AddTestCoverage),
            Confidence::High
        ));
    }

    // --- efficiency ---

    #[test]
    fn efficiency_surfaces_quick_wins() {
        // Low effort, high priority should have higher efficiency than high effort, high priority
        let low_effort_priority = 60.0_f64;
        let high_effort_priority = 90.0_f64;
        let low_eff = low_effort_priority / EffortEstimate::Low.numeric();
        let high_eff = high_effort_priority / EffortEstimate::High.numeric();
        assert!(
            low_eff > high_eff,
            "low effort (eff={low_eff}) should rank above high effort (eff={high_eff})"
        );
    }

    #[test]
    fn targets_sorted_by_efficiency_descending() {
        // High-priority + high-effort (eff ~30) vs low-priority + low-effort (eff ~20)
        // The low-effort file should appear first because efficiency = priority/effort
        let scores = vec![
            make_score(|s| {
                s.path = std::path::PathBuf::from("/src/big.ts");
                s.complexity_density = 0.8;
                s.fan_in = 25;
                s.lines = 600;
                s.function_count = 20;
                s.dead_code_ratio = 0.6;
            }),
            make_score(|s| {
                s.path = std::path::PathBuf::from("/src/small.ts");
                s.dead_code_ratio = 0.7;
                s.lines = 50;
                s.function_count = 2;
                s.fan_in = 1;
            }),
        ];
        let aux = TargetAuxData {
            circular_files: &rustc_hash::FxHashSet::default(),
            top_complex_fns: &rustc_hash::FxHashMap::default(),
            entry_points: &rustc_hash::FxHashSet::default(),
            value_export_counts: &[
                (std::path::PathBuf::from("/src/big.ts"), 10_usize),
                (std::path::PathBuf::from("/src/small.ts"), 5_usize),
            ]
            .into_iter()
            .collect(),
            unused_export_names: &rustc_hash::FxHashMap::default(),
            cycle_members: &rustc_hash::FxHashMap::default(),
        };
        let (targets, _thresholds) = compute_refactoring_targets(&scores, &aux, &[]);
        assert!(targets.len() >= 2, "expected at least 2 targets");
        // First target should have higher efficiency (quick win)
        assert!(
            targets[0].efficiency >= targets[1].efficiency,
            "targets should be sorted by efficiency desc: {} >= {}",
            targets[0].efficiency,
            targets[1].efficiency
        );
    }

    // --- try_match_rules ---

    #[test]
    fn rule_no_match_clean_file() {
        let score = make_score(|_| {});
        let t = default_thresholds();
        let result = try_match_rules(&score, None, false, false, None, 0, &t);
        assert!(result.is_none());
    }

    #[test]
    fn rule_circular_dep_high_fan_in() {
        let score = make_score(|s| s.fan_in = 5);
        let t = default_thresholds();
        let result = try_match_rules(&score, None, true, false, None, 0, &t);
        assert!(result.is_some());
        let (cat, _) = result.unwrap();
        assert!(matches!(
            cat,
            RecommendationCategory::BreakCircularDependency
        ));
    }

    #[test]
    fn rule_circular_dep_low_fan_in_fallback() {
        let score = make_score(|s| s.fan_in = 1);
        let t = default_thresholds();
        let result = try_match_rules(&score, None, true, false, None, 0, &t);
        assert!(result.is_some());
        let (cat, _) = result.unwrap();
        assert!(matches!(
            cat,
            RecommendationCategory::BreakCircularDependency
        ));
    }

    #[test]
    fn rule_add_test_coverage() {
        let score = make_score(|s| {
            s.crap_above_threshold = 2;
            s.crap_max = 72.0;
            s.complexity_density = 0.5;
        });
        let t = default_thresholds();
        let result = try_match_rules(&score, None, false, false, None, 0, &t);
        assert!(result.is_some());
        let (cat, rec) = result.unwrap();
        assert!(matches!(cat, RecommendationCategory::AddTestCoverage));
        assert!(rec.contains("2 complex functions"));
    }

    #[test]
    fn rule_add_test_coverage_below_density_threshold() {
        // crap_above >= 2 but density <= 0.3 -> rule does not fire
        let score = make_score(|s| {
            s.crap_above_threshold = 3;
            s.crap_max = 72.0;
            s.complexity_density = 0.2;
        });
        let t = default_thresholds();
        let result = try_match_rules(&score, None, false, false, None, 0, &t);
        assert!(result.is_none());
    }

    #[test]
    fn rule_split_high_impact() {
        let score = make_score(|s| {
            s.complexity_density = 0.5;
            s.fan_in = 20;
        });
        let t = default_thresholds();
        let result = try_match_rules(&score, None, false, false, None, 0, &t);
        assert!(result.is_some());
        let (cat, rec) = result.unwrap();
        assert!(matches!(cat, RecommendationCategory::SplitHighImpact));
        assert!(
            rec.contains("100 LOC"),
            "recommendation should include LOC: {rec}"
        );
    }

    #[test]
    fn rule_remove_dead_code() {
        let score = make_score(|s| s.dead_code_ratio = 0.6);
        let t = default_thresholds();
        let result = try_match_rules(&score, None, false, false, None, 5, &t);
        assert!(result.is_some());
        let (cat, _) = result.unwrap();
        assert!(matches!(cat, RecommendationCategory::RemoveDeadCode));
    }

    #[test]
    fn rule_dead_code_gate_too_few_exports() {
        // dead_code_ratio high but only 2 value exports — below gate of 3
        let score = make_score(|s| s.dead_code_ratio = 0.8);
        let t = default_thresholds();
        let result = try_match_rules(&score, None, false, false, None, 2, &t);
        // Should NOT match dead code rule
        assert!(result.is_none());
    }

    #[test]
    fn rule_extract_complex_functions() {
        let score = make_score(|_| {});
        let fns = vec![("handleSubmit".to_string(), 10u32, 35u16)];
        let t = default_thresholds();
        let result = try_match_rules(&score, None, false, false, Some(&fns), 0, &t);
        assert!(result.is_some());
        let (cat, rec) = result.unwrap();
        assert!(matches!(
            cat,
            RecommendationCategory::ExtractComplexFunctions
        ));
        assert!(rec.contains("handleSubmit"));
        assert!(
            rec.contains("100-LOC"),
            "recommendation should include LOC: {rec}"
        );
    }

    #[test]
    fn rule_extract_dependencies_not_entry() {
        let score = make_score(|s| {
            s.fan_out = 20;
            s.maintainability_index = 50.0;
        });
        let t = default_thresholds();
        let result = try_match_rules(&score, None, false, false, None, 0, &t);
        assert!(result.is_some());
        let (cat, rec) = result.unwrap();
        assert!(matches!(cat, RecommendationCategory::ExtractDependencies));
        assert!(
            rec.contains("100-LOC"),
            "recommendation should include LOC: {rec}"
        );
    }

    #[test]
    fn rule_extract_dependencies_skipped_for_entry() {
        let score = make_score(|s| {
            s.fan_out = 20;
            s.maintainability_index = 50.0;
        });
        let t = default_thresholds();
        // is_entry=true -> rule 6 should not match
        let result = try_match_rules(&score, None, false, true, None, 0, &t);
        assert!(result.is_none());
    }

    #[test]
    fn rule_urgent_churn_complexity() {
        let score = make_score(|s| s.complexity_density = 0.8);
        let hotspot = HotspotEntry {
            path: std::path::PathBuf::from("/src/foo.ts"),
            score: 60.0,
            commits: 20,
            weighted_commits: 15.0,
            lines_added: 500,
            lines_deleted: 100,
            complexity_density: 0.8,
            fan_in: 5,
            trend: fallow_core::churn::ChurnTrend::Accelerating,
            ownership: None,
            is_test_path: false,
        };
        let t = default_thresholds();
        let result = try_match_rules(&score, Some(&hotspot), false, false, None, 0, &t);
        assert!(result.is_some());
        let (cat, _) = result.unwrap();
        assert!(matches!(cat, RecommendationCategory::UrgentChurnComplexity));
    }

    // --- compute_effort_estimate ---

    #[test]
    fn effort_high_for_large_file() {
        let score = make_score(|s| s.lines = 600);
        let t = default_thresholds();
        assert!(matches!(
            compute_effort_estimate(&score, &t),
            EffortEstimate::High
        ));
    }

    #[test]
    fn effort_high_for_high_fan_in() {
        let score = make_score(|s| s.fan_in = 25);
        let t = default_thresholds();
        assert!(matches!(
            compute_effort_estimate(&score, &t),
            EffortEstimate::High
        ));
    }

    #[test]
    fn effort_high_for_many_complex_functions() {
        let score = make_score(|s| {
            s.function_count = 20;
            s.complexity_density = 0.6;
        });
        let t = default_thresholds();
        assert!(matches!(
            compute_effort_estimate(&score, &t),
            EffortEstimate::High
        ));
    }

    #[test]
    fn effort_low_for_small_simple_file() {
        let score = make_score(|s| {
            s.lines = 50;
            s.function_count = 2;
            s.fan_in = 1;
        });
        let t = default_thresholds();
        assert!(matches!(
            compute_effort_estimate(&score, &t),
            EffortEstimate::Low
        ));
    }

    #[test]
    fn effort_medium_for_moderate_file() {
        let score = make_score(|s| {
            s.lines = 200;
            s.function_count = 8;
            s.fan_in = 3;
        });
        let t = default_thresholds();
        assert!(matches!(
            compute_effort_estimate(&score, &t),
            EffortEstimate::Medium
        ));
    }

    // --- build_evidence ---

    #[test]
    fn evidence_dead_code_includes_unused_exports() {
        let mut unused = rustc_hash::FxHashMap::default();
        unused.insert(
            std::path::PathBuf::from("/src/foo.ts"),
            vec!["bar".to_string(), "baz".to_string()],
        );
        let cycle_members = rustc_hash::FxHashMap::default();
        let ev = build_evidence(
            &RecommendationCategory::RemoveDeadCode,
            std::path::Path::new("/src/foo.ts"),
            &unused,
            None,
            &cycle_members,
        );
        assert!(ev.is_some());
        let ev = ev.unwrap();
        assert_eq!(ev.unused_exports, vec!["bar", "baz"]);
        assert!(ev.complex_functions.is_empty());
        assert!(ev.cycle_path.is_empty());
    }

    #[test]
    fn evidence_dead_code_none_when_no_exports() {
        let unused = rustc_hash::FxHashMap::default();
        let cycle_members = rustc_hash::FxHashMap::default();
        let ev = build_evidence(
            &RecommendationCategory::RemoveDeadCode,
            std::path::Path::new("/src/foo.ts"),
            &unused,
            None,
            &cycle_members,
        );
        assert!(ev.is_none());
    }

    #[test]
    fn evidence_extract_complex_functions() {
        let unused = rustc_hash::FxHashMap::default();
        let cycle_members = rustc_hash::FxHashMap::default();
        let fns = vec![
            ("processData".to_string(), 10u32, 40u16),
            ("handleEvent".to_string(), 25u32, 35u16),
            ("simpleHelper".to_string(), 50u32, 5u16),
        ];
        let ev = build_evidence(
            &RecommendationCategory::ExtractComplexFunctions,
            std::path::Path::new("/src/foo.ts"),
            &unused,
            Some(&fns),
            &cycle_members,
        );
        assert!(ev.is_some());
        let ev = ev.unwrap();
        assert!(ev.unused_exports.is_empty());
        // Only functions above COGNITIVE_EXTRACTION_THRESHOLD (25) included
        assert_eq!(ev.complex_functions.len(), 2);
        assert_eq!(ev.complex_functions[0].name, "processData");
        assert_eq!(ev.complex_functions[1].name, "handleEvent");
    }

    #[test]
    fn evidence_break_circular_dep() {
        let unused = rustc_hash::FxHashMap::default();
        let mut cycle_members = rustc_hash::FxHashMap::default();
        cycle_members.insert(
            std::path::PathBuf::from("/src/a.ts"),
            vec![
                std::path::PathBuf::from("/src/b.ts"),
                std::path::PathBuf::from("/src/c.ts"),
            ],
        );
        let ev = build_evidence(
            &RecommendationCategory::BreakCircularDependency,
            std::path::Path::new("/src/a.ts"),
            &unused,
            None,
            &cycle_members,
        );
        assert!(ev.is_some());
        let ev = ev.unwrap();
        assert_eq!(ev.cycle_path.len(), 2);
        assert!(ev.unused_exports.is_empty());
    }

    #[test]
    fn evidence_add_test_coverage_includes_all_fns() {
        let unused = rustc_hash::FxHashMap::default();
        let cycle_members = rustc_hash::FxHashMap::default();
        let fns = vec![("render".to_string(), 5u32, 12u16)];
        let ev = build_evidence(
            &RecommendationCategory::AddTestCoverage,
            std::path::Path::new("/src/foo.ts"),
            &unused,
            Some(&fns),
            &cycle_members,
        );
        assert!(ev.is_some());
        let ev = ev.unwrap();
        assert_eq!(ev.complex_functions.len(), 1);
        assert_eq!(ev.complex_functions[0].name, "render");
    }

    #[test]
    fn evidence_split_high_impact_returns_none() {
        let unused = rustc_hash::FxHashMap::default();
        let cycle_members = rustc_hash::FxHashMap::default();
        let ev = build_evidence(
            &RecommendationCategory::SplitHighImpact,
            std::path::Path::new("/src/foo.ts"),
            &unused,
            None,
            &cycle_members,
        );
        assert!(ev.is_none());
    }

    // --- percentile_usize ---

    #[test]
    fn percentile_empty_returns_zero() {
        assert!((percentile_usize(&[], 0.5)).abs() < f64::EPSILON);
    }

    #[test]
    fn percentile_single_element() {
        assert!((percentile_usize(&[42], 0.5) - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn percentile_p50_median() {
        let data = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let p50 = percentile_usize(&data, 0.50);
        assert!((p50 - 5.0).abs() < f64::EPSILON);
    }

    // --- rule priority ordering ---

    #[test]
    fn rule_urgent_churn_overrides_circular_dep() {
        // Both Rule 1 (urgent churn) and Rule 2 (circular dep) could match
        // Rule 1 has higher priority
        let score = make_score(|s| {
            s.complexity_density = 0.8;
            s.fan_in = 10;
        });
        let hotspot = HotspotEntry {
            path: std::path::PathBuf::from("/src/foo.ts"),
            score: 60.0,
            commits: 20,
            weighted_commits: 15.0,
            lines_added: 500,
            lines_deleted: 100,
            complexity_density: 0.8,
            fan_in: 10,
            trend: fallow_core::churn::ChurnTrend::Accelerating,
            ownership: None,
            is_test_path: false,
        };
        let t = default_thresholds();
        let result = try_match_rules(&score, Some(&hotspot), true, false, None, 0, &t);
        assert!(result.is_some());
        let (cat, _) = result.unwrap();
        assert!(
            matches!(cat, RecommendationCategory::UrgentChurnComplexity),
            "Rule 1 should win over Rule 2"
        );
    }

    #[test]
    fn rule_extract_two_complex_functions() {
        let score = make_score(|_| {});
        let fns = vec![
            ("processData".to_string(), 10u32, 40u16),
            ("handleEvent".to_string(), 25u32, 35u16),
        ];
        let t = default_thresholds();
        let result = try_match_rules(&score, None, false, false, Some(&fns), 0, &t);
        assert!(result.is_some());
        let (cat, rec) = result.unwrap();
        assert!(matches!(
            cat,
            RecommendationCategory::ExtractComplexFunctions
        ));
        assert!(rec.contains("processData"));
        assert!(rec.contains("handleEvent"));
        assert!(
            rec.contains("100-LOC"),
            "two-function recommendation should include LOC: {rec}"
        );
    }

    // --- contributing factors ---

    #[test]
    fn contributing_factor_hotspot() {
        let scores = vec![make_score(|s| {
            s.complexity_density = 0.5;
            s.fan_in = 15;
        })];
        let hotspots = vec![HotspotEntry {
            path: std::path::PathBuf::from("/src/foo.ts"),
            score: 45.0,
            commits: 10,
            weighted_commits: 8.0,
            lines_added: 200,
            lines_deleted: 50,
            complexity_density: 0.5,
            fan_in: 15,
            trend: fallow_core::churn::ChurnTrend::Stable,
            ownership: None,
            is_test_path: false,
        }];
        let value_exports: rustc_hash::FxHashMap<std::path::PathBuf, usize> =
            rustc_hash::FxHashMap::default();
        let aux = TargetAuxData {
            circular_files: &rustc_hash::FxHashSet::default(),
            top_complex_fns: &rustc_hash::FxHashMap::default(),
            entry_points: &rustc_hash::FxHashSet::default(),
            value_export_counts: &value_exports,
            unused_export_names: &rustc_hash::FxHashMap::default(),
            cycle_members: &rustc_hash::FxHashMap::default(),
        };
        let (targets, _) = compute_refactoring_targets(&scores, &aux, &hotspots);
        assert!(!targets.is_empty());
        let target = &targets[0];
        assert!(target.factors.iter().any(|f| f.metric == "hotspot_score"));
    }

    #[test]
    fn contributing_factor_crap() {
        let scores = vec![make_score(|s| {
            s.complexity_density = 0.5;
            s.crap_above_threshold = 3;
            s.crap_max = 72.0;
        })];
        let value_exports: rustc_hash::FxHashMap<std::path::PathBuf, usize> =
            rustc_hash::FxHashMap::default();
        let aux = TargetAuxData {
            circular_files: &rustc_hash::FxHashSet::default(),
            top_complex_fns: &rustc_hash::FxHashMap::default(),
            entry_points: &rustc_hash::FxHashSet::default(),
            value_export_counts: &value_exports,
            unused_export_names: &rustc_hash::FxHashMap::default(),
            cycle_members: &rustc_hash::FxHashMap::default(),
        };
        let (targets, _) = compute_refactoring_targets(&scores, &aux, &[]);
        assert!(!targets.is_empty());
        let target = &targets[0];
        assert!(target.factors.iter().any(|f| f.metric == "crap_max"));
    }

    #[test]
    fn contributing_factor_circular_dependency() {
        let mut circular = rustc_hash::FxHashSet::default();
        circular.insert(std::path::PathBuf::from("/src/foo.ts"));
        let value_exports: rustc_hash::FxHashMap<std::path::PathBuf, usize> =
            rustc_hash::FxHashMap::default();
        let scores = vec![make_score(|s| s.complexity_density = 0.1)];
        let aux = TargetAuxData {
            circular_files: &circular,
            top_complex_fns: &rustc_hash::FxHashMap::default(),
            entry_points: &rustc_hash::FxHashSet::default(),
            value_export_counts: &value_exports,
            unused_export_names: &rustc_hash::FxHashMap::default(),
            cycle_members: &rustc_hash::FxHashMap::default(),
        };
        let (targets, _) = compute_refactoring_targets(&scores, &aux, &[]);
        assert!(!targets.is_empty());
        let target = &targets[0];
        assert!(
            target
                .factors
                .iter()
                .any(|f| f.metric == "circular_dependency")
        );
    }

    #[test]
    fn contributing_factor_dead_code_with_value_exports() {
        let mut value_exports = rustc_hash::FxHashMap::default();
        value_exports.insert(std::path::PathBuf::from("/src/foo.ts"), 6_usize);
        let scores = vec![make_score(|s| s.dead_code_ratio = 0.7)];
        let aux = TargetAuxData {
            circular_files: &rustc_hash::FxHashSet::default(),
            top_complex_fns: &rustc_hash::FxHashMap::default(),
            entry_points: &rustc_hash::FxHashSet::default(),
            value_export_counts: &value_exports,
            unused_export_names: &rustc_hash::FxHashMap::default(),
            cycle_members: &rustc_hash::FxHashMap::default(),
        };
        let (targets, _) = compute_refactoring_targets(&scores, &aux, &[]);
        assert!(!targets.is_empty());
        let target = &targets[0];
        assert!(target.factors.iter().any(|f| f.metric == "dead_code_ratio"));
    }

    #[test]
    fn contributing_factor_fan_out() {
        let value_exports: rustc_hash::FxHashMap<std::path::PathBuf, usize> =
            rustc_hash::FxHashMap::default();
        let scores = vec![make_score(|s| {
            s.fan_out = 20;
            s.maintainability_index = 50.0;
        })];
        let aux = TargetAuxData {
            circular_files: &rustc_hash::FxHashSet::default(),
            top_complex_fns: &rustc_hash::FxHashMap::default(),
            entry_points: &rustc_hash::FxHashSet::default(),
            value_export_counts: &value_exports,
            unused_export_names: &rustc_hash::FxHashMap::default(),
            cycle_members: &rustc_hash::FxHashMap::default(),
        };
        let (targets, _) = compute_refactoring_targets(&scores, &aux, &[]);
        assert!(!targets.is_empty());
        let target = &targets[0];
        assert!(target.factors.iter().any(|f| f.metric == "fan_out"));
    }

    #[test]
    fn contributing_factor_cognitive_complexity() {
        let mut top_fns = rustc_hash::FxHashMap::default();
        top_fns.insert(
            std::path::PathBuf::from("/src/foo.ts"),
            vec![("complexFn".to_string(), 10u32, 30u16)],
        );
        let value_exports: rustc_hash::FxHashMap<std::path::PathBuf, usize> =
            rustc_hash::FxHashMap::default();
        let scores = vec![make_score(|_| {})];
        let aux = TargetAuxData {
            circular_files: &rustc_hash::FxHashSet::default(),
            top_complex_fns: &top_fns,
            entry_points: &rustc_hash::FxHashSet::default(),
            value_export_counts: &value_exports,
            unused_export_names: &rustc_hash::FxHashMap::default(),
            cycle_members: &rustc_hash::FxHashMap::default(),
        };
        let (targets, _) = compute_refactoring_targets(&scores, &aux, &[]);
        assert!(!targets.is_empty());
        let target = &targets[0];
        assert!(
            target
                .factors
                .iter()
                .any(|f| f.metric == "cognitive_complexity")
        );
    }

    #[test]
    fn no_targets_for_clean_files() {
        let scores = vec![make_score(|s| {
            s.path = std::path::PathBuf::from("/src/clean.ts");
            s.complexity_density = 0.1;
            s.fan_in = 2;
            s.fan_out = 3;
            s.dead_code_ratio = 0.0;
        })];
        let value_exports: rustc_hash::FxHashMap<std::path::PathBuf, usize> =
            rustc_hash::FxHashMap::default();
        let aux = TargetAuxData {
            circular_files: &rustc_hash::FxHashSet::default(),
            top_complex_fns: &rustc_hash::FxHashMap::default(),
            entry_points: &rustc_hash::FxHashSet::default(),
            value_export_counts: &value_exports,
            unused_export_names: &rustc_hash::FxHashMap::default(),
            cycle_members: &rustc_hash::FxHashMap::default(),
        };
        let (targets, _) = compute_refactoring_targets(&scores, &aux, &[]);
        assert!(targets.is_empty());
    }

    #[test]
    fn rule_split_high_impact_moderate_fan_in_many_functions() {
        // Rule 3 alternate path: fan_in >= p75 AND function_count >= 5
        let score = make_score(|s| {
            s.complexity_density = 0.5;
            s.fan_in = 10; // equals p75 in default thresholds
            s.function_count = 8;
        });
        let t = default_thresholds();
        let result = try_match_rules(&score, None, false, false, None, 0, &t);
        assert!(result.is_some());
        let (cat, _) = result.unwrap();
        assert!(matches!(cat, RecommendationCategory::SplitHighImpact));
    }
}
