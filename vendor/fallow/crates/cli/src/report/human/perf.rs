use colored::Colorize;
use fallow_core::trace::PipelineTimings;

pub(in crate::report) fn print_performance_human(t: &PipelineTimings) {
    for line in build_performance_human_lines(t) {
        eprintln!("{line}");
    }
}

/// Build human-readable output lines for pipeline performance timings.
pub(in crate::report) fn build_performance_human_lines(t: &PipelineTimings) -> Vec<String> {
    let mut lines = Vec::new();

    lines.push(String::new());
    lines.push(
        "┌─ Pipeline Performance ─────────────────────────────"
            .dimmed()
            .to_string(),
    );
    lines.push(
        format!(
            "│  discover files:   {:>8.1}ms  ({} files)",
            t.discover_files_ms, t.file_count
        )
        .dimmed()
        .to_string(),
    );
    lines.push(
        format!(
            "│  workspaces:       {:>8.1}ms  ({} workspaces)",
            t.workspaces_ms, t.workspace_count
        )
        .dimmed()
        .to_string(),
    );
    lines.push(
        format!("│  plugins:          {:>8.1}ms", t.plugins_ms)
            .dimmed()
            .to_string(),
    );
    lines.push(
        format!("│  script analysis:  {:>8.1}ms", t.script_analysis_ms)
            .dimmed()
            .to_string(),
    );
    let cache_detail = if t.cache_hits > 0 {
        format!(", {} cached, {} parsed", t.cache_hits, t.cache_misses)
    } else {
        String::new()
    };
    lines.push(
        format!(
            "│  parse/extract:    {:>8.1}ms  ({} modules{})",
            t.parse_extract_ms, t.module_count, cache_detail
        )
        .dimmed()
        .to_string(),
    );
    lines.push(
        format!("│  cache update:     {:>8.1}ms", t.cache_update_ms)
            .dimmed()
            .to_string(),
    );
    lines.push(
        format!(
            "│  entry points:     {:>8.1}ms  ({} entries)",
            t.entry_points_ms, t.entry_point_count
        )
        .dimmed()
        .to_string(),
    );
    lines.push(
        format!("│  resolve imports:  {:>8.1}ms", t.resolve_imports_ms)
            .dimmed()
            .to_string(),
    );
    lines.push(
        format!("│  build graph:      {:>8.1}ms", t.build_graph_ms)
            .dimmed()
            .to_string(),
    );
    lines.push(
        format!("│  analyze:          {:>8.1}ms", t.analyze_ms)
            .dimmed()
            .to_string(),
    );
    if let Some(duplication_ms) = t.duplication_ms {
        lines.push(
            format!("│  duplication:      {duplication_ms:>8.1}ms")
                .dimmed()
                .to_string(),
        );
    }
    lines.push(
        "│  ────────────────────────────────────────────────"
            .dimmed()
            .to_string(),
    );
    lines.push(
        format!("│  TOTAL:            {:>8.1}ms", t.total_ms)
            .bold()
            .dimmed()
            .to_string(),
    );
    lines.push(
        "└───────────────────────────────────────────────────"
            .dimmed()
            .to_string(),
    );
    lines.push(String::new());

    lines
}

pub(in crate::report) fn print_health_performance_human(t: &crate::health_types::HealthTimings) {
    for line in build_health_performance_lines(t) {
        eprintln!("{line}");
    }
}

fn build_health_performance_lines(t: &crate::health_types::HealthTimings) -> Vec<String> {
    let mut lines = Vec::new();

    lines.push(String::new());
    lines.push(
        "┌─ Health Pipeline Performance ─────────────────────"
            .dimmed()
            .to_string(),
    );
    lines.push(
        format!("│  config:           {:>8.1}ms", t.config_ms)
            .dimmed()
            .to_string(),
    );
    lines.push(
        format!("│  discover files:   {:>8.1}ms", t.discover_ms)
            .dimmed()
            .to_string(),
    );
    lines.push(
        format!("│  parse/extract:    {:>8.1}ms", t.parse_ms)
            .dimmed()
            .to_string(),
    );
    lines.push(
        format!("│  complexity:       {:>8.1}ms", t.complexity_ms)
            .dimmed()
            .to_string(),
    );
    lines.push(
        format!("│  file scores:      {:>8.1}ms", t.file_scores_ms)
            .dimmed()
            .to_string(),
    );
    let cache_note = if t.git_churn_cache_hit {
        " (cached)"
    } else {
        " (cold)"
    };
    lines.push(
        format!(
            "│  git churn:        {:>8.1}ms{}",
            t.git_churn_ms, cache_note
        )
        .dimmed()
        .to_string(),
    );
    lines.push(
        format!("│  hotspots:         {:>8.1}ms", t.hotspots_ms)
            .dimmed()
            .to_string(),
    );
    lines.push(
        format!("│  duplication:      {:>8.1}ms", t.duplication_ms)
            .dimmed()
            .to_string(),
    );
    lines.push(
        format!("│  targets:          {:>8.1}ms", t.targets_ms)
            .dimmed()
            .to_string(),
    );
    lines.push(
        "│  ────────────────────────────────────────────────"
            .dimmed()
            .to_string(),
    );
    lines.push(
        format!("│  TOTAL:            {:>8.1}ms", t.total_ms)
            .bold()
            .dimmed()
            .to_string(),
    );
    lines.push(
        "└───────────────────────────────────────────────────"
            .dimmed()
            .to_string(),
    );
    lines.push(String::new());

    lines
}

#[cfg(test)]
mod tests {
    use super::super::plain;
    use super::*;

    #[test]
    fn performance_output_contains_all_pipeline_stages() {
        let timings = PipelineTimings {
            discover_files_ms: 12.5,
            file_count: 100,
            workspaces_ms: 3.2,
            workspace_count: 3,
            plugins_ms: 1.0,
            script_analysis_ms: 2.5,
            parse_extract_ms: 45.0,
            module_count: 80,
            cache_hits: 0,
            cache_misses: 80,
            cache_update_ms: 5.0,
            entry_points_ms: 0.5,
            entry_point_count: 10,
            resolve_imports_ms: 8.0,
            build_graph_ms: 15.0,
            analyze_ms: 10.0,
            duplication_ms: Some(7.2),
            total_ms: 102.7,
        };
        let lines = build_performance_human_lines(&timings);
        let text = plain(&lines);
        assert!(text.contains("Pipeline Performance"));
        assert!(text.contains("discover files"));
        assert!(text.contains("100 files"));
        assert!(text.contains("workspaces"));
        assert!(text.contains("3 workspaces"));
        assert!(text.contains("plugins"));
        assert!(text.contains("script analysis"));
        assert!(text.contains("parse/extract"));
        assert!(text.contains("80 modules"));
        assert!(text.contains("cache update"));
        assert!(text.contains("entry points"));
        assert!(text.contains("10 entries"));
        assert!(text.contains("resolve imports"));
        assert!(text.contains("build graph"));
        assert!(text.contains("analyze"));
        assert!(text.contains("duplication"));
        assert!(text.contains("7.2"));
        assert!(text.contains("TOTAL"));
        assert!(text.contains("102.7"));
    }

    #[test]
    fn performance_output_shows_cache_detail_when_cache_hits_nonzero() {
        let timings = PipelineTimings {
            discover_files_ms: 10.0,
            file_count: 50,
            workspaces_ms: 1.0,
            workspace_count: 1,
            plugins_ms: 0.5,
            script_analysis_ms: 1.0,
            parse_extract_ms: 20.0,
            module_count: 40,
            cache_hits: 30,
            cache_misses: 10,
            cache_update_ms: 2.0,
            entry_points_ms: 0.3,
            entry_point_count: 5,
            resolve_imports_ms: 3.0,
            build_graph_ms: 5.0,
            analyze_ms: 4.0,
            duplication_ms: None,
            total_ms: 46.8,
        };
        let lines = build_performance_human_lines(&timings);
        let text = plain(&lines);
        assert!(text.contains("30 cached"));
        assert!(text.contains("10 parsed"));
    }

    #[test]
    fn performance_output_omits_cache_detail_when_no_cache_hits() {
        let timings = PipelineTimings {
            discover_files_ms: 10.0,
            file_count: 50,
            workspaces_ms: 1.0,
            workspace_count: 1,
            plugins_ms: 0.5,
            script_analysis_ms: 1.0,
            parse_extract_ms: 20.0,
            module_count: 40,
            cache_hits: 0,
            cache_misses: 40,
            cache_update_ms: 2.0,
            entry_points_ms: 0.3,
            entry_point_count: 5,
            resolve_imports_ms: 3.0,
            build_graph_ms: 5.0,
            analyze_ms: 4.0,
            duplication_ms: None,
            total_ms: 46.8,
        };
        let lines = build_performance_human_lines(&timings);
        let text = plain(&lines);
        assert!(!text.contains("cached"));
        assert!(!text.contains("parsed"));
    }
}
