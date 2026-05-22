#[path = "common/mod.rs"]
mod common;

use common::run_fallow_combined;

#[test]
fn combined_human_output_hides_internal_info_logs_when_rust_log_is_empty() {
    let output = run_fallow_combined("basic-project", &["--summary"]);
    assert_ne!(
        output.code, 2,
        "combined run should not hard-fail: stdout={} stderr={}",
        output.stdout, output.stderr
    );

    let combined = format!("{}\n{}", output.stdout, output.stderr);
    assert!(
        !combined.contains("active plugins"),
        "human output should not leak plugin tracing: {combined}"
    );
    assert!(
        !combined.contains("incremental cache stats"),
        "human output should not leak cache tracing: {combined}"
    );
    assert!(
        !combined.contains(" INFO ")
            && !combined.contains(" DEBUG ")
            && !combined.contains(" TRACE "),
        "human output should stay free of tracing levels: {combined}"
    );
    assert!(
        output.stderr.contains("Dead Code") || output.stderr.contains("■ Metrics"),
        "expected the normal combined human report on stderr: {}",
        output.stderr
    );
}
