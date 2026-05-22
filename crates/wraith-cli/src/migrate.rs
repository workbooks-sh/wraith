//! Translate other tools' configs into `.wraithrc.json`.

use crate::MigrateFrom;
use anyhow::Result;
use std::path::Path;
use wraith_core::config::Config;

pub fn run(root: &Path, from: MigrateFrom) -> Result<()> {
    let mut cfg = Config::load(root).unwrap_or_default();

    match from {
        MigrateFrom::Clippy => migrate_clippy(root, &mut cfg)?,
        MigrateFrom::Deny => migrate_deny(root, &mut cfg)?,
    }

    cfg.write(root)?;
    println!("merged into .wraithrc.json");
    Ok(())
}

fn migrate_clippy(root: &Path, cfg: &mut Config) -> Result<()> {
    let p = root.join("clippy.toml");
    if !p.exists() {
        eprintln!("wraith: no clippy.toml found at {}", p.display());
        return Ok(());
    }
    let text = std::fs::read_to_string(&p)?;
    let doc: toml::Value = toml::from_str(&text)?;

    // cognitive-complexity-threshold → complexity.cognitive
    if let Some(v) = doc.get("cognitive-complexity-threshold").and_then(|v| v.as_integer()) {
        cfg.complexity.cognitive = v as u32;
    }
    // type-complexity-threshold has no exact map; skip.
    // disallowed-types / disallowed-methods could feed boundaries.deny —
    // but those are about types/method names, not paths. Skip for now.

    eprintln!("wraith: migrated clippy.toml → complexity thresholds");
    Ok(())
}

fn migrate_deny(root: &Path, cfg: &mut Config) -> Result<()> {
    let p = root.join("deny.toml");
    if !p.exists() {
        eprintln!("wraith: no deny.toml found at {}", p.display());
        return Ok(());
    }
    let text = std::fs::read_to_string(&p)?;
    let doc: toml::Value = toml::from_str(&text)?;

    // [bans].deny[*].name → allow_unused_deps inverse isn't right;
    // [bans].skip[*].name → allow_unused_deps (since they're known
    // duplicates/exceptions we'd otherwise flag).
    if let Some(skip) = doc
        .get("bans")
        .and_then(|b| b.get("skip"))
        .and_then(|s| s.as_array())
    {
        for entry in skip {
            if let Some(name) = entry.get("name").and_then(|v| v.as_str()) {
                if !cfg.allow_unused_deps.iter().any(|s| s == name) {
                    cfg.allow_unused_deps.push(name.to_string());
                }
            }
        }
    }

    eprintln!("wraith: migrated deny.toml → allow_unused_deps");
    Ok(())
}
