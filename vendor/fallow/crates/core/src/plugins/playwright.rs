//! Playwright test runner plugin.
//!
//! Detects Playwright projects and marks test files and config as entry points.

#[cfg(test)]
use std::path::Path;

use super::config_parser;
use super::{Plugin, PluginResult};

define_plugin!(
    struct PlaywrightPlugin => "playwright",
    enablers: &["@playwright/test"],
    entry_patterns: &[
        "**/*.spec.{ts,tsx,js,jsx}",
        "**/*.test.{ts,tsx,js,jsx}",
        "tests/**/*.{ts,tsx,js,jsx}",
        "e2e/**/*.{ts,tsx,js,jsx}",
    ],
    config_patterns: &["playwright.config.{ts,js}"],
    always_used: &["playwright.config.{ts,js}"],
    tooling_dependencies: &["@playwright/test", "playwright"],
    fixture_glob_patterns: &[
        "**/fixtures/**/*.{ts,tsx,js,jsx,json}",
        "e2e/fixtures/**/*.{ts,tsx,js,jsx,json}",
    ],
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();

        let imports = config_parser::extract_imports(source, config_path);
        for imp in &imports {
            let dep = crate::resolve::extract_package_name(imp);
            result.referenced_dependencies.push(dep);
        }

        // globalSetup / globalTeardown -> setup files
        if let Some(setup) =
            config_parser::extract_config_string(source, config_path, &["globalSetup"])
        {
            result
                .setup_files
                .push(root.join(setup.trim_start_matches("./")));
        }
        if let Some(teardown) =
            config_parser::extract_config_string(source, config_path, &["globalTeardown"])
        {
            result
                .setup_files
                .push(root.join(teardown.trim_start_matches("./")));
        }

        result
    },
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_config_global_setup() {
        let source = r#"
            export default {
                globalSetup: "./global-setup.ts"
            };
        "#;
        let plugin = PlaywrightPlugin;
        let result = plugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(
            result.setup_files,
            vec![Path::new("/project/global-setup.ts")]
        );
    }

    #[test]
    fn resolve_config_global_teardown() {
        let source = r#"
            export default {
                globalTeardown: "./global-teardown.ts"
            };
        "#;
        let plugin = PlaywrightPlugin;
        let result = plugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(
            result.setup_files,
            vec![Path::new("/project/global-teardown.ts")]
        );
    }

    #[test]
    fn resolve_config_both_setup_and_teardown() {
        let source = r#"
            export default {
                globalSetup: "./setup.ts",
                globalTeardown: "./teardown.ts"
            };
        "#;
        let plugin = PlaywrightPlugin;
        let result = plugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(
            result.setup_files,
            vec![
                Path::new("/project/setup.ts"),
                Path::new("/project/teardown.ts"),
            ]
        );
    }

    #[test]
    fn resolve_config_imports() {
        let source = r#"
            import { defineConfig, devices } from '@playwright/test';
            export default defineConfig({
                globalSetup: "./setup.ts"
            });
        "#;
        let plugin = PlaywrightPlugin;
        let result = plugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@playwright/test".to_string())
        );
        assert_eq!(result.setup_files, vec![Path::new("/project/setup.ts")]);
    }

    #[test]
    fn resolve_config_empty() {
        let source = r"export default {};";
        let plugin = PlaywrightPlugin;
        let result = plugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        );
        assert!(result.setup_files.is_empty());
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn resolve_config_setup_strips_dot_slash() {
        let source = r#"
            export default {
                globalSetup: "./tests/global-setup.ts"
            };
        "#;
        let plugin = PlaywrightPlugin;
        let result = plugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(
            result.setup_files,
            vec![Path::new("/project/tests/global-setup.ts")]
        );
    }

    #[test]
    fn resolve_config_setup_without_dot_slash() {
        let source = r#"
            export default {
                globalSetup: "tests/global-setup.ts"
            };
        "#;
        let plugin = PlaywrightPlugin;
        let result = plugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(
            result.setup_files,
            vec![Path::new("/project/tests/global-setup.ts")]
        );
    }

    #[test]
    fn fixture_patterns_are_set() {
        let plugin = PlaywrightPlugin;
        assert!(!plugin.fixture_glob_patterns().is_empty());
    }
}
