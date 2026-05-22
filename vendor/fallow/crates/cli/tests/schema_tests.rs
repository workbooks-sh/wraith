#[path = "common/mod.rs"]
mod common;

use common::{parse_json, run_fallow_raw};

// ---------------------------------------------------------------------------
// schema command
// ---------------------------------------------------------------------------

#[test]
fn schema_outputs_valid_json() {
    let output = run_fallow_raw(&["schema"]);
    assert_eq!(output.code, 0, "schema should exit 0");
    let json = parse_json(&output);
    assert!(json.is_object(), "schema output should be a JSON object");
}

#[test]
fn schema_has_name_and_version() {
    let output = run_fallow_raw(&["schema"]);
    let json = parse_json(&output);
    assert_eq!(
        json["name"].as_str().unwrap(),
        "fallow",
        "schema name should be 'fallow'"
    );
    assert!(
        json.get("version").is_some(),
        "schema should have version field"
    );
}

#[test]
fn schema_has_commands_array() {
    let output = run_fallow_raw(&["schema"]);
    let json = parse_json(&output);
    let commands = json["commands"].as_array().unwrap();
    assert!(!commands.is_empty(), "schema should list commands");

    let names: Vec<&str> = commands
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"audit"), "should list audit command");
    assert!(
        names.contains(&"dead-code"),
        "should list dead-code command"
    );
    assert!(names.contains(&"health"), "should list health command");
    assert!(names.contains(&"dupes"), "should list dupes command");
    assert!(names.contains(&"explain"), "should list explain command");
}

#[test]
fn explain_outputs_rule_guidance_as_json() {
    let output = run_fallow_raw(&["explain", "unused-exports", "--format", "json", "--quiet"]);
    assert_eq!(output.code, 0, "explain should exit 0: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["id"].as_str(), Some("fallow/unused-export"));
    assert!(json["example"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(json["how_to_fix"].as_str().is_some_and(|s| !s.is_empty()));
}

#[test]
fn explain_compact_is_single_line() {
    let output = run_fallow_raw(&[
        "explain",
        "unused-exports",
        "--format",
        "compact",
        "--quiet",
    ]);
    assert_eq!(output.code, 0, "explain should exit 0: {}", output.stderr);
    assert_eq!(
        output.stdout.trim(),
        "explain:fallow/unused-export:Export is never imported:https://docs.fallow.tools/explanations/dead-code#unused-exports"
    );
}

#[test]
fn explain_markdown_is_markdown() {
    let output = run_fallow_raw(&[
        "explain",
        "unused-exports",
        "--format",
        "markdown",
        "--quiet",
    ]);
    assert_eq!(output.code, 0, "explain should exit 0: {}", output.stderr);
    assert!(output.stdout.starts_with("# Unused Exports\n\n"));
    assert!(output.stdout.contains("## Why it matters"));
    assert!(
        output
            .stdout
            .contains("[Docs](https://docs.fallow.tools/explanations/dead-code#unused-exports)")
    );
}

#[test]
fn explain_rejects_unknown_issue_type() {
    let output = run_fallow_raw(&["explain", "not-a-real-rule", "--format", "json", "--quiet"]);
    assert_eq!(output.code, 2, "unknown explain id should exit 2");
    let json = parse_json(&output);
    assert_eq!(json["error"].as_bool(), Some(true));
}

#[test]
fn schema_has_issue_types() {
    let output = run_fallow_raw(&["schema"]);
    let json = parse_json(&output);
    let types = json["issue_types"].as_array().unwrap();
    assert!(!types.is_empty(), "schema should list issue types");
}

#[test]
fn schema_has_exit_codes() {
    let output = run_fallow_raw(&["schema"]);
    let json = parse_json(&output);
    assert!(
        json.get("exit_codes").is_some(),
        "schema should document exit codes"
    );
}

// ---------------------------------------------------------------------------
// config-schema command
// ---------------------------------------------------------------------------

#[test]
fn config_schema_outputs_valid_json() {
    let output = run_fallow_raw(&["config-schema"]);
    assert_eq!(output.code, 0, "config-schema should exit 0");
    let json = parse_json(&output);
    assert!(json.is_object(), "config-schema should be a JSON object");
}

#[test]
fn config_schema_is_json_schema() {
    let output = run_fallow_raw(&["config-schema"]);
    let json = parse_json(&output);
    assert!(
        json.get("$schema").is_some() || json.get("type").is_some(),
        "config-schema should be a JSON Schema document"
    );
}

// ---------------------------------------------------------------------------
// plugin-schema command
// ---------------------------------------------------------------------------

#[test]
fn plugin_schema_outputs_valid_json() {
    let output = run_fallow_raw(&["plugin-schema"]);
    assert_eq!(output.code, 0, "plugin-schema should exit 0");
    let json = parse_json(&output);
    assert!(json.is_object(), "plugin-schema should be a JSON object");
}

#[test]
fn plugin_schema_is_json_schema() {
    let output = run_fallow_raw(&["plugin-schema"]);
    let json = parse_json(&output);
    assert!(
        json.get("$schema").is_some() || json.get("type").is_some(),
        "plugin-schema should be a JSON Schema document"
    );
}
