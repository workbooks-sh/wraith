use super::helpers::*;

// ---- should_skip_dependency tests ----

#[test]
fn skip_dep_returns_false_when_no_guard_matches() {
    let (root_flagged, script_used, plugin_referenced, ignore_deps, workspace_names) = empty_sets();
    let result = should_skip_dependency(
        "some-package",
        &root_flagged,
        &script_used,
        &plugin_referenced,
        &ignore_deps,
        &workspace_names,
        |_| false,
    );
    assert!(!result);
}

#[test]
fn skip_dep_when_root_flagged() {
    let (mut root_flagged, script_used, plugin_referenced, ignore_deps, workspace_names) =
        empty_sets();
    root_flagged.insert("lodash".to_string());
    assert!(should_skip_dependency(
        "lodash",
        &root_flagged,
        &script_used,
        &plugin_referenced,
        &ignore_deps,
        &workspace_names,
        |_| false,
    ));
}

#[test]
fn skip_dep_when_script_used() {
    let (root_flagged, mut script_used, plugin_referenced, ignore_deps, workspace_names) =
        empty_sets();
    script_used.insert("eslint");
    assert!(should_skip_dependency(
        "eslint",
        &root_flagged,
        &script_used,
        &plugin_referenced,
        &ignore_deps,
        &workspace_names,
        |_| false,
    ));
}

#[test]
fn skip_dep_when_plugin_referenced() {
    let (root_flagged, script_used, mut plugin_referenced, ignore_deps, workspace_names) =
        empty_sets();
    plugin_referenced.insert("tailwindcss");
    assert!(should_skip_dependency(
        "tailwindcss",
        &root_flagged,
        &script_used,
        &plugin_referenced,
        &ignore_deps,
        &workspace_names,
        |_| false,
    ));
}

#[test]
fn skip_dep_when_in_ignore_list() {
    let (root_flagged, script_used, plugin_referenced, mut ignore_deps, workspace_names) =
        empty_sets();
    ignore_deps.insert("my-internal-package");
    assert!(should_skip_dependency(
        "my-internal-package",
        &root_flagged,
        &script_used,
        &plugin_referenced,
        &ignore_deps,
        &workspace_names,
        |_| false,
    ));
}

#[test]
fn skip_dep_when_workspace_name() {
    let (root_flagged, script_used, plugin_referenced, ignore_deps, mut workspace_names) =
        empty_sets();
    workspace_names.insert("@myorg/shared");
    assert!(should_skip_dependency(
        "@myorg/shared",
        &root_flagged,
        &script_used,
        &plugin_referenced,
        &ignore_deps,
        &workspace_names,
        |_| false,
    ));
}

#[test]
fn skip_dep_when_used_in_workspace() {
    let (root_flagged, script_used, plugin_referenced, ignore_deps, workspace_names) = empty_sets();
    assert!(should_skip_dependency(
        "react",
        &root_flagged,
        &script_used,
        &plugin_referenced,
        &ignore_deps,
        &workspace_names,
        |dep| dep == "react",
    ));
}

#[test]
fn skip_dep_closure_receives_correct_dep_name() {
    let (root_flagged, script_used, plugin_referenced, ignore_deps, workspace_names) = empty_sets();
    // Closure that only returns true for "axios"
    let result = should_skip_dependency(
        "axios",
        &root_flagged,
        &script_used,
        &plugin_referenced,
        &ignore_deps,
        &workspace_names,
        |dep| dep == "axios",
    );
    assert!(result);

    // Different dep name should not match
    let result = should_skip_dependency(
        "express",
        &root_flagged,
        &script_used,
        &plugin_referenced,
        &ignore_deps,
        &workspace_names,
        |dep| dep == "axios",
    );
    assert!(!result);
}

#[test]
fn skip_dep_no_match_with_similar_names() {
    let (mut root_flagged, script_used, plugin_referenced, ignore_deps, workspace_names) =
        empty_sets();
    root_flagged.insert("lodash-es".to_string());
    // "lodash" is not the same as "lodash-es"
    assert!(!should_skip_dependency(
        "lodash",
        &root_flagged,
        &script_used,
        &plugin_referenced,
        &ignore_deps,
        &workspace_names,
        |_| false,
    ));
}

#[test]
fn skip_dep_multiple_guards_match() {
    // When multiple guards would match, function still returns true
    let (mut root_flagged, mut script_used, plugin_referenced, ignore_deps, workspace_names) =
        empty_sets();
    root_flagged.insert("eslint".to_string());
    script_used.insert("eslint");
    assert!(should_skip_dependency(
        "eslint",
        &root_flagged,
        &script_used,
        &plugin_referenced,
        &ignore_deps,
        &workspace_names,
        |_| false,
    ));
}

// ---- Additional coverage: should_skip_dependency with empty string ----

#[test]
fn skip_dep_empty_string_no_match() {
    let (root_flagged, script_used, plugin_referenced, ignore_deps, workspace_names) = empty_sets();
    assert!(!should_skip_dependency(
        "",
        &root_flagged,
        &script_used,
        &plugin_referenced,
        &ignore_deps,
        &workspace_names,
        |_| false,
    ));
}
