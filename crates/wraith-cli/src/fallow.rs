//! Fallow integration — dispatches TS/JS analysis to a fallow binary if
//! one is available on PATH, then wraps its findings into wraith's
//! unified Finding schema.
//!
//! NOTE on integration shape: the original plan called for compiling
//! fallow's analysis crates in-tree as path deps so wraith ships a
//! single binary. Blocked today by:
//!   - fallow workspace uses resolver = "3" (Rust 2024 edition); local
//!     toolchain is 1.94 and the workspace can't compile that as a
//!     dependent.
//!   - fallow pins oxc_* at 0.126 with several uncoordinated workspace
//!     deps; folding it into wraith's `resolver = "2"` workspace would
//!     require version-aligning ~20 deps.
//!
//! When the host bumps to Rust 1.95+ stable and we adopt a workspace-
//! per-language layout, this shim should be replaced by direct calls
//! into `fallow-core` / `fallow-extract`. Tracked as a wb-5lgj follow-up.

use anyhow::Result;
use std::path::Path;
use std::process::Command;
use wraith_core::report::{Finding, Severity};

#[allow(dead_code)]
pub fn is_fallow_target(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()),
        Some("ts") | Some("tsx") | Some("js") | Some("jsx") | Some("mjs") | Some("cjs")
    )
}

/// Returns the path to a `fallow` binary if one is on PATH, or None.
pub fn fallow_available() -> bool {
    Command::new("fallow")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run `fallow <subcommand> --format=json` against `root` and wrap its
/// findings in wraith's unified Finding schema. Best-effort: if fallow
/// isn't installed or returns an unparseable payload we return empty.
pub fn run(root: &Path, subcommand: &str) -> Result<Vec<Finding>> {
    if !fallow_available() {
        return Ok(Vec::new());
    }
    let out = Command::new("fallow")
        .arg(subcommand)
        .arg("--format")
        .arg("json")
        .current_dir(root)
        .output()?;
    let text = String::from_utf8_lossy(&out.stdout);
    parse_fallow_json(&text)
}

fn parse_fallow_json(text: &str) -> Result<Vec<Finding>> {
    let val: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return Ok(Vec::new()),
    };
    let arr = match val.as_array() {
        Some(a) => a.clone(),
        None => val
            .get("findings")
            .and_then(|f| f.as_array())
            .cloned()
            .unwrap_or_default(),
    };
    let mut out = Vec::new();
    for entry in arr {
        let file = entry
            .get("file")
            .or_else(|| entry.get("path"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let line = entry.get("line").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let col = entry.get("col").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let category = entry
            .get("kind")
            .or_else(|| entry.get("category"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let message = entry
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let severity = match entry.get("severity").and_then(|v| v.as_str()) {
            Some("error") => Severity::Error,
            Some("info") => Severity::Info,
            _ => Severity::Warning,
        };
        out.push(Finding::external(
            std::path::PathBuf::from(file),
            line,
            col,
            "fallow",
            &category,
            &message,
            severity,
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatches_on_extension() {
        assert!(is_fallow_target(Path::new("foo.ts")));
        assert!(is_fallow_target(Path::new("a/b/c.tsx")));
        assert!(is_fallow_target(Path::new("x.mjs")));
        assert!(!is_fallow_target(Path::new("main.rs")));
        assert!(!is_fallow_target(Path::new("Cargo.toml")));
    }

    #[test]
    fn parses_fallow_findings_array() {
        let json = r#"[{"file":"a.ts","line":3,"col":5,"kind":"unused-export","message":"foo","severity":"warning"}]"#;
        let out = parse_fallow_json(json).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].line, 3);
    }

    #[test]
    fn empty_when_no_fallow_binary() {
        // can't reliably test without manipulating PATH; just verify the
        // happy path of parse with empty input.
        let out = parse_fallow_json("").unwrap();
        assert!(out.is_empty());
    }
}
