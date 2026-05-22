//! Hook installation + CI templates.

use crate::CiKind;
use anyhow::Result;
use std::path::Path;

const PRE_COMMIT: &str = r#"#!/usr/bin/env bash
# Installed by `wraith hooks install`.
set -e
exec wraith audit
"#;

pub fn install_git_hook(root: &Path) -> Result<()> {
    let hooks_dir = root.join(".git").join("hooks");
    if !hooks_dir.exists() {
        std::fs::create_dir_all(&hooks_dir)?;
    }
    let path = hooks_dir.join("pre-commit");
    std::fs::write(&path, PRE_COMMIT)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&path)?.permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&path, perm)?;
    }
    Ok(())
}

const CLAUDE_HOOK: &str = r#"{
  "name": "wraith-audit",
  "description": "Surface wraith findings to the agent after edits.",
  "events": ["PostToolUse"],
  "matchers": [{"tool_name": "Edit"}, {"tool_name": "Write"}],
  "command": "wraith audit --format json --exit-zero"
}
"#;

pub fn install_claude_hook(root: &Path) -> Result<()> {
    let dir = root.join(".claude").join("hooks");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("wraith-audit.json"), CLAUDE_HOOK)?;
    Ok(())
}

const GITHUB_CI: &str = r#"name: wraith

on:
  pull_request:
  push:
    branches: [main]

jobs:
  audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
      - name: Build wraith
        run: cargo install --path packages/wraith/crates/wraith-cli
      - name: Run wraith audit
        run: wraith audit
"#;

const GITLAB_CI: &str = r#"wraith:
  image: rust:latest
  stage: test
  script:
    - cargo install --path packages/wraith/crates/wraith-cli
    - wraith audit
  rules:
    - if: $CI_PIPELINE_SOURCE == "merge_request_event"
    - if: $CI_COMMIT_BRANCH == $CI_DEFAULT_BRANCH
"#;

pub fn write_ci_template(root: &Path, kind: CiKind) -> Result<()> {
    match kind {
        CiKind::Github => {
            let dir = root.join(".github").join("workflows");
            std::fs::create_dir_all(&dir)?;
            let p = dir.join("wraith.yml");
            std::fs::write(&p, GITHUB_CI)?;
            println!("wrote {}", p.display());
        }
        CiKind::Gitlab => {
            let p = root.join(".gitlab-ci.wraith.yml");
            std::fs::write(&p, GITLAB_CI)?;
            println!("wrote {} (include from .gitlab-ci.yml)", p.display());
        }
    }
    Ok(())
}
