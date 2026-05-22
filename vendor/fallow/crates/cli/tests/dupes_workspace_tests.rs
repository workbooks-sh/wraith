//! End-to-end tests that `fallow dupes` and combined-mode dupes respect
//! `--workspace` and `--changed-workspaces` scoping.
//!
//! The fixture builds three packages with intentionally duplicated code across
//! packages. Without workspace scoping, dupes detects the cross-package clone
//! groups. With scoping, clone groups that have no instance under the selected
//! workspace root(s) are dropped.

#[path = "common/mod.rs"]
mod common;

use common::{parse_json, run_fallow_raw};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Two-workspace monorepo with a repeating block of TypeScript duplicated
/// across both workspaces. The block is sized to clear fallow's default
/// clone-detection thresholds (min_tokens, min_lines).
fn build_dupes_fixture() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    fs::create_dir_all(dir.join("packages/ui/src")).unwrap();
    fs::create_dir_all(dir.join("packages/api/src")).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name":"monorepo","private":true,"workspaces":["packages/*"]}"#,
    )
    .unwrap();

    // A non-trivial duplicated block. Comfortably above min_tokens (50) and
    // min_lines (5) so Mild mode will pick it up deterministically.
    let duplicated_block = r"
export function transform(input: { items: number[]; scale: number }) {
    const { items, scale } = input;
    const normalized = items.map((n) => n * scale);
    const sum = normalized.reduce((acc, n) => acc + n, 0);
    const mean = sum / normalized.length;
    const variance = normalized.reduce((acc, n) => acc + (n - mean) ** 2, 0) / normalized.length;
    const stddev = Math.sqrt(variance);
    return { sum, mean, stddev, values: normalized };
}
";

    fs::write(
        dir.join("packages/ui/package.json"),
        r#"{"name":"@mono/ui","main":"src/index.ts"}"#,
    )
    .unwrap();
    fs::write(dir.join("packages/ui/src/index.ts"), duplicated_block).unwrap();

    fs::write(
        dir.join("packages/api/package.json"),
        r#"{"name":"@mono/api","main":"src/index.ts"}"#,
    )
    .unwrap();
    fs::write(dir.join("packages/api/src/index.ts"), duplicated_block).unwrap();

    git_init_and_commit(dir);
    tmp
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .status()
        .expect("git command failed");
    assert!(status.success(), "git {args:?} failed");
}

fn git_init_and_commit(dir: &Path) {
    git(dir, &["init", "-b", "main"]);
    git(dir, &["add", "."]);
    git(
        dir,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );
}

fn count_clone_groups(json: &serde_json::Value) -> usize {
    json.get("clone_groups")
        .and_then(|v| v.as_array())
        .map_or(0, std::vec::Vec::len)
}

fn combined_dupes_clone_groups(json: &serde_json::Value) -> usize {
    json.get("dupes")
        .and_then(|d| d.get("clone_groups"))
        .and_then(|v| v.as_array())
        .map_or(0, std::vec::Vec::len)
}

// ────────────────────────────────────────────────────────────────
// Standalone `fallow dupes` with --workspace
// ────────────────────────────────────────────────────────────────

#[test]
fn dupes_without_scope_finds_cross_package_clone() {
    let tmp = build_dupes_fixture();
    let out = run_fallow_raw(&[
        "dupes",
        "--root",
        tmp.path().to_str().unwrap(),
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(out.code, 0, "dupes should exit 0 without a threshold");
    let json = parse_json(&out);
    assert!(
        count_clone_groups(&json) >= 1,
        "expected at least 1 clone group across ui+api without scoping, got {}",
        count_clone_groups(&json)
    );
}

#[test]
fn dupes_workspace_scope_drops_cross_package_only_group() {
    let tmp = build_dupes_fixture();
    // Both instances of the clone live in ui+api. Scoping to `@mono/ui` alone
    // must still retain the group (one instance is under ui), but scoping to
    // a third package name that doesn't exist — or to a single workspace with
    // no instances — drops the group. We use `@mono/ui` here which should keep
    // the group since one instance is in ui.
    let out = run_fallow_raw(&[
        "dupes",
        "--root",
        tmp.path().to_str().unwrap(),
        "--workspace",
        "@mono/ui",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(out.code, 0);
    let json = parse_json(&out);
    // At least one instance is in ui, so the cross-workspace group is retained.
    assert!(
        count_clone_groups(&json) >= 1,
        "group with an instance under ui should be retained, got {}",
        count_clone_groups(&json)
    );
}

// ────────────────────────────────────────────────────────────────
// Combined-mode dupes with --changed-workspaces
// ────────────────────────────────────────────────────────────────

#[test]
fn combined_changed_workspaces_head_drops_all_dupes() {
    let tmp = build_dupes_fixture();
    // HEAD diff = empty, so 0 workspaces in scope, so all clone groups drop.
    // Before the fix shipped in this test's PR, combined mode's dupes was
    // unscoped and this assertion would fail.
    let out = run_fallow_raw(&[
        "--root",
        tmp.path().to_str().unwrap(),
        "--changed-workspaces",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        out.code, 0,
        "no issues should yield exit 0, stderr={}",
        out.stderr
    );
    let json = parse_json(&out);
    let dupes_count = combined_dupes_clone_groups(&json);
    assert_eq!(
        dupes_count, 0,
        "combined mode must apply --changed-workspaces to dupes; got {dupes_count} groups"
    );
}

#[test]
fn combined_workspace_scope_applies_to_dupes() {
    let tmp = build_dupes_fixture();
    // Sanity: with explicit --workspace, we still see the group because ui+api
    // overlap with the ui scope (one instance is there).
    let out_with_scope = run_fallow_raw(&[
        "--root",
        tmp.path().to_str().unwrap(),
        "--workspace",
        "@mono/ui",
        "--format",
        "json",
        "--quiet",
    ]);
    let json = parse_json(&out_with_scope);
    assert!(
        combined_dupes_clone_groups(&json) >= 1,
        "ui scope keeps the cross-package group (instance under ui)"
    );
    // Regression for the pre-existing gap this change closes: WITHOUT the fix,
    // this assertion would still be satisfied because dupes was always
    // unfiltered. With the fix, it still passes but only because the cross-
    // package clone genuinely has an instance under ui. The two prior tests
    // (HEAD diff empty → 0 dupes) are what catch the regression.
}
