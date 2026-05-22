use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DuplicateConfig {
    pub min_tokens: usize,
    pub similarity_threshold: f32,
}

impl Default for DuplicateConfig {
    fn default() -> Self {
        Self {
            min_tokens: 40,
            similarity_threshold: 0.85,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ComplexityConfig {
    pub cyclomatic: u32,
    pub cognitive: u32,
}

impl Default for ComplexityConfig {
    fn default() -> Self {
        Self {
            cyclomatic: 15,
            cognitive: 25,
        }
    }
}

/// A single boundary rule. `from` is a glob-ish path (matched as a path
/// prefix on the crate root_dir relative to workspace root). `allow` is a
/// list of permitted import path prefixes (matched against the `root`
/// segment of recorded references). `deny` overrides allow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundaryRule {
    pub from: String,
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub ignore: Vec<String>,
    pub allow_dead: Vec<String>,
    pub allow_unused_deps: Vec<String>,
    pub treat_pub_crate_as_internal: bool,
    pub duplicates: DuplicateConfig,
    pub complexity: ComplexityConfig,
    pub boundaries: Vec<BoundaryRule>,
}

impl Config {
    pub fn load(root: &Path) -> anyhow::Result<Self> {
        let path = root.join(".wraithrc.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)?;
        let cfg = serde_json::from_str(&text)?;
        Ok(cfg)
    }

    pub fn default_for_workspace() -> Self {
        Self {
            ignore: vec!["target".into(), "node_modules".into(), ".git".into()],
            allow_dead: vec![],
            allow_unused_deps: vec![],
            treat_pub_crate_as_internal: true,
            duplicates: DuplicateConfig::default(),
            complexity: ComplexityConfig::default(),
            boundaries: vec![],
        }
    }

    pub fn write(&self, root: &Path) -> anyhow::Result<()> {
        let path = root.join(".wraithrc.json");
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(path, text)?;
        Ok(())
    }
}
