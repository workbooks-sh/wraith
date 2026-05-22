//! Prettier plugin.
//!
//! Detects Prettier projects and marks config files as always used.
//! Parses prettier config to extract plugins as referenced dependencies.

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["prettier"];

const CONFIG_PATTERNS: &[&str] = &[
    ".prettierrc",
    ".prettierrc.{json,json5,js,cjs,mjs,ts,cts}",
    "prettier.config.{js,cjs,mjs,ts,cts}",
];

const ALWAYS_USED: &[&str] = &[
    ".prettierrc",
    ".prettierrc.{json,json5,yml,yaml,js,cjs,mjs,ts,cts,toml}",
    "prettier.config.{js,cjs,mjs,ts,cts}",
    ".prettierignore",
];

const TOOLING_DEPENDENCIES: &[&str] = &["prettier"];

define_plugin! {
    struct PrettierPlugin => "prettier",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    package_json_config_key: "prettier",
    resolve_config(config_path, source, _root) {
        let mut result = PluginResult::default();

        // Handle JSON configs (.prettierrc, .prettierrc.json)
        let is_json = config_path.extension().is_some_and(|ext| ext == "json")
            || config_path
                .file_name()
                .is_some_and(|name| name == ".prettierrc");
        let (parse_source, parse_path_buf) = if is_json {
            (format!("({source})"), config_path.with_extension("js"))
        } else {
            (source.to_string(), config_path.to_path_buf())
        };
        let parse_path: &std::path::Path = &parse_path_buf;

        // Extract imports from JS/TS configs
        let imports = config_parser::extract_imports(&parse_source, parse_path);
        for imp in &imports {
            let dep = crate::resolve::extract_package_name(imp);
            result.referenced_dependencies.push(dep);
        }

        // plugins -> referenced dependencies
        // e.g. { "plugins": ["prettier-plugin-svelte", "prettier-plugin-tailwindcss"] }
        let plugins =
            config_parser::extract_config_shallow_strings(&parse_source, parse_path, "plugins");
        for plugin in &plugins {
            let dep = crate::resolve::extract_package_name(plugin);
            result.referenced_dependencies.push(dep);
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn resolve_config_json_plugins() {
        let source = r#"{"plugins": ["prettier-plugin-svelte", "prettier-plugin-tailwindcss"]}"#;
        let plugin = PrettierPlugin;
        let result = plugin.resolve_config(Path::new(".prettierrc"), source, Path::new("/project"));

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"prettier-plugin-svelte".to_string()));
        assert!(deps.contains(&"prettier-plugin-tailwindcss".to_string()));
    }

    #[test]
    fn resolve_config_js_plugins() {
        let source = r#"
            export default {
                plugins: ["prettier-plugin-svelte"]
            };
        "#;
        let plugin = PrettierPlugin;
        let result = plugin.resolve_config(
            Path::new("prettier.config.js"),
            source,
            Path::new("/project"),
        );

        assert!(
            result
                .referenced_dependencies
                .contains(&"prettier-plugin-svelte".to_string())
        );
    }

    #[test]
    fn resolve_config_empty() {
        let source = r#"{"singleQuote": true}"#;
        let plugin = PrettierPlugin;
        let result = plugin.resolve_config(Path::new(".prettierrc"), source, Path::new("/project"));

        assert!(result.referenced_dependencies.is_empty());
    }
}
