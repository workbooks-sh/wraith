//! `wraith watch` — re-run analysis on file save.

use anyhow::Result;
use notify::{EventKind, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::time::Duration;
use wraith_core::analyze::{analyze_root, find_dead_code, find_unused_deps};
use wraith_core::audit::run_audit;
use wraith_core::config::Config;
use wraith_core::report::Finding;
use wraith_core::workspace::Workspace;

pub fn run(root: &Path, cfg: Config, target: String) -> Result<()> {
    eprintln!("wraith: watching {} (target={})", root.display(), target);

    // initial pass
    let findings = run_target(root, &cfg, &target)?;
    emit_jsonl(&findings);

    let (tx, rx) = channel();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })?;
    watcher.watch(root, RecursiveMode::Recursive)?;

    let debounce = Duration::from_millis(250);
    let mut pending = false;

    loop {
        match rx.recv_timeout(debounce) {
            Ok(Ok(event)) => {
                if relevant(&event) {
                    pending = true;
                }
            }
            Ok(Err(_)) => {}
            Err(RecvTimeoutError::Timeout) => {
                if pending {
                    pending = false;
                    match run_target(root, &cfg, &target) {
                        Ok(f) => emit_jsonl(&f),
                        Err(e) => eprintln!("wraith: watch error: {:#}", e),
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

fn relevant(e: &notify::Event) -> bool {
    if !matches!(e.kind, EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)) {
        return false;
    }
    e.paths.iter().any(|p| {
        let s = p.to_string_lossy();
        if s.contains("/target/") || s.contains("/.git/") {
            return false;
        }
        matches!(
            p.extension().and_then(|s| s.to_str()),
            Some("rs") | Some("toml")
        )
    })
}

fn run_target(root: &Path, cfg: &Config, target: &str) -> Result<Vec<Finding>> {
    match target {
        "audit" => {
            let ws = Workspace::load(root)?;
            run_audit(&ws, cfg)
        }
        "dead-code" => {
            let (_ws, g) = analyze_root(root, cfg)?;
            Ok(find_dead_code(&g, cfg))
        }
        "unused-deps" => {
            let (ws, g) = analyze_root(root, cfg)?;
            Ok(find_unused_deps(&ws, &g, cfg))
        }
        other => {
            anyhow::bail!("unknown watch target: {}", other)
        }
    }
}

fn emit_jsonl(findings: &[Finding]) {
    for f in findings {
        println!("{}", serde_json::to_string(f).unwrap());
    }
    // marker line so consumers know the batch ended
    println!("{{\"event\":\"batch-end\",\"count\":{}}}", findings.len());
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

// keep PathBuf in scope to dampen unused-imports warnings if api changes
#[allow(dead_code)]
fn _path_ref(_p: PathBuf) {}
