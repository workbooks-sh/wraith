use cargo_metadata::MetadataCommand;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrateInfo {
    pub name: String,
    pub manifest_path: PathBuf,
    pub root_dir: PathBuf,
    pub src_paths: Vec<PathBuf>,
    pub deps: Vec<DepInfo>,
    pub is_lib: bool,
    pub is_bin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepInfo {
    pub name: String,
    pub rename: Option<String>,
    pub kind: DepKind,
    pub optional: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DepKind {
    Normal,
    Dev,
    Build,
}

#[derive(Debug)]
pub struct Workspace {
    pub root: PathBuf,
    pub crates: Vec<CrateInfo>,
}

impl Workspace {
    pub fn load(root: &Path) -> anyhow::Result<Self> {
        let manifest = root.join("Cargo.toml");
        let mut cmd = MetadataCommand::new();
        if manifest.exists() {
            cmd.manifest_path(&manifest);
        } else {
            cmd.current_dir(root);
        }
        let meta = cmd.exec()?;
        let workspace_members: Vec<_> = meta.workspace_members.iter().collect();

        let mut crates = Vec::new();
        for pkg in meta.packages.iter() {
            if !workspace_members.contains(&&pkg.id) {
                continue;
            }
            let manifest_path: PathBuf = pkg.manifest_path.clone().into();
            let root_dir = manifest_path.parent().unwrap_or(Path::new(".")).to_path_buf();

            let mut is_lib = false;
            let mut is_bin = false;
            let mut src_paths = Vec::new();
            for tgt in &pkg.targets {
                let kinds: Vec<String> = tgt.kind.iter().map(|k| k.to_string()).collect();
                if kinds.iter().any(|k| k == "lib" || k == "rlib" || k == "proc-macro" || k == "cdylib") {
                    is_lib = true;
                }
                if kinds.iter().any(|k| k == "bin") {
                    is_bin = true;
                }
                let p: PathBuf = tgt.src_path.clone().into();
                if !src_paths.contains(&p) {
                    src_paths.push(p);
                }
            }

            let deps = pkg
                .dependencies
                .iter()
                .map(|d| DepInfo {
                    name: d.name.clone(),
                    rename: d.rename.clone(),
                    kind: match d.kind {
                        cargo_metadata::DependencyKind::Development => DepKind::Dev,
                        cargo_metadata::DependencyKind::Build => DepKind::Build,
                        _ => DepKind::Normal,
                    },
                    optional: d.optional,
                })
                .collect();

            crates.push(CrateInfo {
                name: pkg.name.clone(),
                manifest_path,
                root_dir,
                src_paths,
                deps,
                is_lib,
                is_bin,
            });
        }

        Ok(Self {
            root: meta.workspace_root.into(),
            crates,
        })
    }

    /// List all `.rs` files belonging to a crate (recursively under its src
    /// root + any sibling module dirs).
    pub fn crate_rs_files(&self, c: &CrateInfo) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut roots: Vec<PathBuf> = Vec::new();
        for src in &c.src_paths {
            if let Some(parent) = src.parent() {
                if !roots.contains(&parent.to_path_buf()) {
                    roots.push(parent.to_path_buf());
                }
            }
        }
        for root in roots {
            for entry in walkdir::WalkDir::new(&root)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let p = entry.path();
                if p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("rs") {
                    files.push(p.to_path_buf());
                }
            }
        }
        files.sort();
        files.dedup();
        files
    }
}
