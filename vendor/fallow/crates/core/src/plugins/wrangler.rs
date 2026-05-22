//! Wrangler / Cloudflare Workers plugin.
//!
//! Detects Cloudflare Workers projects and marks worker entry points
//! and config files.

use super::Plugin;

const ENABLERS: &[&str] = &["wrangler"];

const ENTRY_PATTERNS: &[&str] = &[
    "src/index.{ts,js}",
    "src/worker.{ts,js}",
    "functions/**/*.{ts,js}",
];

const ALWAYS_USED: &[&str] = &["wrangler.toml", "wrangler.json", "wrangler.jsonc"];

const TOOLING_DEPENDENCIES: &[&str] = &["wrangler", "@cloudflare/workers-types"];

define_plugin! {
    struct WranglerPlugin => "wrangler",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
}
