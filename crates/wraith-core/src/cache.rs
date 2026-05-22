//! Incremental analysis cache.
//!
//! Persists per-file parse output (symbols + references) to
//! `.wraithrc.cache` between `wraith` invocations. On a warm run the
//! analyzer only re-parses files whose mtime is newer than the cached
//! entry; everything else replays from disk.
//!
//! ## Schema versioning
//!
//! `SCHEMA_VERSION` is the only field that gates cache compatibility.
//! Bump it whenever ANY of these change shape:
//! - `Symbol` / `SymbolNode` / `Reference` (graph.rs)
//! - The `CachedFile` envelope itself
//! - The parser's behaviour in a way that produces different
//!   `Symbol`/`Reference` outputs for the same source bytes
//!
//! On version mismatch the cache is silently dropped and rebuilt from
//! scratch — no error surfaced to the user.

use crate::graph::{Reference, SymbolNode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: u32 = 1;

const CACHE_FILENAME: &str = ".wraithrc.cache";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedFile {
    pub mtime_secs: u64,
    pub symbols: Vec<SymbolNode>,
    pub references: Vec<Reference>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cache {
    pub schema_version: u32,
    pub entries: HashMap<PathBuf, CachedFile>,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            entries: HashMap::new(),
        }
    }
}

impl Cache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load the cache from `<root>/.wraithrc.cache`. Returns an empty
    /// cache when the file is missing, unreadable, mis-versioned, or
    /// otherwise corrupt — never errors.
    pub fn load(root: &Path) -> Self {
        let path = root.join(CACHE_FILENAME);
        let Ok(bytes) = std::fs::read(&path) else {
            return Self::new();
        };
        match bincode::deserialize::<Cache>(&bytes) {
            Ok(c) if c.schema_version == SCHEMA_VERSION => c,
            _ => Self::new(),
        }
    }

    /// Persist the cache to `<root>/.wraithrc.cache`. Best-effort: a
    /// failed write logs nothing — cache misses are recoverable on the
    /// next run.
    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        let path = root.join(CACHE_FILENAME);
        let bytes =
            bincode::serialize(self).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, bytes)
    }

    pub fn get(&self, path: &Path) -> Option<&CachedFile> {
        self.entries.get(path)
    }

    pub fn insert(&mut self, path: PathBuf, entry: CachedFile) {
        self.entries.insert(path, entry);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Return file mtime as seconds since UNIX_EPOCH, or `None` if the
/// file is missing or its mtime is unreadable.
pub fn file_mtime_secs(path: &Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    mtime
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// Counter of cache hits + misses during a single `analyze_root` call.
/// Surfaced via stderr when `WRAITH_CACHE_DEBUG=1` is set, and exposed
/// to integration tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct CacheStats {
    pub hits: usize,
    pub misses: usize,
}
