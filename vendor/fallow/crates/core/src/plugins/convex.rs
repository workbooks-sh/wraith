//! Convex backend platform plugin.
//!
//! Detects Convex projects and marks all files in the convex/ directory as
//! entry points, since Convex deploys every exported function.

use super::Plugin;

const ENABLERS: &[&str] = &["convex"];

const ENTRY_PATTERNS: &[&str] = &["convex/**/*.{ts,js}"];

const ALWAYS_USED: &[&str] = &[
    "convex/_generated/**/*",
    "convex/schema.{ts,js}",
    "convex/auth.config.{ts,js}",
    "convex/auth.{ts,js}",
    "convex/http.{ts,js}",
];

const TOOLING_DEPENDENCIES: &[&str] = &["convex"];

define_plugin! {
    struct ConvexPlugin => "convex",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_patterns_cover_convex_directory() {
        let plugin = ConvexPlugin;
        assert!(plugin.entry_patterns().contains(&"convex/**/*.{ts,js}"));
    }

    #[test]
    fn always_used_protects_generated_files() {
        let plugin = ConvexPlugin;
        assert!(plugin.always_used().contains(&"convex/_generated/**/*"));
    }

    #[test]
    fn always_used_protects_schema() {
        let plugin = ConvexPlugin;
        assert!(plugin.always_used().contains(&"convex/schema.{ts,js}"));
    }

    #[test]
    fn always_used_protects_auth_config() {
        let plugin = ConvexPlugin;
        let used = plugin.always_used();
        assert!(used.contains(&"convex/auth.config.{ts,js}"));
        assert!(used.contains(&"convex/auth.{ts,js}"));
    }

    #[test]
    fn always_used_protects_http_router() {
        let plugin = ConvexPlugin;
        assert!(plugin.always_used().contains(&"convex/http.{ts,js}"));
    }

    #[test]
    fn enabled_with_convex_dep() {
        let plugin = ConvexPlugin;
        let deps = vec!["convex".to_string()];
        assert!(plugin.is_enabled_with_deps(&deps, std::path::Path::new("/project")));
    }

    #[test]
    fn not_enabled_without_convex_dep() {
        let plugin = ConvexPlugin;
        let deps = vec!["firebase".to_string()];
        assert!(!plugin.is_enabled_with_deps(&deps, std::path::Path::new("/project")));
    }

    #[test]
    fn tooling_dependencies_include_convex() {
        let plugin = ConvexPlugin;
        assert!(plugin.tooling_dependencies().contains(&"convex"));
    }
}
