//! Integration tests: build synthetic crates in a tempdir, then invoke
//! the `wraith` binary against them and assert on stdout.

use std::path::Path;
use std::process::Command;

fn wraith_bin() -> String {
    env!("CARGO_BIN_EXE_wraith").to_string()
}

fn run_wraith(root: &Path, args: &[&str]) -> (String, String, i32) {
    let out = Command::new(wraith_bin())
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .expect("failed to run wraith");
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    )
}

fn write(root: &Path, rel: &str, content: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

fn write_root_workspace(root: &Path, members: &[&str]) {
    let body = format!(
        "[workspace]\nresolver = \"2\"\nmembers = [{}]\n",
        members
            .iter()
            .map(|m| format!("\"{}\"", m))
            .collect::<Vec<_>>()
            .join(", ")
    );
    write(root, "Cargo.toml", &body);
}

fn write_basic_lib_crate(root: &Path, name: &str, src: &str, deps: &str) {
    let toml = format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n\n[dependencies]\n{deps}\n"
    );
    write(root, &format!("{name}/Cargo.toml"), &toml);
    write(root, &format!("{name}/src/lib.rs"), src);
}

#[test]
fn dead_code_finds_unreferenced_pub_fn() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["a"]);
    write_basic_lib_crate(
        root,
        "a",
        r#"
pub fn used_one() -> i32 { 1 }
pub fn dead_one() -> i32 { 2 }
fn _consumer() -> i32 { used_one() }
"#,
        "",
    );
    let (out, _err, code) = run_wraith(root, &["dead-code"]);
    assert!(out.contains("dead_one"), "expected dead_one in output, got: {}", out);
    assert!(!out.contains("used_one"), "used_one should not appear, got: {}", out);
    assert_eq!(code, 1);
}

#[test]
fn dead_code_ignores_main_entrypoint() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["b"]);
    let toml = "[package]\nname = \"b\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[[bin]]\nname = \"b\"\npath = \"src/main.rs\"\n";
    write(root, "b/Cargo.toml", toml);
    write(root, "b/src/main.rs", "fn main() { println!(\"hi\"); }\n");
    let (out, _err, code) = run_wraith(root, &["dead-code"]);
    assert_eq!(code, 0, "expected no findings, got: {}", out);
    assert!(out.contains("no findings"), "got: {}", out);
}

#[test]
fn dead_code_ignores_tests_module() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["c"]);
    write_basic_lib_crate(
        root,
        "c",
        r#"
pub fn alive() -> i32 { 1 }
fn _consumer() -> i32 { alive() }

#[cfg(test)]
mod tests {
    pub fn looks_like_dead() -> i32 { 42 }
}
"#,
        "",
    );
    let (out, _err, code) = run_wraith(root, &["dead-code"]);
    assert!(!out.contains("looks_like_dead"), "tests module should be skipped: {}", out);
    assert_eq!(code, 0);
}

#[test]
fn unused_deps_finds_uncited_crate() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["d"]);
    let toml = "[package]\nname = \"d\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n\n[dependencies]\nserde = \"1\"\nlog = \"0.4\"\n";
    write(root, "d/Cargo.toml", toml);
    write(
        root,
        "d/src/lib.rs",
        r#"
use serde::Serialize;

#[derive(Serialize)]
pub struct Foo { pub x: u32 }
fn _consumer() -> Foo { Foo { x: 1 } }
"#,
    );
    let (out, _err, code) = run_wraith(root, &["unused-deps"]);
    assert!(out.contains("log"), "expected log in output, got: {}", out);
    assert!(!out.contains("`serde`"), "serde should not be unused, got: {}", out);
    assert_eq!(code, 1);
}

#[test]
fn init_writes_wraithrc() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["e"]);
    write_basic_lib_crate(root, "e", "pub fn x() {}\n", "");
    let (_out, _err, _code) = run_wraith(root, &["init"]);
    let cfg_path = root.join(".wraithrc.json");
    assert!(cfg_path.exists(), ".wraithrc.json was not created");
    let body = std::fs::read_to_string(cfg_path).unwrap();
    assert!(body.contains("ignore"));
    assert!(body.contains("allow_dead"));
}

#[test]
fn json_format_outputs_valid_json() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["f"]);
    write_basic_lib_crate(
        root,
        "f",
        "pub fn dead_f() -> i32 { 3 }\n",
        "",
    );
    let (out, _err, code) = run_wraith(root, &["--format", "json", "dead-code"]);
    assert_eq!(code, 1);
    let _: serde_json::Value = serde_json::from_str(&out).expect("invalid json");
    assert!(out.contains("dead_f"));
}

#[test]
fn circular_deps_detects_module_cycle() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["circ"]);
    let toml = "[package]\nname = \"circ\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n";
    write(root, "circ/Cargo.toml", toml);
    write(
        root,
        "circ/src/lib.rs",
        "pub mod a;\npub mod b;\n",
    );
    write(
        root,
        "circ/src/a.rs",
        "use crate::b::B;\npub struct A;\npub fn use_b() -> B { B }\n",
    );
    write(
        root,
        "circ/src/b.rs",
        "use crate::a::A;\npub struct B;\npub fn use_a() -> A { A }\n",
    );
    let (out, _err, code) = run_wraith(root, &["circular-deps"]);
    assert!(out.contains("cycle"), "expected cycle in output: {}", out);
    assert_eq!(code, 1);
}

#[test]
fn circular_deps_skips_crate_root_reference() {
    // Regression: a leaf module reading a constant from the crate root
    // (a normal Rust pattern) was being reported as a cycle because the
    // synthetic "crate" node SCC-collapsed everything reachable via
    // `mod foo;` declarations in lib.rs.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["rootref"]);
    let toml = "[package]\nname = \"rootref\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n";
    write(root, "rootref/Cargo.toml", toml);
    write(
        root,
        "rootref/src/lib.rs",
        "pub mod foo;\npub const ROOT_CONST: u32 = 42;\n",
    );
    write(
        root,
        "rootref/src/foo.rs",
        "pub fn bar() -> u32 { crate::ROOT_CONST }\n",
    );
    let (out, _err, code) = run_wraith(root, &["circular-deps"]);
    assert_eq!(code, 0, "expected no cycles, got: {}", out);
    assert!(!out.contains("cycle"), "should not report a cycle: {}", out);
}

#[test]
fn circular_deps_disambiguates_shared_leaf_names() {
    // Regression for wb-5lgj.24. Two sibling modules each define their
    // own `pub fn new() -> Self` on their own struct. There is no
    // cross-module call between them. The old leaf-name resolver would
    // resolve every `Foo::new()` and `Bar::new()` against both modules
    // (because both define a symbol named `new`), producing phantom
    // bidirectional edges between foo and bar and reporting a cycle.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["share"]);
    let toml = "[package]\nname = \"share\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n";
    write(root, "share/Cargo.toml", toml);
    write(root, "share/src/lib.rs", "pub mod foo;\npub mod bar;\n");
    write(
        root,
        "share/src/foo.rs",
        "pub struct Foo;\nimpl Foo { pub fn new() -> Self { Foo } }\npub fn use_foo() { let _ = Foo::new(); }\n",
    );
    write(
        root,
        "share/src/bar.rs",
        "pub struct Bar;\nimpl Bar { pub fn new() -> Self { Bar } }\npub fn use_bar() { let _ = Bar::new(); }\n",
    );
    let (out, _err, code) = run_wraith(root, &["circular-deps"]);
    assert_eq!(code, 0, "expected no cycles, got: {}", out);
    assert!(
        !out.contains("cycle"),
        "shared leaf `new` should not create a phantom cycle: {}",
        out
    );
}

#[test]
fn dupes_finds_near_clones() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["dup"]);
    write_basic_lib_crate(
        root,
        "dup",
        r#"
pub fn alpha(x: i32, y: i32) -> i32 {
    let mut acc = 0;
    for i in 0..x {
        if i % 2 == 0 {
            acc += y;
        } else {
            acc -= y;
        }
    }
    acc + x * y - 7 + 11 * 3
}

pub fn beta(x: i32, y: i32) -> i32 {
    let mut acc = 0;
    for i in 0..x {
        if i % 2 == 0 {
            acc += y;
        } else {
            acc -= y;
        }
    }
    acc + x * y - 7 + 11 * 3
}
"#,
        "",
    );
    // lower min_tokens via config
    let cfg = r#"{"ignore":[],"allow_dead":[],"allow_unused_deps":[],"treat_pub_crate_as_internal":false,"duplicates":{"min_tokens":20,"similarity_threshold":0.85},"complexity":{"cyclomatic":15,"cognitive":25},"boundaries":[]}"#;
    write(root, ".wraithrc.json", cfg);
    let (out, _err, code) = run_wraith(root, &["dupes"]);
    assert!(out.contains("alpha") || out.contains("beta"), "got: {}", out);
    assert_eq!(code, 1);
}

#[test]
fn health_flags_complex_fn() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["hot"]);
    // build a fn with many branches
    let mut body = String::from("pub fn busy(x: i32) -> i32 {\n");
    for i in 0..20 {
        body.push_str(&format!("    if x == {i} {{ return {i}; }}\n"));
    }
    body.push_str("    0\n}\n");
    write_basic_lib_crate(root, "hot", &body, "");
    let (out, _err, code) = run_wraith(root, &["health"]);
    assert!(out.contains("busy"), "expected busy in output: {}", out);
    assert_eq!(code, 1);
}

#[test]
fn boundaries_blocks_disallowed_import() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["lib_y", "guarded"]);
    write_basic_lib_crate(root, "lib_y", "pub fn ok() {}\n", "");
    let guarded_toml = "[package]\nname = \"guarded\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n\n[dependencies]\nlib_y = { path = \"../lib_y\" }\n";
    write(root, "guarded/Cargo.toml", guarded_toml);
    write(root, "guarded/src/lib.rs", "use lib_y::ok;\npub fn run() { ok(); }\n");
    let cfg = r#"{"ignore":[],"allow_dead":[],"allow_unused_deps":[],"treat_pub_crate_as_internal":false,"duplicates":{"min_tokens":40,"similarity_threshold":0.85},"complexity":{"cyclomatic":15,"cognitive":25},"boundaries":[{"from":"guarded","allow":[],"deny":["lib_y"]}]}"#;
    write(root, ".wraithrc.json", cfg);
    let (out, _err, code) = run_wraith(root, &["boundaries"]);
    assert!(out.contains("boundary"), "expected violation: {}", out);
    assert_eq!(code, 1);
}

#[test]
fn fix_dry_run_lists_edits_without_changing_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["fixme"]);
    write_basic_lib_crate(
        root,
        "fixme",
        "pub fn alive() {}\npub fn dead_a() {}\nfn _u() { alive() }\n",
        "",
    );
    let before = std::fs::read_to_string(root.join("fixme/src/lib.rs")).unwrap();
    let (out, _err, _code) = run_wraith(root, &["fix"]);
    let after = std::fs::read_to_string(root.join("fixme/src/lib.rs")).unwrap();
    assert!(out.contains("dead_a"), "expected dead_a in plan: {}", out);
    assert_eq!(before, after, "dry-run should not modify files");
}

#[test]
fn init_with_ci_writes_workflow() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["w"]);
    write_basic_lib_crate(root, "w", "pub fn x() {}\n", "");
    let (_o, _e, _c) = run_wraith(root, &["init", "--ci", "github"]);
    assert!(root.join(".github/workflows/wraith.yml").exists());
}

#[test]
fn health_show_branches_prints_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["br"]);
    write_basic_lib_crate(
        root,
        "br",
        r#"
pub fn driver(x: i32, ys: &[i32]) -> i32 {
    let mut acc = 0;
    for y in ys {
        if *y > x {
            match y {
                0 => acc += 1,
                _ => acc -= 1,
            }
        }
    }
    acc
}
"#,
        "",
    );
    let (out, _err, code) = run_wraith(root, &["health", "--fn", "driver", "--show-branches"]);
    assert_eq!(code, 0, "expected success, got: {} / {}", code, out);
    assert!(out.contains("br::driver"), "expected qualified name in output: {}", out);
    assert!(out.contains("for"), "expected for-loop node: {}", out);
    assert!(out.contains("if"), "expected if node: {}", out);
    assert!(out.contains("match"), "expected match node: {}", out);
    // tree indentation present
    assert!(out.contains("  if"), "expected nested if under for: {}", out);
}

#[test]
fn dupes_clusters_transitively() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["clust"]);
    // Three near-identical fns. Pair-mode would emit 3 pair findings;
    // cluster-mode (default) should emit ONE cluster of size 3.
    write_basic_lib_crate(
        root,
        "clust",
        r#"
pub fn ext_one(m: &str) -> &'static str {
    if m.contains("png") { return "png"; }
    if m.contains("jpg") { return "jpg"; }
    if m.contains("gif") { return "gif"; }
    "bin"
}

pub fn ext_two(m: &str) -> &'static str {
    if m.contains("png") { return "png"; }
    if m.contains("jpg") { return "jpg"; }
    if m.contains("gif") { return "gif"; }
    "bin"
}

pub fn ext_three(m: &str) -> &'static str {
    if m.contains("png") { return "png"; }
    if m.contains("jpg") { return "jpg"; }
    if m.contains("gif") { return "gif"; }
    "bin"
}
"#,
        "",
    );
    let cfg = r#"{"ignore":[],"allow_dead":[],"allow_unused_deps":[],"treat_pub_crate_as_internal":false,"duplicates":{"min_tokens":20,"similarity_threshold":0.85},"complexity":{"cyclomatic":15,"cognitive":25},"boundaries":[]}"#;
    write(root, ".wraithrc.json", cfg);
    let (out, _err, code) = run_wraith(root, &["dupes"]);
    assert_eq!(code, 1, "expected findings, got: {}", out);
    assert!(out.contains("duplicate cluster"), "expected cluster output, got: {}", out);
    assert!(out.contains("ext_one"), "missing ext_one: {}", out);
    assert!(out.contains("ext_two"), "missing ext_two: {}", out);
    assert!(out.contains("ext_three"), "missing ext_three: {}", out);
    let cluster_count = out.matches("duplicate cluster").count();
    assert_eq!(cluster_count, 1, "expected exactly 1 cluster, got {}: {}", cluster_count, out);
}

#[test]
fn dupes_pairs_mode_dedupes_identical_findings() {
    // wb-5lgj.26 regression: identical pair findings were emitted twice
    // (once per direction). Canonicalize by sorted (a, b) qualified name.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["dd"]);
    write_basic_lib_crate(
        root,
        "dd",
        r#"
pub fn aaa(s: &str) -> usize {
    if s.is_empty() { return 0; }
    if s.len() > 100 { return 100; }
    if s.contains("x") { return 1; }
    s.len()
}
pub fn bbb(s: &str) -> usize {
    if s.is_empty() { return 0; }
    if s.len() > 100 { return 100; }
    if s.contains("x") { return 1; }
    s.len()
}
"#,
        "",
    );
    let cfg = r#"{"ignore":[],"allow_dead":[],"allow_unused_deps":[],"treat_pub_crate_as_internal":false,"duplicates":{"min_tokens":15,"similarity_threshold":0.85},"complexity":{"cyclomatic":15,"cognitive":25},"boundaries":[]}"#;
    write(root, ".wraithrc.json", cfg);
    let (out, _err, _code) = run_wraith(root, &["dupes", "--pairs"]);
    let pair_lines: Vec<&str> = out
        .lines()
        .filter(|l| l.contains("duplicate code"))
        .collect();
    assert_eq!(
        pair_lines.len(),
        1,
        "expected exactly one pair finding, got {}: {:?}",
        pair_lines.len(),
        pair_lines
    );
}

#[test]
fn cross_crate_reference_is_alive() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["lib_x", "consumer"]);
    write_basic_lib_crate(
        root,
        "lib_x",
        "pub fn exported_fn() -> i32 { 1 }\npub fn dead_fn() -> i32 { 2 }\n",
        "",
    );
    let consumer_toml = "[package]\nname = \"consumer\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[[bin]]\nname = \"consumer\"\npath = \"src/main.rs\"\n\n[dependencies]\nlib_x = { path = \"../lib_x\" }\n";
    write(root, "consumer/Cargo.toml", consumer_toml);
    write(
        root,
        "consumer/src/main.rs",
        "use lib_x::exported_fn;\nfn main() { println!(\"{}\", exported_fn()); }\n",
    );
    let (out, _err, code) = run_wraith(root, &["dead-code"]);
    assert!(out.contains("dead_fn"));
    assert!(!out.contains("exported_fn"));
    assert_eq!(code, 1);
}

fn run_wraith_no_root(args: &[&str]) -> (String, String, i32) {
    let out = Command::new(wraith_bin())
        .args(args)
        .output()
        .expect("failed to run wraith");
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    )
}

#[test]
fn extract_fn_pure_read_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let file = root.join("fixture.rs");
    std::fs::write(
        &file,
        "fn outer() {\n    let a = 10;\n    let b = 20;\n    let sum = a + b;\n    let doubled = sum * 2;\n    println!(\"{doubled}\");\n    println!(\"done\");\n}\n",
    )
    .unwrap();
    let (out, err, code) = run_wraith_no_root(&[
        "refactor",
        "extract-fn",
        &format!("{}:4..6", file.display()),
        "--name",
        "compute_and_print",
        "--dry-run",
    ]);
    assert_eq!(code, 0, "stderr: {err}");
    assert!(out.contains("compute_and_print(a, b);"), "out: {out}");
    assert!(
        out.contains("fn compute_and_print(a: i32, b: i32)"),
        "out: {out}"
    );
    assert!(out.contains("let sum = a + b;"), "out: {out}");
    assert!(out.contains("println!(\"done\");"), "out: {out}");
}

// --- extract-fn v2 patterns (wb-5lgj.27) --------------------------------

#[test]
fn extract_fn_v2_handles_early_return() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let file = root.join("fixture.rs");
    std::fs::write(
        &file,
        "fn outer() -> i32 {\n    let a = 10;\n    if a > 5 {\n        return 0;\n    }\n    a + 1\n}\n",
    )
    .unwrap();
    let (out, err, code) = run_wraith_no_root(&[
        "refactor",
        "extract-fn",
        &format!("{}:3..5", file.display()),
        "--name",
        "maybe_bail",
        "--dry-run",
    ]);
    assert_eq!(code, 0, "expected exit 0, got {code}, err: {err}");
    assert!(
        out.contains("fn maybe_bail(a: i32) -> Option<i32>"),
        "out: {out}"
    );
    assert!(out.contains("return Some(0);"), "out: {out}");
    assert!(out.contains("None"), "out: {out}");
    assert!(out.contains("if let Some("), "out: {out}");
}

#[test]
fn extract_fn_v2_handles_try_operator() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let file = root.join("fixture.rs");
    std::fs::write(
        &file,
        "fn maybe() -> Result<i32, ()> { Ok(1) }\nfn outer() -> Result<i32, Box<dyn std::error::Error>> {\n    let raw = 1;\n    let parsed = maybe().map_err(|_| -> Box<dyn std::error::Error> { \"x\".into() })?;\n    Ok(parsed + raw)\n}\n",
    )
    .unwrap();
    let (out, err, code) = run_wraith_no_root(&[
        "refactor",
        "extract-fn",
        &format!("{}:4..4", file.display()),
        "--name",
        "do_parse",
        "--dry-run",
    ]);
    assert_eq!(code, 0, "stderr: {err}");
    assert!(out.contains("-> Result<"), "out: {out}");
    assert!(out.contains("do_parse()?;"), "out: {out}");
}

#[test]
fn extract_fn_v2_handles_await_in_async_fn() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let file = root.join("fixture.rs");
    std::fs::write(
        &file,
        "async fn outer() -> i32 {\n    let a = 1;\n    let b = async_helper(a).await;\n    b + 1\n}\nasync fn async_helper(x: i32) -> i32 { x }\n",
    )
    .unwrap();
    let (out, err, code) = run_wraith_no_root(&[
        "refactor",
        "extract-fn",
        &format!("{}:3..3", file.display()),
        "--name",
        "do_step",
        "--dry-run",
    ]);
    assert_eq!(code, 0, "stderr: {err}");
    assert!(out.contains("async fn do_step("), "out: {out}");
    assert!(out.contains(".await;"), "out: {out}");
}

#[test]
fn extract_fn_v2_refuses_await_in_non_async_fn() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let file = root.join("fixture.rs");
    std::fs::write(
        &file,
        "fn outer() -> i32 {\n    let a = 1;\n    let b = async_helper(a).await;\n    b + 1\n}\nasync fn async_helper(x: i32) -> i32 { x }\n",
    )
    .unwrap();
    let (_out, err, code) = run_wraith_no_root(&[
        "refactor",
        "extract-fn",
        &format!("{}:3..3", file.display()),
        "--name",
        "do_step",
        "--dry-run",
    ]);
    assert_eq!(code, 64, "expected exit 64, got {code}, err: {err}");
    assert!(err.contains(".await"), "err: {err}");
}

#[test]
fn extract_fn_v2_handles_self_method() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let file = root.join("fixture.rs");
    std::fs::write(
        &file,
        "struct Foo { x: i32 }\nimpl Foo {\n    fn outer(&self) -> i32 {\n        let a = 10;\n        let b = self.x + a;\n        b\n    }\n}\n",
    )
    .unwrap();
    let (out, err, code) = run_wraith_no_root(&[
        "refactor",
        "extract-fn",
        &format!("{}:5..5", file.display()),
        "--name",
        "sum_with_x",
        "--dry-run",
    ]);
    assert_eq!(code, 0, "stderr: {err}");
    assert!(out.contains("Self::sum_with_x("), "out: {out}");
    assert!(out.contains("self_ref: &Self"), "out: {out}");
}

#[test]
fn extract_fn_v2_handles_match_arm_body() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let file = root.join("fixture.rs");
    // Local enum so the resolver can find variant field types.
    std::fs::write(
        &file,
        "enum E { Some(i32), None }\nfn outer(o: E) {\n    match o {\n        E::Some(y) => {\n            let z = y + 1;\n            println!(\"{z}\");\n        }\n        E::None => {}\n    }\n}\n",
    )
    .unwrap();
    let (out, err, code) = run_wraith_no_root(&[
        "refactor",
        "extract-fn",
        &format!("{}:4..7", file.display()),
        "--name",
        "handle_some",
        "--dry-run",
    ]);
    assert_eq!(code, 0, "stderr: {err}");
    assert!(out.contains("fn handle_some(y: i32)"), "out: {out}");
    assert!(out.contains("handle_some(y)"), "out: {out}");
}

// wb-5lgj.41 — extract-fn must hoist LOCAL `use` statements from the
// enclosing fn body when the extracted code depends on them. Repro
// (wavelet bin): `use wavelet::{compose, ...};` declared inside run_image
// → extract a match-arm body that calls `compose::composite_over(...)`
// → extracted top-level fn fails to compile with E0433 because the
// local `use` doesn't travel with the body.
#[test]
fn extract_fn_v2_hoists_local_uses_when_body_depends_on_them() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let file = root.join("fixture.rs");
    // outer() has a LOCAL `use std::cmp::max;`. Extracting the body
    // that calls max() must include `use std::cmp::max;` in the
    // extracted fn body.
    std::fs::write(
        &file,
        "fn outer(a: i32, b: i32) -> i32 {\n    use std::cmp::max;\n    let result = max(a, b);\n    result + 1\n}\n",
    )
    .unwrap();
    let (out, err, code) = run_wraith_no_root(&[
        "refactor",
        "extract-fn",
        &format!("{}:3..4", file.display()),
        "--name",
        "biggest_plus_one",
        "--dry-run",
    ]);
    assert_eq!(code, 0, "stderr: {err}");
    // The extracted fn body must contain the hoisted use.
    let body_start = out
        .find("fn biggest_plus_one")
        .expect(&format!("no extracted fn in: {out}"));
    let rest = &out[body_start..];
    let brace_open = rest.find('{').expect("no opening brace");
    let brace_close_rel = rest[brace_open..].find("\n}").expect("no closing brace");
    let body_block = &rest[brace_open + 1..brace_open + brace_close_rel];
    assert!(
        body_block.contains("use std :: cmp :: max")
            || body_block.contains("use std::cmp::max"),
        "extracted body missing hoisted `use std::cmp::max;`:\n{body_block}"
    );
}

// wb-5lgj.42 — extracted match-arm body must be 4-space-indented at the
// top level, regardless of how deep the original arm body was. Pre-fix,
// extracting from a 12-space-indented match arm preserved the 12-space
// indent on line 2+ (line 1 was column-stripped by the syn span slicer,
// so min_indent computed 0 → no dedent happened).
#[test]
fn extract_fn_v2_match_arm_body_dedents_to_top_level_indent() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let file = root.join("fixture.rs");
    // Match arm body nested 12 spaces deep (fn body 4 + match 4 + arm 4).
    // Multi-statement so line 1 vs line 2+ differ in column-strip behavior.
    std::fs::write(
        &file,
        "enum E { Some(i32), None }\nfn outer(o: E) {\n    match o {\n        E::Some(y) => {\n            let z = y + 1;\n            let w = z * 2;\n            println!(\"{w}\");\n        }\n        E::None => {}\n    }\n}\n",
    )
    .unwrap();
    let (out, err, code) = run_wraith_no_root(&[
        "refactor",
        "extract-fn",
        &format!("{}:4..8", file.display()),
        "--name",
        "handle_some",
        "--dry-run",
    ]);
    assert_eq!(code, 0, "stderr: {err}");

    // Find the extracted fn block + scan its body lines. Every non-blank
    // body line must start with EXACTLY 4 leading spaces — not 12, not 16.
    let body_start = out.find("fn handle_some").expect(&format!("no extracted fn in: {out}"));
    let rest = &out[body_start..];
    let brace_open = rest.find('{').expect("no opening brace");
    let brace_close_rel = rest[brace_open..].find("\n}").expect("no closing brace line");
    let body_block = &rest[brace_open + 1..brace_open + brace_close_rel];

    for line in body_block.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let leading = line.chars().take_while(|c| *c == ' ').count();
        assert_eq!(
            leading, 4,
            "body line should be 4-space indented, got {leading}: {line:?}\nfull body:\n{body_block}"
        );
    }
}

// wb-5lgj.40 — match-arm-body extraction regression. Verifies:
//   - struct-variant binding types resolve from the local enum decl
//     (NOT silently defaulted to i32)
//   - return type is inferred from tail expressions (`ExitCode::*`)
//   - body text does NOT include the `=>` arm separator
//   - callsite args follow destructuring pattern declaration order
#[test]
fn extract_fn_v2_match_arm_body_preserves_field_types_and_arg_order() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let file = root.join("fixture.rs");
    let body = "use std::path::PathBuf;\nuse std::process::ExitCode;\nenum Op {\n    A { x: PathBuf, y: Option<i32> },\n    B { name: String },\n}\nfn handle(op: Op) -> ExitCode {\n    match op {\n        Op::A { x, y } => match maybe_run(&x, y) {\n            Ok(_) => ExitCode::SUCCESS,\n            Err(_) => ExitCode::from(2),\n        },\n        Op::B { name } => {\n            let _ = name;\n            ExitCode::SUCCESS\n        }\n    }\n}\nfn maybe_run(_x: &PathBuf, _y: Option<i32>) -> Result<(), ()> { Ok(()) }\n";
    std::fs::write(&file, body).unwrap();
    // Arm body lives on lines 9..12 (the inner `match maybe_run(...) { ... }`).
    let (out, err, code) = run_wraith_no_root(&[
        "refactor",
        "extract-fn",
        &format!("{}:9..12", file.display()),
        "--name",
        "handle_a",
        "--dry-run",
    ]);
    assert_eq!(code, 0, "expected exit 0, got {code}, stderr: {err}");
    // Param types come from the variant decl, NOT defaulted to i32.
    assert!(
        out.contains("fn handle_a(x: PathBuf, y: Option<i32>)"),
        "out: {out}"
    );
    // Return type inferred from `ExitCode::SUCCESS` + `ExitCode::from(2)`.
    assert!(out.contains("-> ExitCode"), "out: {out}");
    // Body must NOT have leaked `=>` from the arm header. Inspect the
    // bytes between `fn handle_a(...) -> ExitCode {` and the matching
    // close brace of that fn — the only `=>` that should appear there
    // is from inner match arms inside the body (e.g. `Ok(_) =>`).
    let body_start = out.find("fn handle_a").expect("handle_a fn missing");
    let after_open = out[body_start..].find("{").expect("fn open brace missing");
    let fn_text = &out[body_start + after_open..];
    let leak_pat = "} =>";
    assert!(
        !fn_text.contains(leak_pat),
        "extracted body leaked `{}` from the arm header: {}",
        leak_pat,
        fn_text
    );
    // Callsite args in source order: x, y (NOT y, x).
    assert!(out.contains("handle_a(x, y),"), "out: {out}");
}

#[test]
fn extract_fn_v2_match_arm_body_refuses_when_types_unresolvable() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let file = root.join("fixture.rs");
    // Discriminant is `Option<i32>` — std::option::Option isn't in this
    // file, so the resolver can't find Some's tuple-field types. We
    // must REFUSE rather than silently lie with `i32`.
    let body = "fn outer(o: std::collections::HashMap<i32, i32>) {\n    match o.get(&1) {\n        Some(y) => {\n            let z = *y + 1;\n            let _ = z;\n        }\n        None => {}\n    }\n}\n";
    std::fs::write(&file, body).unwrap();
    let (_out, err, code) = run_wraith_no_root(&[
        "refactor",
        "extract-fn",
        &format!("{}:3..6", file.display()),
        "--name",
        "handle_some",
        "--dry-run",
    ]);
    assert_eq!(code, 64, "expected refusal (64), got {code}, stderr: {err}");
    assert!(
        err.contains("types can't be resolved"),
        "expected types-can't-be-resolved error, got: {err}"
    );
}

#[test]
fn extract_fn_detects_mutation_as_mut_ref() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let file = root.join("fixture.rs");
    std::fs::write(
        &file,
        "fn outer() {\n    let mut x = 5;\n    x += 1;\n    println!(\"{x}\");\n}\n",
    )
    .unwrap();
    let (out, err, code) = run_wraith_no_root(&[
        "refactor",
        "extract-fn",
        &format!("{}:3..3", file.display()),
        "--name",
        "bump",
        "--dry-run",
    ]);
    assert_eq!(code, 0, "stderr: {err}");
    assert!(out.contains("fn bump(x: &mut i32)"), "out: {out}");
    assert!(out.contains("bump(&mut x);"), "out: {out}");
}

// --- dedupe-cluster (wb-5lgj.28) ----------------------------------------

fn write_dedupe_workspace(root: &Path) {
    write_root_workspace(root, &["crate_a", "crate_b", "crate_c"]);
    let body = r#"
pub fn pick_ext(m: &str) -> &'static str {
    if m.contains("png") { return "png"; }
    if m.contains("jpg") { return "jpg"; }
    if m.contains("gif") { return "gif"; }
    "bin"
}

pub fn caller_a() -> &'static str { pick_ext("a.png") }
"#;
    write_basic_lib_crate(root, "crate_a", body, "");

    let body_b = r#"
pub fn pick_ext(m: &str) -> &'static str {
    if m.contains("png") { return "png"; }
    if m.contains("jpg") { return "jpg"; }
    if m.contains("gif") { return "gif"; }
    "bin"
}

pub fn caller_b() -> &'static str { pick_ext("b.jpg") }
"#;
    write_basic_lib_crate(
        root,
        "crate_b",
        body_b,
        "crate_a = { path = \"../crate_a\" }\n",
    );

    let body_c = r#"
pub fn pick_ext(m: &str) -> &'static str {
    if m.contains("png") { return "png"; }
    if m.contains("jpg") { return "jpg"; }
    if m.contains("gif") { return "gif"; }
    "bin"
}

pub fn caller_c() -> &'static str { pick_ext("c.gif") }
"#;
    write_basic_lib_crate(
        root,
        "crate_c",
        body_c,
        "crate_a = { path = \"../crate_a\" }\n",
    );

    let cfg = r#"{"ignore":[],"allow_dead":[],"allow_unused_deps":[],"treat_pub_crate_as_internal":false,"duplicates":{"min_tokens":15,"similarity_threshold":0.85},"complexity":{"cyclomatic":15,"cognitive":25},"boundaries":[]}"#;
    write(root, ".wraithrc.json", cfg);
}

#[test]
fn dedupe_cluster_collapses_byte_identical_members() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_dedupe_workspace(root);

    let (out, err, code) = run_wraith(
        root,
        &["refactor", "dedupe-cluster", "0", "--canonical", "crate_a"],
    );
    assert_eq!(code, 0, "expected success, got code={code}\nstdout: {out}\nstderr: {err}");

    let a = std::fs::read_to_string(root.join("crate_a/src/lib.rs")).unwrap();
    let b = std::fs::read_to_string(root.join("crate_b/src/lib.rs")).unwrap();
    let c = std::fs::read_to_string(root.join("crate_c/src/lib.rs")).unwrap();

    assert!(a.contains("pub fn pick_ext"), "crate_a should still define pick_ext: {a}");
    assert!(!b.contains("pub fn pick_ext"), "crate_b should no longer define pick_ext: {b}");
    assert!(!c.contains("pub fn pick_ext"), "crate_c should no longer define pick_ext: {c}");
    assert!(b.contains("use crate_a::pick_ext"), "crate_b missing use import: {b}");
    assert!(c.contains("use crate_a::pick_ext"), "crate_c missing use import: {c}");

    // Callers still reference pick_ext by leaf name (since canonical leaf == local leaf).
    assert!(b.contains("pick_ext(\"b.jpg\")"), "crate_b caller broken: {b}");
    assert!(c.contains("pick_ext(\"c.gif\")"), "crate_c caller broken: {c}");

    // Whole workspace still type-checks.
    let status = Command::new("cargo")
        .arg("check")
        .arg("--quiet")
        .current_dir(root)
        .status()
        .expect("cargo check failed to spawn");
    assert!(status.success(), "cargo check should pass after dedupe");
}

#[test]
fn dedupe_cluster_refuses_non_identical_members() {
    // Near-clones (similarity ~0.95) — body differs by one literal —
    // should be refused with exit 64 and a pointer to wb-5lgj.30.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["near"]);
    write_basic_lib_crate(
        root,
        "near",
        r#"
pub fn aaa(s: &str) -> usize {
    if s.is_empty() { return 0; }
    if s.len() > 100 { return 100; }
    if s.contains("x") { return 1; }
    if s.contains("y") { return 2; }
    if s.contains("z") { return 3; }
    s.len()
}
pub fn bbb(s: &str) -> usize {
    if s.is_empty() { return 0; }
    if s.len() > 100 { return 100; }
    if s.contains("x") { return 1; }
    if s.contains("y") { return 2; }
    if s.contains("q") { return 99; }
    s.len()
}
"#,
        "",
    );
    let cfg = r#"{"ignore":[],"allow_dead":[],"allow_unused_deps":[],"treat_pub_crate_as_internal":false,"duplicates":{"min_tokens":15,"similarity_threshold":0.85},"complexity":{"cyclomatic":15,"cognitive":25},"boundaries":[]}"#;
    write(root, ".wraithrc.json", cfg);

    let (_out, err, code) = run_wraith(root, &["refactor", "dedupe-cluster", "0"]);
    assert_eq!(code, 64, "expected exit 64, got {code}, err: {err}");
    assert!(
        err.contains("non-identical members") && err.contains("wb-5lgj.30"),
        "stderr should point to wb-5lgj.30: {err}"
    );
}

#[test]
fn dedupe_cluster_dry_run_does_not_modify_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_dedupe_workspace(root);

    let before_b = std::fs::read_to_string(root.join("crate_b/src/lib.rs")).unwrap();
    let before_c = std::fs::read_to_string(root.join("crate_c/src/lib.rs")).unwrap();

    let (out, _err, code) = run_wraith(
        root,
        &[
            "refactor",
            "dedupe-cluster",
            "0",
            "--canonical",
            "crate_a",
            "--dry-run",
        ],
    );
    assert_eq!(code, 0);
    assert!(
        out.contains("dry-run") && out.contains("would dedupe"),
        "expected dry-run notice: {out}"
    );

    let after_b = std::fs::read_to_string(root.join("crate_b/src/lib.rs")).unwrap();
    let after_c = std::fs::read_to_string(root.join("crate_c/src/lib.rs")).unwrap();
    assert_eq!(before_b, after_b, "dry-run must not modify crate_b");
    assert_eq!(before_c, after_c, "dry-run must not modify crate_c");
}

// --- wb-5lgj.37: scope-aware safety check ---------------------------------

#[test]
fn dedupe_cluster_refuses_divergent_free_name_resolutions() {
    // Two crates each define their own `KIND_LIST` const with
    // different contents; the body `check_kind` is byte-identical but
    // resolves to different definitions in each crate. Dedupe MUST
    // refuse with exit 64 and an error naming KIND_LIST.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["scope_a", "scope_b"]);
    write_basic_lib_crate(
        root,
        "scope_a",
        r#"
pub const KIND_LIST: &[&str] = &["png", "jpg", "gif"];
pub fn check_kind(needle: &str) -> bool {
    KIND_LIST.iter().any(|k| needle.contains(k))
}
pub fn caller_a() -> bool { check_kind("png") }
"#,
        "",
    );
    write_basic_lib_crate(
        root,
        "scope_b",
        r#"
pub const KIND_LIST: &[&str] = &["mp4", "mov", "webm"];
pub fn check_kind(needle: &str) -> bool {
    KIND_LIST.iter().any(|k| needle.contains(k))
}
pub fn caller_b() -> bool { check_kind("mp4") }
"#,
        "",
    );
    let cfg = r#"{"ignore":[],"allow_dead":[],"allow_unused_deps":[],"treat_pub_crate_as_internal":false,"duplicates":{"min_tokens":5,"similarity_threshold":0.85},"complexity":{"cyclomatic":15,"cognitive":25},"boundaries":[]}"#;
    write(root, ".wraithrc.json", cfg);

    let (_out, err, code) = run_wraith(root, &["refactor", "dedupe-cluster", "0"]);
    assert_eq!(code, 64, "expected exit 64, got {code}, stderr: {err}");
    assert!(
        err.contains("KIND_LIST"),
        "stderr should name KIND_LIST: {err}"
    );
    assert!(
        err.contains("different definitions"),
        "stderr should explain divergence: {err}"
    );
}

// --- wb-5lgj.39: visibility auto-elevation --------------------------------

fn write_private_helper_workspace(root: &Path) {
    write_root_workspace(root, &["elev_a", "elev_b"]);
    write_basic_lib_crate(
        root,
        "elev_a",
        r#"
fn shared_helper(s: &str) -> usize {
    let mut n = 0;
    for _ in s.chars() { n += 1; }
    n
}
pub fn caller_a() -> usize { shared_helper("hi") }
"#,
        "",
    );
    write_basic_lib_crate(
        root,
        "elev_b",
        r#"
fn shared_helper(s: &str) -> usize {
    let mut n = 0;
    for _ in s.chars() { n += 1; }
    n
}
pub fn caller_b() -> usize { shared_helper("ho") }
"#,
        "elev_a = { path = \"../elev_a\" }\n",
    );
    let cfg = r#"{"ignore":[],"allow_dead":[],"allow_unused_deps":[],"treat_pub_crate_as_internal":false,"duplicates":{"min_tokens":5,"similarity_threshold":0.85},"complexity":{"cyclomatic":15,"cognitive":25},"boundaries":[]}"#;
    write(root, ".wraithrc.json", cfg);
}

#[test]
fn dedupe_cluster_auto_elevates_canonical_visibility() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_private_helper_workspace(root);

    let (out, err, code) = run_wraith(
        root,
        &["refactor", "dedupe-cluster", "0", "--canonical", "elev_a"],
    );
    assert_eq!(code, 0, "expected success, got code={code}\nstdout: {out}\nstderr: {err}");
    assert!(
        out.contains("elevating") && (out.contains("pub") ),
        "expected visibility-elevation notice, got: {out}"
    );

    let a = std::fs::read_to_string(root.join("elev_a/src/lib.rs")).unwrap();
    let b = std::fs::read_to_string(root.join("elev_b/src/lib.rs")).unwrap();
    assert!(
        a.contains("pub fn shared_helper"),
        "canonical should be elevated to pub: {a}"
    );
    assert!(
        !b.contains("fn shared_helper"),
        "elev_b should no longer define shared_helper: {b}"
    );
    assert!(
        b.contains("use elev_a::shared_helper"),
        "elev_b should import canonical: {b}"
    );

    let status = Command::new("cargo")
        .arg("check")
        .arg("--quiet")
        .current_dir(root)
        .status()
        .expect("cargo check failed to spawn");
    assert!(status.success(), "cargo check should pass after dedupe + elevate");
}

#[test]
fn dedupe_cluster_no_elevate_refuses_insufficient_visibility() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_private_helper_workspace(root);

    let (_out, err, code) = run_wraith(
        root,
        &[
            "refactor",
            "dedupe-cluster",
            "0",
            "--canonical",
            "elev_a",
            "--no-elevate",
        ],
    );
    assert_eq!(code, 64, "expected exit 64 with --no-elevate, got {code}, stderr: {err}");
    assert!(
        err.contains("private") && err.contains("pub"),
        "stderr should mention private + pub: {err}"
    );
}

#[test]
fn extract_fn_writes_file_when_not_dry_run() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let file = root.join("fixture.rs");
    std::fs::write(
        &file,
        "fn outer() {\n    let a = 10;\n    let b = 20;\n    let sum = a + b;\n    println!(\"{sum}\");\n}\n",
    )
    .unwrap();
    let (_out, err, code) = run_wraith_no_root(&[
        "refactor",
        "extract-fn",
        &format!("{}:4..5", file.display()),
        "--name",
        "show_sum",
    ]);
    assert_eq!(code, 0, "stderr: {err}");
    let written = std::fs::read_to_string(&file).unwrap();
    assert!(written.contains("show_sum(a, b);"), "file: {written}");
    assert!(
        written.contains("fn show_sum(a: i32, b: i32)"),
        "file: {written}"
    );
}

#[test]
fn health_suggest_extractions_ranks_three_branches() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["sx"]);
    write_basic_lib_crate(
        root,
        "sx",
        r#"
pub fn big_driver(items: &[i32], threshold: i32, dispatch: i32) -> i32 {
    let mut total = 0;
    if threshold > 0 {
        let bumped = threshold + 1;
        let doubled = bumped * 2;
        let tripled = doubled * 3;
        let summed = bumped + doubled + tripled;
        let acc = summed + 1;
        let mixed = acc + 2;
        let again = mixed + 3;
        let more = again + 4;
        total = total + bumped + doubled + summed + acc + mixed + again + more;
    }
    for item in items {
        let scaled = item * 2;
        let plussed = scaled + 1;
        let chopped = plussed - 1;
        let folded = chopped + scaled;
        let blended = folded + 1;
        let buffered = blended + 2;
        let final_step = buffered + 3;
        let stowed = final_step + 4;
        total = total + scaled + plussed + folded + buffered + final_step + stowed;
    }
    match dispatch {
        1 => {
            let mode = 1;
            let p = mode + 1;
            let q = p + 1;
            let r = q + 1;
            let s = r + 1;
            let t = s + 1;
            let u = t + 1;
            let v = u + 1;
            let w = v + 1;
            total = total + p + q + r + s + t + u + v + w;
        }
        2 => {
            let alt = 2;
            let a = alt + 1;
            let b = a + 1;
            let c = b + 1;
            let d = c + 1;
            let e = d + 1;
            let f = e + 1;
            let g = f + 1;
            let h = g + 1;
            total = total + a + b + c + d + e + f + g + h;
        }
        _ => {
            let other = 3;
            let o1 = other + 1;
            let o2 = o1 + 1;
            let o3 = o2 + 1;
            let o4 = o3 + 1;
            let o5 = o4 + 1;
            let o6 = o5 + 1;
            let o7 = o6 + 1;
            let o8 = o7 + 1;
            total = total + o1 + o2 + o3 + o4 + o5 + o6 + o7 + o8;
        }
    }
    total
}
"#,
        "",
    );
    let (out, err, code) = run_wraith(
        root,
        &[
            "--format",
            "json",
            "health",
            "--fn",
            "big_driver",
            "--suggest-extractions",
        ],
    );
    assert_eq!(code, 0, "expected exit 0, stderr: {err}, stdout: {out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("expected valid JSON output");
    let suggestions = v
        .get("suggestions")
        .and_then(|x| x.as_array())
        .expect("suggestions array");
    assert_eq!(
        suggestions.len(),
        3,
        "expected 3 suggestions, got {}: {out}",
        suggestions.len()
    );
    for s in suggestions {
        let sub_cyclo = s.get("sub_cyclo").and_then(|x| x.as_u64()).unwrap_or(0);
        assert!(sub_cyclo >= 2, "sub_cyclo too low: {s}");
        let name = s
            .get("suggested_name")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        assert!(!name.is_empty(), "missing suggested_name: {s}");
        let cmd = s
            .get("extract_fn_command")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        assert!(
            cmd.contains("wraith refactor extract-fn"),
            "missing command: {s}"
        );
    }
    // Each suggestion's suggested_name should reflect the control-flow kind
    // (handle_/process_/run_loop/extract_block_).
    let names: Vec<String> = suggestions
        .iter()
        .map(|s| s.get("suggested_name").unwrap().as_str().unwrap().to_string())
        .collect();
    let has_kind = |prefix: &str| names.iter().any(|n| n.starts_with(prefix));
    assert!(
        has_kind("handle_") || has_kind("process_") || has_kind("run_loop") || has_kind("extract_block_"),
        "no recognizable name prefix in {:?}",
        names
    );
}

// --- queries: ctx / summarize / ls (wb-5lgj.33) ------------------------

#[test]
fn ctx_returns_body_and_callers_for_known_fn() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["q"]);
    write_basic_lib_crate(
        root,
        "q",
        r#"
pub fn target_fn(x: i32) -> i32 {
    x + 1
}

pub fn caller_a() -> i32 { target_fn(1) }
pub fn caller_b() -> i32 { target_fn(2) + caller_a() }
"#,
        "",
    );
    let (out, err, code) = run_wraith(
        root,
        &["--format", "json", "ctx", "target_fn"],
    );
    assert_eq!(code, 0, "expected exit 0, stderr: {err}, stdout: {out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("invalid json");
    let sig = v.get("signature").and_then(|x| x.as_str()).unwrap_or("");
    assert!(sig.contains("target_fn"), "sig missing fn name: {out}");
    let body = v.get("body").and_then(|x| x.as_str()).unwrap_or("");
    assert!(body.contains("x + 1"), "body missing return expr: {out}");
    let callers = v
        .get("callers")
        .and_then(|x| x.as_array())
        .expect("callers array");
    assert!(
        callers.len() >= 1,
        "expected >=1 caller, got {}: {out}",
        callers.len()
    );
    let names: Vec<String> = callers
        .iter()
        .map(|c| c.get("symbol").and_then(|x| x.as_str()).unwrap_or("").to_string())
        .collect();
    assert!(
        names.iter().any(|n| n.ends_with("::caller_a") || n.ends_with("::caller_b")),
        "no expected caller in {:?}",
        names
    );
}

#[test]
fn ctx_no_body_omits_body_field() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["qn"]);
    write_basic_lib_crate(
        root,
        "qn",
        "pub fn alone() -> i32 { 42 }\n",
        "",
    );
    let (out, _err, code) = run_wraith(
        root,
        &["--format", "json", "ctx", "alone", "--no-body"],
    );
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v.get("body").is_none(), "body should be omitted: {out}");
    assert!(v.get("signature").is_some());
}

#[test]
fn summarize_lists_pub_items_with_complexity() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["sm"]);
    let body = r#"
use std::collections::HashMap;

pub struct Thing { pub x: u32 }

pub fn handle(t: &Thing) -> u32 {
    if t.x > 0 { t.x } else { 0 }
}

pub const ANSWER: u32 = 42;

fn private_helper() -> u32 { 1 }
"#;
    write_basic_lib_crate(root, "sm", body, "");
    let file = root.join("sm/src/lib.rs");
    let (out, err, code) = run_wraith(
        root,
        &["--format", "json", "summarize", file.to_str().unwrap()],
    );
    assert_eq!(code, 0, "expected exit 0, stderr: {err}, stdout: {out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("invalid json");
    let items = v
        .get("pub_items")
        .and_then(|x| x.as_array())
        .expect("pub_items array");
    assert!(items.len() >= 3, "expected >=3 pub items, got {}", items.len());
    let kinds: Vec<String> = items
        .iter()
        .map(|i| i.get("kind").and_then(|x| x.as_str()).unwrap_or("").to_string())
        .collect();
    assert!(kinds.contains(&"fn".to_string()), "no fn in {:?}", kinds);
    assert!(kinds.contains(&"struct".to_string()), "no struct in {:?}", kinds);
    assert!(kinds.contains(&"const".to_string()), "no const in {:?}", kinds);
    // private_helper must not appear
    let names: Vec<String> = items
        .iter()
        .map(|i| i.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string())
        .collect();
    assert!(
        !names.contains(&"private_helper".to_string()),
        "private_helper should not appear: {:?}",
        names
    );
    let imports = v.get("imports").and_then(|x| x.as_array()).expect("imports");
    assert!(
        imports.iter().any(|u| u.as_str().unwrap_or("").contains("HashMap")),
        "missing use line: {out}"
    );
}

// --- diff-cluster + extract-shared (wb-5lgj.30) -------------------------

fn write_pick_ext_cluster(root: &Path) {
    write_root_workspace(root, &["picker"]);
    let body = r#"
pub fn for_jpg(m: &str) -> &'static str {
    if m.contains("jpeg") { "jpg" } else { "png" }
}

pub fn for_webp(m: &str) -> &'static str {
    if m.contains("webp") { "webp" } else { "png" }
}

pub fn for_gif(m: &str) -> &'static str {
    if m.contains("gif") { "gif" } else { "png" }
}

pub fn caller_a() -> &'static str { for_jpg("image/jpeg") }
pub fn caller_b() -> &'static str { for_webp("image/webp") }
pub fn caller_c() -> &'static str { for_gif("image/gif") }
"#;
    write_basic_lib_crate(root, "picker", body, "");
    let cfg = r#"{"ignore":[],"allow_dead":[],"allow_unused_deps":[],"treat_pub_crate_as_internal":false,"duplicates":{"min_tokens":8,"similarity_threshold":0.6},"complexity":{"cyclomatic":15,"cognitive":25},"boundaries":[]}"#;
    write(root, ".wraithrc.json", cfg);
}

#[test]
fn diff_cluster_emits_string_literal_divergences() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_pick_ext_cluster(root);

    let (out, err, code) = run_wraith(
        root,
        &["refactor", "diff-cluster", "0", "--format", "json"],
    );
    assert_eq!(code, 0, "stderr: {err}\nstdout: {out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let divs = v["divergences"].as_array().expect("divergences");
    assert!(divs.len() >= 2, "expected ≥2 divergences, got {}: {out}", divs.len());
    for d in divs {
        assert_eq!(d["kind"], "string-literal", "got: {d}");
        assert_eq!(d["suggested_type"], "&'static str", "got: {d}");
    }
    assert_eq!(v["ready_for_extract_shared"], true, "got: {v}");
}

#[test]
fn diff_cluster_refuses_identifier_divergence() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["ident"]);
    // Two fns that differ on a method ident (foo vs bar) — blocking.
    let body = r#"
pub struct W;
impl W {
    pub fn foo(&self) -> i32 { 1 }
    pub fn bar(&self) -> i32 { 2 }
}
pub fn one(w: &W) -> i32 { let v = w.foo(); v + 1 }
pub fn two(w: &W) -> i32 { let v = w.bar(); v + 1 }
pub fn cx() -> i32 { one(&W) + two(&W) }
"#;
    write_basic_lib_crate(root, "ident", body, "");
    let cfg = r#"{"ignore":[],"allow_dead":[],"allow_unused_deps":[],"treat_pub_crate_as_internal":false,"duplicates":{"min_tokens":8,"similarity_threshold":0.6},"complexity":{"cyclomatic":15,"cognitive":25},"boundaries":[]}"#;
    write(root, ".wraithrc.json", cfg);

    let (out, err, code) = run_wraith(
        root,
        &["refactor", "diff-cluster", "0", "--format", "json"],
    );
    assert_eq!(code, 0, "stderr: {err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["ready_for_extract_shared"], false, "got: {v}");
    let blockers = v["blockers"].as_array().expect("blockers");
    assert!(!blockers.is_empty(), "expected blockers: {v}");
}

#[test]
fn extract_shared_round_trip_replaces_cluster() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_pick_ext_cluster(root);

    let mapping = r#"{
      "for_jpg":  {"needle": "\"jpeg\"", "ext": "\"jpg\""},
      "for_webp": {"needle": "\"webp\"", "ext": "\"webp\""},
      "for_gif":  {"needle": "\"gif\"",  "ext": "\"gif\""}
    }"#;
    let (out, err, code) = run_wraith(
        root,
        &[
            "refactor",
            "extract-shared",
            "0",
            "--signature",
            "fn pick_ext(needle: &'static str, ext: &'static str) -> &'static str",
            "--param-mapping",
            mapping,
            "--extract-to",
            "crate::util",
        ],
    );
    assert_eq!(code, 0, "stderr: {err}\nstdout: {out}");

    let lib = std::fs::read_to_string(root.join("picker/src/lib.rs")).unwrap();
    assert!(!lib.contains("pub fn for_jpg"), "for_jpg should be gone:\n{lib}");
    assert!(!lib.contains("pub fn for_webp"), "for_webp should be gone:\n{lib}");
    assert!(!lib.contains("pub fn for_gif"), "for_gif should be gone:\n{lib}");

    let util = std::fs::read_to_string(root.join("picker/src/util.rs")).unwrap();
    assert!(util.contains("fn pick_ext"), "util.rs missing pick_ext:\n{util}");

    // Call sites rewritten.
    assert!(lib.contains("pick_ext"), "callers should reference pick_ext:\n{lib}");
    assert!(!lib.contains("for_jpg(\"image/jpeg\")"), "old caller still present:\n{lib}");

    let status = Command::new("cargo")
        .arg("check")
        .arg("--quiet")
        .current_dir(root)
        .status()
        .expect("cargo check spawn");
    assert!(status.success(), "cargo check should pass post-extract-shared");
}

#[test]
fn extract_shared_missing_mapping_exits_64() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_pick_ext_cluster(root);

    let lib_before = std::fs::read_to_string(root.join("picker/src/lib.rs")).unwrap();

    // Mapping intentionally omits `for_gif`.
    let mapping = r#"{
      "for_jpg":  {"needle": "\"jpeg\"", "ext": "\"jpg\""},
      "for_webp": {"needle": "\"webp\"", "ext": "\"webp\""}
    }"#;
    let (_out, err, code) = run_wraith(
        root,
        &[
            "refactor",
            "extract-shared",
            "0",
            "--signature",
            "fn pick_ext(needle: &'static str, ext: &'static str) -> &'static str",
            "--param-mapping",
            mapping,
            "--extract-to",
            "crate::util",
        ],
    );
    assert_eq!(code, 64, "stderr: {err}");
    assert!(
        err.contains("param-mapping missing keys"),
        "stderr should mention missing keys: {err}"
    );

    let lib_after = std::fs::read_to_string(root.join("picker/src/lib.rs")).unwrap();
    assert_eq!(lib_before, lib_after, "files must be unchanged on refusal");
}

#[test]
fn extract_shared_dry_run_does_not_modify_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_pick_ext_cluster(root);

    let before = std::fs::read_to_string(root.join("picker/src/lib.rs")).unwrap();

    let mapping = r#"{
      "for_jpg":  {"needle": "\"jpeg\"", "ext": "\"jpg\""},
      "for_webp": {"needle": "\"webp\"", "ext": "\"webp\""},
      "for_gif":  {"needle": "\"gif\"",  "ext": "\"gif\""}
    }"#;
    let (out, _err, code) = run_wraith(
        root,
        &[
            "refactor",
            "extract-shared",
            "0",
            "--signature",
            "fn pick_ext(needle: &'static str, ext: &'static str) -> &'static str",
            "--param-mapping",
            mapping,
            "--extract-to",
            "crate::util",
            "--dry-run",
        ],
    );
    assert_eq!(code, 0);
    assert!(out.contains("dry-run"), "dry-run notice: {out}");

    let after = std::fs::read_to_string(root.join("picker/src/lib.rs")).unwrap();
    assert_eq!(before, after, "dry-run must not modify lib.rs");
    assert!(
        !root.join("picker/src/util.rs").exists(),
        "dry-run must not create util.rs"
    );
}

#[test]
fn ls_filters_by_kind_and_pattern() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["lsq"]);
    write_basic_lib_crate(
        root,
        "lsq",
        r#"
pub fn run_one() {}
pub fn run_two() {}
pub fn other_thing() {}
pub struct Run {}
"#,
        "",
    );
    let (out, err, code) = run_wraith(
        root,
        &["--format", "json", "ls", "run_*", "--kind", "fn"],
    );
    assert_eq!(code, 0, "stderr: {err}, stdout: {out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let results = v
        .get("results")
        .and_then(|x| x.as_array())
        .expect("results array");
    let names: Vec<String> = results
        .iter()
        .map(|r| r.get("symbol").and_then(|x| x.as_str()).unwrap_or("").to_string())
        .collect();
    assert!(
        names.iter().any(|n| n.ends_with("::run_one")),
        "missing run_one in {:?}",
        names
    );
    assert!(
        names.iter().any(|n| n.ends_with("::run_two")),
        "missing run_two in {:?}",
        names
    );
    assert!(
        !names.iter().any(|n| n.ends_with("::other_thing")),
        "other_thing should be filtered out by pattern: {:?}",
        names
    );
    // kind filter should exclude the Run struct
    for r in results {
        assert_eq!(
            r.get("kind").and_then(|x| x.as_str()),
            Some("fn"),
            "non-fn leaked: {r}"
        );
    }
}

#[test]
fn health_suggest_extractions_filters_refusal_patterns() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["sxr"]);
    // Three branches; the if-branch contains an early `return`. v2 can
    // lift that, so all three pass the feasibility filter — but the
    // if-branch is tagged `v2-only-with-rewrite`.
    write_basic_lib_crate(
        root,
        "sxr",
        r#"
pub fn refusal_driver(items: &[i32], dispatch: i32) -> i32 {
    let mut total = 0;
    if dispatch < 0 {
        let a = 1;
        let b = a + 1;
        let c = b + 1;
        let d = c + 1;
        let e = d + 1;
        let f = e + 1;
        let g = f + 1;
        let h = g + 1;
        total = total + a + b + c + d + e + f + g + h;
        return total;
    }
    for item in items {
        let scaled = item * 2;
        let plussed = scaled + 1;
        let chopped = plussed - 1;
        let folded = chopped + scaled;
        let blended = folded + 1;
        let buffered = blended + 2;
        let final_step = buffered + 3;
        let stowed = final_step + 4;
        total = total + scaled + plussed + folded + buffered + final_step + stowed;
    }
    match dispatch {
        1 => {
            let mode = 1;
            let p = mode + 1;
            let q = p + 1;
            let r = q + 1;
            let s = r + 1;
            let t = s + 1;
            let u = t + 1;
            let v = u + 1;
            let w = v + 1;
            total = total + p + q + r + s + t + u + v + w;
        }
        _ => {
            let other = 3;
            let o1 = other + 1;
            let o2 = o1 + 1;
            let o3 = o2 + 1;
            let o4 = o3 + 1;
            let o5 = o4 + 1;
            let o6 = o5 + 1;
            let o7 = o6 + 1;
            let o8 = o7 + 1;
            total = total + o1 + o2 + o3 + o4 + o5 + o6 + o7 + o8;
        }
    }
    total
}
"#,
        "",
    );
    let (out, err, code) = run_wraith(
        root,
        &[
            "--format",
            "json",
            "health",
            "--fn",
            "refusal_driver",
            "--suggest-extractions",
        ],
    );
    assert_eq!(code, 0, "expected exit 0, stderr: {err}, stdout: {out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    let suggestions = v
        .get("suggestions")
        .and_then(|x| x.as_array())
        .expect("suggestions array");
    assert_eq!(
        suggestions.len(),
        3,
        "expected all 3 suggestions (v2 acceptor lifts early-return), got {}: {out}",
        suggestions.len()
    );
    let feas: Vec<&str> = suggestions
        .iter()
        .map(|s| s.get("feasibility").and_then(|x| x.as_str()).unwrap_or(""))
        .collect();
    assert!(
        feas.contains(&"v2-only-with-rewrite"),
        "expected at least one v2-only-with-rewrite, got {:?}",
        feas
    );
    assert!(
        feas.iter().any(|f| *f == "ok"),
        "expected at least one ok feasibility, got {:?}",
        feas
    );
}

fn fixture_root() -> std::path::PathBuf {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples/small-test-crate")
}

#[test]
fn graph_crate_deps_returns_workspace_shape() {
    let root = fixture_root();
    let (out, err, code) = run_wraith(
        &root,
        &["graph", "crate-deps", "--format=json"],
    );
    assert_eq!(code, 0, "stderr: {err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid json");
    assert_eq!(v["query"], "crate-deps");
    let nodes = v["nodes"].as_array().expect("nodes");
    let names: Vec<String> = nodes
        .iter()
        .map(|n| n.as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"crate-a".to_string()), "got: {names:?}");
    assert!(names.contains(&"crate-b".to_string()), "got: {names:?}");
    let edges = v["edges"].as_array().expect("edges");
    assert!(!edges.is_empty(), "expected at least one edge, got: {out}");
    assert!(
        edges
            .iter()
            .any(|e| e["from"] == "crate-b" && e["to"] == "crate-a"),
        "expected crate-b -> crate-a edge, got: {out}"
    );
}

#[test]
fn graph_callers_returns_non_empty_for_used_fn() {
    let root = fixture_root();
    let (out, err, code) = run_wraith(
        &root,
        &["graph", "callers", "crate_a::used_function", "--format=json"],
    );
    assert_eq!(code, 0, "stderr: {err}, stdout: {out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid json");
    assert_eq!(v["query"], "callers");
    let results = v["results"].as_array().expect("results");
    assert!(!results.is_empty(), "expected at least one caller, got: {out}");
    assert!(
        results.iter().any(|r| {
            r["symbol"]
                .as_str()
                .map(|s| s.contains("crate_b::main"))
                .unwrap_or(false)
        }),
        "expected crate_b::main as a caller, got: {out}"
    );
}

#[test]
fn graph_callees_returns_referenced_symbols() {
    let root = fixture_root();
    let (out, err, code) = run_wraith(
        &root,
        &[
            "graph",
            "callees",
            "crate_a::used_function",
            "--format=json",
        ],
    );
    assert_eq!(code, 0, "stderr: {err}, stdout: {out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid json");
    assert_eq!(v["query"], "callees");
    let results = v["results"].as_array().expect("results");
    assert!(
        results.iter().any(|r| {
            r["symbol"]
                .as_str()
                .map(|s| s.contains("UsedStruct"))
                .unwrap_or(false)
        }),
        "expected UsedStruct as callee, got: {out}"
    );
}

#[test]
fn graph_blast_radius_returns_transitive_dependents() {
    let root = fixture_root();
    let (out, err, code) = run_wraith(
        &root,
        &[
            "graph",
            "blast-radius",
            "crate_a::used_function",
            "--format=json",
        ],
    );
    assert_eq!(code, 0, "stderr: {err}, stdout: {out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid json");
    assert_eq!(v["query"], "blast-radius");
    let results = v["results"].as_array().expect("results");
    assert!(!results.is_empty(), "expected non-empty blast radius, got: {out}");
}

#[test]
fn graph_reverse_deps_returns_dependents() {
    let root = fixture_root();
    let (out, err, code) = run_wraith(
        &root,
        &["graph", "reverse-deps", "crate-a", "--format=json"],
    );
    assert_eq!(code, 0, "stderr: {err}, stdout: {out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid json");
    assert_eq!(v["query"], "reverse-deps");
    let results = v["results"].as_array().expect("results");
    assert!(
        results.iter().any(|r| r["crate_name"]
            .as_str()
            .map(|s| s.contains("crate_b"))
            .unwrap_or(false)),
        "expected crate_b as dependent of crate_a, got: {out}"
    );
}
// ───────────────────────── `wraith deps` family ─────────────────────────

const DUP_LOCK: &str = r#"
version = 3

[[package]]
name = "myws"
version = "0.1.0"
dependencies = [
 "serde 1.0.190",
 "log",
]

[[package]]
name = "log"
version = "0.4.20"
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
fn deps_duplicates_detects_two_serde_versions() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["myws"]);
    write_basic_lib_crate(root, "myws", "pub fn x() {}\n", "");
    std::fs::write(root.join("Cargo.lock"), DUP_LOCK).unwrap();

    let (out, _err, code) = run_wraith(root, &["deps", "duplicates", "--format", "json"]);
    // exit 1 when duplicates exist
    assert_eq!(code, 1, "expected exit 1, got {code}; stdout={out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    let arr = v.as_array().expect("array");
    assert_eq!(arr.len(), 1, "expected 1 duplicate record, got: {out}");
    assert_eq!(arr[0]["crate_name"], "serde");
    let versions = arr[0]["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 2);
}

#[test]
fn deps_duplicates_clean_lockfile() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["solo"]);
    write_basic_lib_crate(root, "solo", "pub fn x() {}\n", "");
    let lock = r#"
version = 3
[[package]]
name = "serde"
version = "1.0.219"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;
    std::fs::write(root.join("Cargo.lock"), lock).unwrap();
    let (out, _err, code) = run_wraith(root, &["deps", "duplicates", "--format", "json"]);
    assert_eq!(code, 0, "expected exit 0 on no duplicates, stdout: {out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert!(v.as_array().unwrap().is_empty());
}

#[test]
fn deps_audit_missing_binary_exits_64() {
    // We can't reliably test the success-path without cargo-audit installed.
    // The robust thing is to assert the install-hint exit code when the
    // PATH is scrubbed.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["aw"]);
    write_basic_lib_crate(root, "aw", "pub fn x() {}\n", "");

    let out = std::process::Command::new(wraith_bin())
        .arg("--root")
        .arg(root)
        .arg("deps")
        .arg("audit")
        .env("PATH", "/nonexistent-path-for-test")
        .output()
        .expect("ran wraith");
    let code = out.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(code, 64, "expected exit 64 when cargo-audit missing, stderr: {stderr}");
    assert!(
        stderr.contains("cargo-audit"),
        "expected installation hint, got: {stderr}"
    );
}

#[test]
fn deps_unused_features_flags_uncalled_feature() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["uf"]);
    // Crate declares serde with a feature but never imports serde at all.
    let toml = "[package]\nname = \"uf\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n\n[dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\n";
    write(root, "uf/Cargo.toml", toml);
    write(root, "uf/src/lib.rs", "pub fn unrelated() -> i32 { 42 }\n");

    let (out, _err, code) = run_wraith(root, &["deps", "unused-features", "--format", "json"]);
    assert_eq!(code, 1, "expected exit 1 on flagged feature, stdout: {out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    let arr = v.as_array().expect("array");
    let hit = arr
        .iter()
        .find(|r| r["dep_name"] == "serde" && r["feature"] == "derive");
    assert!(hit.is_some(), "expected serde/derive flagged, got: {out}");
}

#[test]
fn deps_unused_features_skips_used_dep() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["uf2"]);
    let toml = "[package]\nname = \"uf2\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n\n[dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\n";
    write(root, "uf2/Cargo.toml", toml);
    write(
        root,
        "uf2/src/lib.rs",
        "use serde::Serialize;\n#[derive(Serialize)]\npub struct S { pub x: i32 }\n",
    );

    let (out, _err, code) = run_wraith(root, &["deps", "unused-features", "--format", "json"]);
    assert_eq!(code, 0, "expected exit 0 when dep is used, stdout: {out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert!(v.as_array().unwrap().is_empty(), "expected no flags, got: {out}");
}

#[test]
fn deps_size_missing_binary_exits_64() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["sz"]);
    write_basic_lib_crate(root, "sz", "pub fn x() {}\n", "");

    let out = std::process::Command::new(wraith_bin())
        .arg("--root")
        .arg(root)
        .arg("deps")
        .arg("size")
        .env("PATH", "/nonexistent-path-for-test")
        .output()
        .expect("ran wraith");
    let code = out.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(code, 64);
    assert!(stderr.contains("cargo-bloat"));
}

// --- visibility tightening (wb-5lgj.35) ---------------------------------

fn write_visibility_workspace(root: &Path) {
    write_root_workspace(root, &["crate_a", "crate_b", "crate_c"]);
    // crate_a: helper() called only by an inline `mod inner` in the
    // same crate; never re-exported, never used across crates.
    write_basic_lib_crate(
        root,
        "crate_a",
        r#"
pub fn helper() -> i32 { 42 }

pub fn cross_crate_api() -> i32 { 7 }

mod inner {
    pub fn use_helper() -> i32 { crate::helper() }
}

fn _keep_inner_alive() -> i32 { inner::use_helper() }
"#,
        "",
    );
    // crate_b: imports cross_crate_api from crate_a. helper() is NOT
    // imported here.
    let b_toml = "[package]\nname = \"crate_b\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n\n[dependencies]\ncrate_a = { path = \"../crate_a\" }\n";
    write(root, "crate_b/Cargo.toml", b_toml);
    write(
        root,
        "crate_b/src/lib.rs",
        "pub fn drive() -> i32 { crate_a::cross_crate_api() }\n",
    );
    // crate_c: bin that pulls crate_b in (keeps drive + cross_crate_api alive).
    let c_toml = "[package]\nname = \"crate_c\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[[bin]]\nname = \"crate_c\"\npath = \"src/main.rs\"\n\n[dependencies]\ncrate_b = { path = \"../crate_b\" }\n";
    write(root, "crate_c/Cargo.toml", c_toml);
    write(
        root,
        "crate_c/src/main.rs",
        "fn main() { println!(\"{}\", crate_b::drive()); }\n",
    );
}

#[test]
fn visibility_suggests_tightening_for_same_crate_helper() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_visibility_workspace(root);

    let (out, err, code) = run_wraith(root, &["--format", "json", "visibility"]);
    assert_eq!(code, 0, "exit {code} stderr: {err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    let suggestions = v
        .get("suggestions")
        .and_then(|x| x.as_array())
        .expect("suggestions array");

    // helper should be suggested for tightening.
    let helper_sug = suggestions
        .iter()
        .find(|s| {
            s.get("symbol")
                .and_then(|x| x.as_str())
                .map(|n| n.ends_with("::helper"))
                .unwrap_or(false)
        })
        .expect("expected a suggestion for helper");
    let suggested = helper_sug
        .get("suggested")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    assert!(
        suggested == "pub(crate)" || suggested == "private",
        "expected pub(crate) or private, got {suggested}: {out}"
    );

    // cross_crate_api MUST NOT be tightened (it's used in crate_b).
    let api_sug = suggestions.iter().find(|s| {
        s.get("symbol")
            .and_then(|x| x.as_str())
            .map(|n| n.ends_with("::cross_crate_api"))
            .unwrap_or(false)
    });
    assert!(
        api_sug.is_none(),
        "cross_crate_api should be kept pub, found suggestion: {api_sug:?}\n{out}"
    );
}

#[test]
fn visibility_apply_rewrites_in_place() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_visibility_workspace(root);

    let (out, err, code) = run_wraith(root, &["visibility", "--apply"]);
    assert_eq!(code, 0, "exit {code} stderr: {err}, stdout: {out}");
    assert!(
        out.contains("applied"),
        "expected applied count in stdout: {out}"
    );

    let lib = std::fs::read_to_string(root.join("crate_a/src/lib.rs")).unwrap();
    assert!(
        lib.contains("pub(crate) fn helper") || lib.contains("fn helper"),
        "helper should be tightened, got:\n{lib}"
    );
    assert!(
        lib.contains("pub fn cross_crate_api"),
        "cross_crate_api should remain pub, got:\n{lib}"
    );
}

#[test]
fn visibility_skips_reexported_public_api() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["reexp"]);
    write_basic_lib_crate(
        root,
        "reexp",
        r#"
mod internal {
    pub fn intentional_api() -> i32 { 1 }
}
pub use internal::intentional_api;
fn _consumer() -> i32 { intentional_api() }
"#,
        "",
    );
    let (out, err, code) = run_wraith(root, &["--format", "json", "visibility"]);
    assert_eq!(code, 0, "stderr: {err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    let suggestions = v
        .get("suggestions")
        .and_then(|x| x.as_array())
        .expect("suggestions array");
    let bad = suggestions.iter().find(|s| {
        s.get("symbol")
            .and_then(|x| x.as_str())
            .map(|n| n.ends_with("::intentional_api"))
            .unwrap_or(false)
    });
    assert!(
        bad.is_none(),
        "should not suggest tightening for `pub use`-reexported items: {bad:?}"
    );
}

fn extract_shared_rolls_back_on_bad_signature_build_fail() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_pick_ext_cluster(root);

    let before = std::fs::read_to_string(root.join("picker/src/lib.rs")).unwrap();

    // Inject a sig that uses an unknown type → cargo check will fail.
    let mapping = r#"{
      "for_jpg":  {"needle": "\"jpeg\"", "ext": "\"jpg\""},
      "for_webp": {"needle": "\"webp\"", "ext": "\"webp\""},
      "for_gif":  {"needle": "\"gif\"",  "ext": "\"gif\""}
    }"#;
    let (_out, err, code) = run_wraith(
        root,
        &[
            "refactor",
            "extract-shared",
            "0",
            "--signature",
            "fn pick_ext(needle: &NoSuchType, ext: &str) -> &'static str",
            "--param-mapping",
            mapping,
            "--extract-to",
            "crate::util",
        ],
    );
    assert_eq!(code, 65, "expected exit 65, got {code}, err: {err}");
    let after = std::fs::read_to_string(root.join("picker/src/lib.rs")).unwrap();
    assert_eq!(before, after, "rollback must restore lib.rs");
    assert!(
        !root.join("picker/src/util.rs").exists(),
        "rollback must remove util.rs"
    );
}

// ============================================================================
// Incremental analysis cache (wb-5lgj.36)
// ============================================================================

fn run_wraith_env(root: &Path, args: &[&str], env: &[(&str, &str)]) -> (String, String, i32) {
    let mut cmd = Command::new(wraith_bin());
    cmd.arg("--root").arg(root).args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("failed to run wraith");
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    )
}

fn parse_cache_stats(stderr: &str) -> Option<(usize, usize)> {
    for line in stderr.lines() {
        // Expected: "wraith: cache hits=N misses=M entries=K"
        let Some(rest) = line.strip_prefix("wraith: cache ") else {
            continue;
        };
        let mut hits: Option<usize> = None;
        let mut misses: Option<usize> = None;
        for piece in rest.split_whitespace() {
            if let Some(v) = piece.strip_prefix("hits=") {
                hits = v.parse().ok();
            } else if let Some(v) = piece.strip_prefix("misses=") {
                misses = v.parse().ok();
            }
        }
        if let (Some(h), Some(m)) = (hits, misses) {
            return Some((h, m));
        }
    }
    None
}

fn write_cache_fixture(root: &Path) {
    write_root_workspace(root, &["alpha", "beta"]);

    // Synth wide-ish crates so parse time dominates over process spawn
    // + cargo_metadata. ~80 fns × 25 modules each in two crates.
    let mut alpha = String::new();
    for m in 0..25 {
        alpha.push_str(&format!("pub mod m{m} {{\n"));
        for n in 0..80 {
            alpha.push_str(&format!(
                "    pub fn fn_{m}_{n}(x: i32) -> i32 {{ let y = x + {n}; let z = y * 2 - {m}; z + y - x }}\n"
            ));
            alpha.push_str(&format!(
                "    pub struct S_{m}_{n} {{ pub a: i32, pub b: i32, pub c: i32 }}\n"
            ));
        }
        alpha.push_str("}\n");
    }
    write_basic_lib_crate(root, "alpha", &alpha, "");

    let mut beta = String::new();
    for m in 0..25 {
        beta.push_str(&format!("pub mod n{m} {{\n"));
        for n in 0..80 {
            beta.push_str(&format!(
                "    pub fn fn_{m}_{n}(x: i32) -> i32 {{ x + {n} + {m} }}\n"
            ));
            beta.push_str(&format!(
                "    pub struct T_{m}_{n} {{ pub a: i32, pub b: i32 }}\n"
            ));
        }
        beta.push_str("}\n");
    }
    write_basic_lib_crate(root, "beta", &beta, "");
}

#[test]
fn cache_warm_run_is_faster_than_cold() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_cache_fixture(root);

    // Warmup: amortize cargo_metadata's first-run filesystem walk so the
    // measurement isolates the analyzer's parse step, not Cargo bookkeeping.
    // Delete the cache afterwards so the measured "cold" is a real miss.
    let (_o, _e, _c) = run_wraith(root, &["dead-code"]);
    let _ = std::fs::remove_file(root.join(".wraithrc.cache"));

    let cold_start = std::time::Instant::now();
    let (_o, _e, code) = run_wraith(root, &["dead-code"]);
    let cold = cold_start.elapsed();
    assert_eq!(code, 1, "expected dead-code findings");

    assert!(
        root.join(".wraithrc.cache").exists(),
        "cache file should exist after first run"
    );

    let warm_start = std::time::Instant::now();
    let (_o, _e, code2) = run_wraith(root, &["dead-code"]);
    let warm = warm_start.elapsed();
    assert_eq!(code2, 1, "warm run findings should match cold run");

    assert!(
        warm * 3 < cold,
        "warm run ({warm:?}) should be at least 3x faster than cold ({cold:?})"
    );
}

#[test]
fn cache_only_reparses_modified_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_cache_fixture(root);

    // Cold run primes the cache.
    let (_o, err, _c) = run_wraith_env(root, &["dead-code"], &[("WRAITH_CACHE_DEBUG", "1")]);
    let (cold_hits, cold_misses) = parse_cache_stats(&err)
        .expect("expected cache debug line on stderr");
    assert_eq!(cold_hits, 0, "first run: all misses");
    assert!(cold_misses >= 2, "expected ≥2 files parsed, got {cold_misses}");

    // Touch one file's mtime forward so the cache entry is stale.
    let target = root.join("alpha/src/lib.rs");
    let now = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    let _ = filetime_set(&target, now);

    let (_o, err2, _c) = run_wraith_env(root, &["dead-code"], &[("WRAITH_CACHE_DEBUG", "1")]);
    let (warm_hits, warm_misses) = parse_cache_stats(&err2)
        .expect("expected cache debug line on stderr");
    assert_eq!(
        warm_misses, 1,
        "exactly one file should re-parse after mtime bump, got misses={warm_misses}"
    );
    assert!(
        warm_hits >= 1,
        "expected ≥1 cache hit on warm run, got {warm_hits}"
    );
}

#[test]
fn cache_invalidates_on_schema_version_mismatch() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_cache_fixture(root);

    let (_o, _e, _c) = run_wraith_env(root, &["dead-code"], &[("WRAITH_CACHE_DEBUG", "1")]);
    let cache_path = root.join(".wraithrc.cache");
    assert!(cache_path.exists());

    // Corrupt the cache by writing a different schema_version prefix.
    // bincode serializes the struct field-by-field; the leading 4 bytes
    // of the file are the u32 schema_version (little-endian).
    let mut bytes = std::fs::read(&cache_path).unwrap();
    assert!(bytes.len() >= 4);
    bytes[0] = 99;
    bytes[1] = 99;
    bytes[2] = 99;
    bytes[3] = 99;
    std::fs::write(&cache_path, bytes).unwrap();

    let (_o, err, code) = run_wraith_env(root, &["dead-code"], &[("WRAITH_CACHE_DEBUG", "1")]);
    assert_eq!(code, 1, "fresh rebuild should still report findings");
    let (hits, misses) = parse_cache_stats(&err)
        .expect("expected cache debug line on stderr");
    assert_eq!(hits, 0, "schema mismatch → no hits, got {hits}");
    assert!(misses >= 2, "schema mismatch → full rebuild, got {misses}");
}

#[test]
fn cache_corrupt_file_does_not_crash() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_cache_fixture(root);

    // Plant garbage at the cache path BEFORE the first run.
    std::fs::write(root.join(".wraithrc.cache"), b"not a real bincode payload").unwrap();

    let (_o, _err, code) = run_wraith(root, &["dead-code"]);
    assert_eq!(code, 1, "wraith should ignore the corrupt cache and run normally");
    // And the cache should now be a valid rewrite.
    assert!(root.join(".wraithrc.cache").exists());
}

#[test]
fn init_appends_cache_to_gitignore() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // Pre-existing gitignore — init should append, not overwrite.
    std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
    let (_o, _e, code) = run_wraith(root, &["init"]);
    assert_eq!(code, 0);
    let body = std::fs::read_to_string(root.join(".gitignore")).unwrap();
    assert!(body.contains("target/"), "must preserve existing entries");
    assert!(
        body.lines().any(|l| l.trim() == ".wraithrc.cache"),
        ".wraithrc.cache must be in .gitignore, got: {body}"
    );

    // Running init twice must not duplicate the entry.
    let (_o, _e, _c) = run_wraith(root, &["init", "--force"]);
    let body2 = std::fs::read_to_string(root.join(".gitignore")).unwrap();
    let count = body2.lines().filter(|l| l.trim() == ".wraithrc.cache").count();
    assert_eq!(count, 1, "entry must be deduped, got body: {body2}");
}

/// Tiny mtime-setter so we don't pull in the `filetime` crate just
/// for one test. Uses libc::utimes via std::fs round-trip.
fn filetime_set(path: &Path, when: std::time::SystemTime) -> std::io::Result<()> {
    let secs = when
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Reach for `touch -t` on unix-likes; falls back to writing-through
    // the file on platforms where that isn't available.
    let touch_arg = chrono_like_touch_arg(secs);
    let out = std::process::Command::new("touch")
        .arg("-t")
        .arg(&touch_arg)
        .arg(path)
        .output();
    if let Ok(o) = out {
        if o.status.success() {
            return Ok(());
        }
    }
    // Last-resort: rewrite the file with the same contents — bumps
    // mtime to "now", which is strictly newer than the cached value.
    let buf = std::fs::read(path)?;
    std::fs::write(path, buf)?;
    Ok(())
}

fn chrono_like_touch_arg(secs: u64) -> String {
    // `touch -t [[CC]YY]MMDDhhmm[.ss]` — build a UTC-ish stamp from
    // epoch seconds without pulling chrono.
    // 60-year safe enough for tests; the spec asks for "newer than cached".
    let days_total = secs / 86_400;
    let secs_of_day = secs % 86_400;
    let hh = secs_of_day / 3_600;
    let mm = (secs_of_day % 3_600) / 60;
    let ss = secs_of_day % 60;

    // Naive: 1970-01-01 + days_total. Good enough — tests just need a
    // valid stamp newer than the original.
    let (year, month, day) = days_to_ymd(days_total as i64);
    format!(
        "{:04}{:02}{:02}{:02}{:02}.{:02}",
        year, month, day, hh, mm, ss
    )
}

fn days_to_ymd(days_since_epoch: i64) -> (i64, u32, u32) {
    // Howard Hinnant's date algorithm — public domain.
    let z = days_since_epoch + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

// --- wb-5lgj.31 part 1: move-fn ----------------------------------------

#[test]
fn move_fn_relocates_across_crates_with_use_rewrite() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["crate_a", "crate_b"]);
    write_basic_lib_crate(
        root,
        "crate_a",
        r#"
pub mod util {
    pub fn helper(x: i32) -> i32 { x + 1 }
}
pub fn caller() -> i32 { util::helper(1) }
"#,
        "",
    );
    write_basic_lib_crate(
        root,
        "crate_b",
        r#"
pub mod shared {
    pub fn unrelated() -> i32 { 0 }
}
"#,
        "",
    );

    let src_file = root.join("crate_a/src/lib.rs");
    let (out, err, code) = run_wraith(
        root,
        &[
            "refactor",
            "move-fn",
            &format!("{}:helper", src_file.display()),
            "--to",
            "crate_a::shared",
        ],
    );
    assert_eq!(code, 0, "expected 0; stdout: {out}; stderr: {err}");

    let a = std::fs::read_to_string(root.join("crate_a/src/lib.rs")).unwrap();
    assert!(!a.contains("pub fn helper"), "helper should be gone from lib.rs: {a}");
    let shared = std::fs::read_to_string(root.join("crate_a/src/shared.rs")).unwrap();
    assert!(shared.contains("fn helper"), "shared.rs missing helper: {shared}");
}

#[test]
fn move_fn_refuses_cross_crate_without_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["crate_a", "crate_b"]);
    write_basic_lib_crate(
        root,
        "crate_a",
        "pub fn helper(x: i32) -> i32 { x + 1 }\n",
        "",
    );
    write_basic_lib_crate(root, "crate_b", "pub fn other() {}\n", "");

    let src_file = root.join("crate_a/src/lib.rs");
    let (_out, err, code) = run_wraith(
        root,
        &[
            "refactor",
            "move-fn",
            &format!("{}:helper", src_file.display()),
            "--to",
            "crate_b::shared",
        ],
    );
    assert_eq!(code, 64, "expected exit 64; stderr: {err}");
    assert!(
        err.contains("cross-crate"),
        "expected cross-crate refusal: {err}"
    );
}

#[test]
fn move_fn_dry_run_leaves_files_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["mvdry"]);
    write_basic_lib_crate(
        root,
        "mvdry",
        "pub fn helper(x: i32) -> i32 { x + 1 }\n",
        "",
    );
    let before = std::fs::read_to_string(root.join("mvdry/src/lib.rs")).unwrap();
    let src_file = root.join("mvdry/src/lib.rs");
    let (out, _err, code) = run_wraith(
        root,
        &[
            "refactor",
            "move-fn",
            &format!("{}:helper", src_file.display()),
            "--to",
            "mvdry::shared",
            "--dry-run",
        ],
    );
    assert_eq!(code, 0);
    assert!(out.contains("dry-run"), "expected dry-run notice: {out}");
    let after = std::fs::read_to_string(root.join("mvdry/src/lib.rs")).unwrap();
    assert_eq!(before, after, "dry-run must not modify source");
    assert!(
        !root.join("mvdry/src/shared.rs").exists(),
        "dry-run must not create shared.rs"
    );
}

// --- wb-5lgj.43: full module-registration + import wiring -----------

#[test]
fn move_fn_same_crate_compiles_first_try() {
    // Acceptance criterion 2: 1-crate workspace, private fn moved
    // out — `cargo check` passes on first try, no manual fixup.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["mycrate"]);
    write_basic_lib_crate(
        root,
        "mycrate",
        r#"
fn helper(x: i32) -> i32 { x + 1 }
pub fn caller() -> i32 { helper(1) }
"#,
        "",
    );
    let src_file = root.join("mycrate/src/lib.rs");
    let (out, err, code) = run_wraith(
        root,
        &[
            "refactor",
            "move-fn",
            &format!("{}:helper", src_file.display()),
            "--to",
            "mycrate::utils::helpers",
        ],
    );
    assert_eq!(code, 0, "expected 0; stdout: {out}; stderr: {err}");

    // Intermediate mod registration: lib.rs has `pub mod utils;`,
    // src/utils/mod.rs exists with `pub mod helpers;`, src/utils/helpers.rs exists.
    let lib = std::fs::read_to_string(root.join("mycrate/src/lib.rs")).unwrap();
    assert!(lib.contains("pub mod utils;"), "lib.rs missing `pub mod utils;`: {lib}");
    let utils_mod = std::fs::read_to_string(root.join("mycrate/src/utils/mod.rs")).unwrap();
    assert!(
        utils_mod.contains("pub mod helpers;"),
        "utils/mod.rs missing `pub mod helpers;`: {utils_mod}"
    );
    let helpers = std::fs::read_to_string(root.join("mycrate/src/utils/helpers.rs")).unwrap();
    assert!(helpers.contains("fn helper"), "helpers.rs missing fn: {helpers}");

    // Sanity: cargo check passes on first try.
    let cargo_out = std::process::Command::new("cargo")
        .arg("check")
        .arg("--quiet")
        .current_dir(root)
        .output()
        .expect("cargo check");
    assert!(
        cargo_out.status.success(),
        "cargo check failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&cargo_out.stdout),
        String::from_utf8_lossy(&cargo_out.stderr),
    );
}

// wb-5lgj.45 — move-fn must inject `use crate::...` into EVERY caller
// file in the same lib crate, not just the first one wraith looks at.
// Repro hit on wavelet image_arg_to_url move (27 callers across multiple
// files; only one got the use line). Also verifies the prefix is
// `crate::` for lib-internal callers, not `<crate-name>::`.
#[test]
fn move_fn_injects_use_into_every_same_crate_caller() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["mycrate"]);

    // lib.rs defines helper + registers two sibling modules. foo.rs and
    // bar.rs each call helper unqualified (they relied on lib's old
    // location). After move, both must have `use crate::utils::helpers::helper;`.
    write_basic_lib_crate(
        root,
        "mycrate",
        r#"
pub mod foo;
pub mod bar;

pub fn helper(x: i32) -> i32 { x + 1 }
"#,
        "",
    );
    write(
        root,
        "mycrate/src/foo.rs",
        "pub fn use_helper_a() -> i32 { helper(1) }\n",
    );
    write(
        root,
        "mycrate/src/bar.rs",
        "pub fn use_helper_b() -> i32 { helper(2) }\n",
    );

    let src_file = root.join("mycrate/src/lib.rs");
    let (out, err, code) = run_wraith(
        root,
        &[
            "refactor",
            "move-fn",
            &format!("{}:helper", src_file.display()),
            "--to",
            "mycrate::utils::helpers",
        ],
    );
    assert_eq!(code, 0, "expected 0; stdout: {out}; stderr: {err}");

    // Both callers must have the use line, and it must use `crate::`
    // (NOT `mycrate::`) since they live inside the same lib crate.
    let foo = std::fs::read_to_string(root.join("mycrate/src/foo.rs")).unwrap();
    let bar = std::fs::read_to_string(root.join("mycrate/src/bar.rs")).unwrap();

    assert!(
        foo.contains("use crate::utils::helpers::helper;"),
        "foo.rs missing use crate::... line:\n{foo}"
    );
    assert!(
        bar.contains("use crate::utils::helpers::helper;"),
        "bar.rs missing use crate::... line:\n{bar}"
    );
    assert!(
        !foo.contains("use mycrate::"),
        "foo.rs should use crate::, not mycrate::\n{foo}"
    );
    assert!(
        !bar.contains("use mycrate::"),
        "bar.rs should use crate::, not mycrate::\n{bar}"
    );

    // Sanity: cargo check passes on first try.
    let cargo_out = std::process::Command::new("cargo")
        .arg("check")
        .arg("--quiet")
        .current_dir(root)
        .output()
        .expect("cargo check");
    assert!(
        cargo_out.status.success(),
        "cargo check failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&cargo_out.stdout),
        String::from_utf8_lossy(&cargo_out.stderr),
    );
}

// wb-5lgj.44 — moving a bin fn into the lib must refuse (with a
// concrete deps list) when the moved fn body calls sibling private
// fns defined in the same bin. Bin items can't be imported from lib;
// blindly moving would produce unresolved-name errors at compile time.
// Safe v2: refuse with a list so the caller extracts deps first.
#[test]
fn move_fn_refuses_bin_to_lib_with_private_sibling_deps() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["mycrate"]);

    // Bin crate that has a moveable fn AND a sibling private helper.
    write(root, "mycrate/Cargo.toml", &format!(
        "[package]\nname = \"mycrate\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n\n[[bin]]\nname = \"mybin\"\npath = \"src/bin/mybin.rs\"\n"
    ));
    write(root, "mycrate/src/lib.rs", "pub fn lib_marker() {}\n");
    write(
        root,
        "mycrate/src/bin/mybin.rs",
        // handle_thing calls parse_region (private sibling) and
        // emit_analysis (also private). Both should appear in the
        // refusal message.
        "fn parse_region(s: &str) -> usize { s.len() }\n\
         fn emit_analysis(n: usize) -> String { format!(\"n={n}\") }\n\
         pub fn handle_thing(s: &str) -> String {\n\
             let r = parse_region(s);\n\
             emit_analysis(r)\n\
         }\n\
         fn main() { println!(\"{}\", handle_thing(\"x\")); }\n",
    );

    let src_file = root.join("mycrate/src/bin/mybin.rs");
    let (out, err, code) = run_wraith(
        root,
        &[
            "refactor",
            "move-fn",
            &format!("{}:handle_thing", src_file.display()),
            "--to",
            "mycrate::handlers",
        ],
    );

    assert_ne!(code, 0, "expected refusal, got success; stdout: {out}");
    // The error must name both bin-private deps so the caller knows
    // exactly what to extract.
    assert!(
        err.contains("parse_region") && err.contains("emit_analysis"),
        "stderr should list both deps; got: {err}"
    );
    // And the error should mention the bin file + the offending fn.
    assert!(
        err.contains("handle_thing"),
        "stderr should name the fn being moved; got: {err}"
    );
}

#[test]
fn move_fn_injects_std_use_imports() {
    // The moved fn references Path + ExitCode; the new file must have
    // `use std::path::Path;` and `use std::process::ExitCode;` at the top.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["mycrate"]);
    write_basic_lib_crate(
        root,
        "mycrate",
        "use std::path::Path;\nuse std::process::ExitCode;\nfn helper(p: &Path) -> ExitCode { let _ = p; ExitCode::SUCCESS }\npub fn caller() -> ExitCode { helper(std::path::Path::new(\"\")) }\n",
        "",
    );
    let src_file = root.join("mycrate/src/lib.rs");
    let (_out, err, code) = run_wraith(
        root,
        &[
            "refactor",
            "move-fn",
            &format!("{}:helper", src_file.display()),
            "--to",
            "mycrate::handlers::helper_h",
        ],
    );
    assert_eq!(code, 0, "{err}");
    let new = std::fs::read_to_string(root.join("mycrate/src/handlers/helper_h.rs")).unwrap();
    assert!(new.contains("use std::path::Path;"), "missing Path use: {new}");
    assert!(new.contains("use std::process::ExitCode;"), "missing ExitCode use: {new}");
}

#[test]
fn move_fn_rewrites_crate_self_paths_in_body() {
    // When the moved body references `<dest_crate>::X`, the new file
    // should use `crate::X` instead.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["mycrate"]);
    write_basic_lib_crate(
        root,
        "mycrate",
        r#"
pub mod backends { pub fn util() -> i32 { 7 } }
fn helper() -> i32 { mycrate::backends::util() }
pub fn caller() -> i32 { helper() }
"#,
        "",
    );
    let src_file = root.join("mycrate/src/lib.rs");
    let (_out, err, code) = run_wraith(
        root,
        &[
            "refactor",
            "move-fn",
            &format!("{}:helper", src_file.display()),
            "--to",
            "mycrate::handlers::h",
        ],
    );
    assert_eq!(code, 0, "{err}");
    let new = std::fs::read_to_string(root.join("mycrate/src/handlers/h.rs")).unwrap();
    assert!(
        new.contains("crate::backends::util"),
        "expected crate::backends path rewrite: {new}"
    );
    assert!(
        !new.contains("mycrate::backends::util"),
        "old mycrate:: prefix should have been rewritten: {new}"
    );
}

#[test]
fn move_fn_bin_to_lib_injects_caller_use_and_lifts_pub() {
    // 2-target crate (lib + bin). Fn defined in `src/bin/foo.rs`,
    // moved to `mycrate::handlers::h`. Caller in the binary gets a
    // `use mycrate::handlers::h::fn_name;`. Visibility lifted to pub.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["mycrate"]);
    // Manual write since we need both lib + bin.
    let toml = "[package]\nname = \"mycrate\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n\n[[bin]]\nname = \"foo\"\npath = \"src/bin/foo.rs\"\n";
    write(root, "mycrate/Cargo.toml", toml);
    write(root, "mycrate/src/lib.rs", "// lib root\n");
    write(
        root,
        "mycrate/src/bin/foo.rs",
        "fn handle_x(x: i32) -> i32 { x + 1 }\nfn main() { let _ = handle_x(1); }\n",
    );
    let src_file = root.join("mycrate/src/bin/foo.rs");
    let (out, err, code) = run_wraith(
        root,
        &[
            "refactor",
            "move-fn",
            &format!("{}:handle_x", src_file.display()),
            "--to",
            "mycrate::handlers::h",
        ],
    );
    assert_eq!(code, 0, "{err}");
    let bin_after = std::fs::read_to_string(root.join("mycrate/src/bin/foo.rs")).unwrap();
    assert!(
        bin_after.contains("use mycrate::handlers::h::handle_x;"),
        "expected caller `use` injected: {bin_after}"
    );
    let dst = std::fs::read_to_string(root.join("mycrate/src/handlers/h.rs")).unwrap();
    assert!(dst.contains("pub fn handle_x"), "expected pub elevation: {dst}");
    assert!(
        out.contains("elevated") || dst.contains("pub fn handle_x"),
        "expected elevation notice or visible pub: stdout={out}"
    );
}

#[test]
fn move_fn_strict_missing_docs_scaffolds_doc_placeholder() {
    // Destination crate has `#![deny(missing_docs)]` — the moved fn
    // AND the `pub mod` declaration get scaffolded doc comments.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["mycrate"]);
    write_basic_lib_crate(
        root,
        "mycrate",
        "#![deny(missing_docs)]\n//! crate docs\nfn helper(x: i32) -> i32 { x + 1 }\n/// caller doc\npub fn caller() -> i32 { helper(1) }\n",
        "",
    );
    let src_file = root.join("mycrate/src/lib.rs");
    let (_out, err, code) = run_wraith(
        root,
        &[
            "refactor",
            "move-fn",
            &format!("{}:helper", src_file.display()),
            "--to",
            "mycrate::utils::helpers",
        ],
    );
    assert_eq!(code, 0, "{err}");
    let lib_after = std::fs::read_to_string(root.join("mycrate/src/lib.rs")).unwrap();
    assert!(
        lib_after.contains("/// (auto-generated placeholder)\npub mod utils;"),
        "expected doc-scaffolded mod decl: {lib_after}"
    );
    let helpers = std::fs::read_to_string(root.join("mycrate/src/utils/helpers.rs")).unwrap();
    assert!(
        helpers.contains("/// (auto-generated placeholder)"),
        "expected doc-scaffolded fn: {helpers}"
    );
    // And it should compile.
    let cargo_out = std::process::Command::new("cargo")
        .arg("check")
        .arg("--quiet")
        .current_dir(root)
        .output()
        .expect("cargo check");
    assert!(
        cargo_out.status.success(),
        "cargo check failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&cargo_out.stdout),
        String::from_utf8_lossy(&cargo_out.stderr),
    );
}

// --- wb-5lgj.31 part 2: rename ----------------------------------------

#[test]
fn rename_workspace_wide_updates_definition_and_callers() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["rn"]);
    write_basic_lib_crate(
        root,
        "rn",
        r#"
pub fn foo() -> i32 { 1 }
pub fn caller_a() -> i32 { foo() + 1 }
pub fn caller_b() -> i32 { foo() + 2 }
pub fn caller_c() -> i32 { foo() + 3 }
"#,
        "",
    );
    let (out, err, code) = run_wraith(
        root,
        &["refactor", "rename", "rn::foo", "--to", "bar"],
    );
    assert_eq!(code, 0, "expected 0; stdout: {out}, stderr: {err}");

    let body = std::fs::read_to_string(root.join("rn/src/lib.rs")).unwrap();
    assert!(body.contains("pub fn bar()"), "missing renamed def: {body}");
    assert!(!body.contains("pub fn foo()"), "old foo def still present: {body}");
    let bar_calls = body.matches("bar()").count();
    assert!(bar_calls >= 4, "expected >=4 bar() occurrences, got {bar_calls}: {body}");
}

#[test]
fn rename_refuses_collision_with_existing_symbol() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["rc"]);
    write_basic_lib_crate(
        root,
        "rc",
        "pub fn foo() {}\npub fn bar() {}\n",
        "",
    );
    let (_out, err, code) = run_wraith(
        root,
        &["refactor", "rename", "foo", "--to", "bar"],
    );
    assert_eq!(code, 64, "expected 64; stderr: {err}");
    assert!(err.contains("collide"), "expected collision error: {err}");
}

#[test]
fn rename_dry_run_does_not_modify_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["rd"]);
    write_basic_lib_crate(
        root,
        "rd",
        "pub fn foo() -> i32 { 1 }\npub fn use_foo() -> i32 { foo() }\n",
        "",
    );
    let before = std::fs::read_to_string(root.join("rd/src/lib.rs")).unwrap();
    let (out, _err, code) = run_wraith(
        root,
        &["refactor", "rename", "foo", "--to", "bar", "--dry-run"],
    );
    assert_eq!(code, 0);
    assert!(out.contains("dry-run"), "expected dry-run notice: {out}");
    let after = std::fs::read_to_string(root.join("rd/src/lib.rs")).unwrap();
    assert_eq!(before, after);
}

// --- wb-5lgj.31 part 3: inline ----------------------------------------

#[test]
fn inline_replaces_call_sites_with_body() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["il"]);
    write_basic_lib_crate(
        root,
        "il",
        r#"
pub fn double(x: i32) -> i32 { x * 2 }
pub fn use_a() -> i32 { double(5) }
pub fn use_b() -> i32 { double(7) }
"#,
        "",
    );
    let lib_path = root.join("il/src/lib.rs");
    let (out, err, code) = run_wraith(
        root,
        &[
            "refactor",
            "inline",
            &format!("{}:double", lib_path.display()),
        ],
    );
    assert_eq!(code, 0, "expected 0; stdout: {out}, stderr: {err}");
    let body = std::fs::read_to_string(&lib_path).unwrap();
    assert!(
        !body.contains("pub fn double"),
        "double fn should be deleted: {body}"
    );
    assert!(
        body.contains("(5)") && body.contains("* 2"),
        "expected inlined body with substituted arg: {body}"
    );
}

#[test]
fn inline_refuses_generic_fn() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["ilg"]);
    write_basic_lib_crate(
        root,
        "ilg",
        r#"
pub fn id<T>(x: T) -> T { x }
pub fn use_id() -> i32 { id(5) }
"#,
        "",
    );
    let lib_path = root.join("ilg/src/lib.rs");
    let (_out, err, code) = run_wraith(
        root,
        &[
            "refactor",
            "inline",
            &format!("{}:id", lib_path.display()),
        ],
    );
    assert_eq!(code, 64, "expected 64; stderr: {err}");
    assert!(err.contains("generic"), "expected generic refusal: {err}");
}

#[test]
fn inline_dry_run_does_not_modify_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_root_workspace(root, &["ild"]);
    write_basic_lib_crate(
        root,
        "ild",
        r#"
pub fn double(x: i32) -> i32 { x * 2 }
pub fn use_a() -> i32 { double(5) }
"#,
        "",
    );
    let lib_path = root.join("ild/src/lib.rs");
    let before = std::fs::read_to_string(&lib_path).unwrap();
    let (out, _err, code) = run_wraith(
        root,
        &[
            "refactor",
            "inline",
            &format!("{}:double", lib_path.display()),
            "--dry-run",
        ],
    );
    assert_eq!(code, 0);
    assert!(out.contains("dry-run"), "expected dry-run notice: {out}");
    let after = std::fs::read_to_string(&lib_path).unwrap();
    assert_eq!(before, after);
}

// --- wb-5lgj.31 part 4: split-fn --------------------------------------

#[test]
fn split_fn_splits_at_statement_boundary() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let file = root.join("split_fixture.rs");
    let body = "fn driver() {\n    let a = 1;\n    let b = a + 1;\n    let c = b + 1;\n    let d = c + 1;\n    let e = d + 1;\n    println!(\"{e}\");\n    println!(\"done\");\n}\n";
    std::fs::write(&file, body).unwrap();

    let (out, err, code) = run_wraith_no_root(&[
        "refactor",
        "split-fn",
        &format!("{}:driver", file.display()),
        "--at-line",
        "6",
        "--names",
        "compute,emit",
    ]);
    assert_eq!(code, 0, "stdout: {out}; stderr: {err}");

    let written = std::fs::read_to_string(&file).unwrap();
    assert!(written.contains("fn compute("), "missing first fn: {written}");
    assert!(written.contains("fn emit("), "missing second fn: {written}");
    // Original fn body must invoke the chain.
    assert!(
        written.contains("compute(") && written.contains("emit("),
        "missing chained calls: {written}"
    );
}

#[test]
fn split_fn_refuses_when_line_is_not_at_statement_boundary() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let file = root.join("nope.rs");
    let body = "fn driver() {\n    let a = 1;\n    let b = a + 1;\n    let c = b + 1;\n}\n";
    std::fs::write(&file, body).unwrap();

    let (_out, err, code) = run_wraith_no_root(&[
        "refactor",
        "split-fn",
        &format!("{}:driver", file.display()),
        "--at-line",
        "99",
        "--names",
        "first,second",
    ]);
    assert_eq!(code, 64, "expected exit 64; stderr: {err}");
    assert!(
        err.contains("statement boundary") || err.contains("empty half"),
        "expected boundary refusal: {err}"
    );
}

#[test]
fn split_fn_dry_run_does_not_modify_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let file = root.join("dry.rs");
    let body = "fn driver() {\n    let a = 1;\n    let b = a + 1;\n    let c = b + 1;\n    let d = c + 1;\n    println!(\"{d}\");\n}\n";
    std::fs::write(&file, body).unwrap();
    let before = std::fs::read_to_string(&file).unwrap();
    let (out, _err, code) = run_wraith_no_root(&[
        "refactor",
        "split-fn",
        &format!("{}:driver", file.display()),
        "--at-line",
        "5",
        "--names",
        "first,second",
        "--dry-run",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("dry-run"), "expected dry-run notice: {out}");
    let after = std::fs::read_to_string(&file).unwrap();
    assert_eq!(before, after);
}
