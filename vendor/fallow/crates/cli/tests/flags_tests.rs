mod common;

use common::run_fallow;

#[test]
fn feature_flag_suppression_next_line() {
    let out = run_fallow(
        "flags",
        "feature-flag-suppression",
        &["--no-cache", "--format", "json"],
    );
    let json: serde_json::Value =
        serde_json::from_str(&out.stdout).expect("valid JSON from flags command");

    let flags = json["feature_flags"]
        .as_array()
        .expect("feature_flags array");

    let flag_names: Vec<&str> = flags
        .iter()
        .filter_map(|f| f["flag_name"].as_str())
        .collect();

    assert!(
        !flag_names.contains(&"FEATURE_DARK_MODE"),
        "FEATURE_DARK_MODE should be suppressed via // fallow-ignore-next-line feature-flag, found: {flag_names:?}"
    );
    assert!(
        flag_names.contains(&"FEATURE_NEW_CHECKOUT"),
        "FEATURE_NEW_CHECKOUT should still be reported (not suppressed), found: {flag_names:?}"
    );
}

#[test]
fn feature_flag_suppression_file_wide() {
    let out = run_fallow(
        "flags",
        "feature-flag-suppression",
        &["--no-cache", "--format", "json"],
    );
    let json: serde_json::Value =
        serde_json::from_str(&out.stdout).expect("valid JSON from flags command");

    let total = json["total_flags"]
        .as_u64()
        .expect("total_flags should be a number");

    assert_eq!(
        total, 1,
        "only 1 flag should remain after suppression (FEATURE_DARK_MODE suppressed)"
    );
}
