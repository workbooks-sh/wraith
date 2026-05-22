//! Nx monorepo plugin.
//!
//! Detects Nx projects and marks workspace config files as always used.
//! Parses `project.json` to extract executor references as tooling dependencies
//! and `options.main` as entry points.

#[cfg(test)]
use std::path::Path;

use super::config_parser;
use super::{Plugin, PluginResult};

define_plugin!(
    struct NxPlugin => "nx",
    enablers: &["nx"],
    config_patterns: &["**/project.json"],
    always_used: &["nx.json", "**/project.json"],
    tooling_dependencies: &[
        "nx",
        "@nx/workspace",
        "@nx/js",
        "@nx/react",
        "@nx/next",
        "@nx/node",
        "@nx/web",
        "@nx/vite",
        "@nx/jest",
        "@nx/eslint",
        "@nx/angular",
        "@nx/storybook",
        "@nx/webpack",
        "@nx/cypress",
        "@nx/playwright",
        "@nx/rollup",
        "@nx/esbuild",
        "@nx/rspack",
        "@nx/express",
        "@nx/nest",
    ],
    resolve_config(config_path, source, _root) {
        let mut result = PluginResult::default();

        // project.json: targets.*.executor → referenced dependency
        // Format: "@angular/build:application" or "@nx/vite:build"
        // Extract the package name before the ":" separator.
        let executor_strings = config_parser::extract_config_object_nested_strings(
            source,
            config_path,
            &["targets"],
            &["executor"],
        );
        for executor in &executor_strings {
            if let Some(pkg) = executor.split(':').next()
                && !pkg.is_empty()
            {
                result.referenced_dependencies.push(pkg.to_string());
            }
        }

        // Compute project root relative to workspace root for Nx token expansion.
        // `{projectRoot}` is the directory containing project.json relative to
        // the workspace root. `{workspaceRoot}` is the workspace root itself.
        // All path-valued fields below may use these tokens. See issue #114.
        let project_root_rel = config_path
            .parent()
            .and_then(|p| p.strip_prefix(_root).ok())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();

        // project.json: targets.*.options.{main,browser} → entry point.
        // The Angular CLI's newer `@angular/build:application` executor uses
        // `browser` instead of `main`; both forms appear in real Nx projects,
        // so extract either as an entry point. Mirrors `angular.rs`.
        for field in &["main", "browser"] {
            let mains = config_parser::extract_config_object_nested_strings(
                source,
                config_path,
                &["targets"],
                &["options", field],
            );
            for main in &mains {
                let expanded = expand_nx_tokens(main, &project_root_rel);
                let path = expanded.trim_start_matches("./");
                result.push_entry_pattern(path.to_string());
            }
        }

        // project.json: targets.*.options.{styles,scripts} → entry patterns.
        // Global stylesheets and scripts declared for the Angular build
        // executor are real entry points (bundled into the app output); they
        // must be marked reachable or their contents get false-flagged as
        // unused files / unused imports. Mirrors the angular.json handling
        // in `angular.rs`. See issue #125 (follow-up) — Nx projects using
        // `project.json` previously lost this coverage.
        for field in &["styles", "scripts"] {
            let entries = config_parser::extract_config_object_nested_string_or_array(
                source,
                config_path,
                &["targets"],
                &["options", field],
            );
            for entry in &entries {
                let expanded = expand_nx_tokens(entry, &project_root_rel);
                let path = expanded.trim_start_matches("./");
                result.push_entry_pattern(path.to_string());
            }
        }

        // project.json: targets.*.options.tsConfig → always used
        let tsconfigs = config_parser::extract_config_object_nested_strings(
            source,
            config_path,
            &["targets"],
            &["options", "tsConfig"],
        );
        for tsconfig in &tsconfigs {
            let expanded = expand_nx_tokens(tsconfig, &project_root_rel);
            let path = expanded.trim_start_matches("./");
            result.always_used_files.push(path.to_string());
        }

        // project.json: targets.*.options.stylePreprocessorOptions.includePaths
        // Angular executors invoked through Nx consume the same
        // stylePreprocessorOptions as the Angular CLI. Resolve paths relative
        // to the workspace root so bare SCSS `@import '...'` specifiers can
        // find shared partials. See issues #103, #114.
        let include_paths = config_parser::extract_config_object_nested_string_or_array(
            source,
            config_path,
            &["targets"],
            &["options", "stylePreprocessorOptions", "includePaths"],
        );
        for entry in &include_paths {
            let expanded = expand_nx_tokens(entry, &project_root_rel);
            let absolute = _root.join(expanded.trim_start_matches("./"));
            if absolute.is_dir() {
                result.scss_include_paths.push(absolute);
            }
        }

        result
    },
);

/// Expand Nx workspace tokens in a path string.
///
/// - `{projectRoot}` → the project's root directory relative to the workspace root
/// - `{workspaceRoot}` → empty string (paths are already resolved from workspace root)
///
/// See: <https://nx.dev/concepts/how-caching-works#runtime-hash-inputs>
fn expand_nx_tokens(path: &str, project_root_rel: &str) -> String {
    if !path.contains('{') {
        return path.to_string();
    }
    // Replace `{token}/rest` as a unit so that empty replacements don't leave
    // a leading `/` (e.g., `{projectRoot}/src` with empty root → `src`).
    let result = if project_root_rel.is_empty() {
        path.replace("{projectRoot}/", "")
            .replace("{projectRoot}", "")
    } else {
        path.replace("{projectRoot}", project_root_rel)
    };
    result
        .replace("{workspaceRoot}/", "")
        .replace("{workspaceRoot}", "")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_entry_pattern(result: &PluginResult, pattern: &str) -> bool {
        result
            .entry_patterns
            .iter()
            .any(|entry_pattern| entry_pattern.pattern == pattern)
    }

    #[test]
    fn resolve_config_extracts_executor() {
        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application"
                },
                "test": {
                    "executor": "@nx/vite:test"
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result =
            plugin.resolve_config(Path::new("project.json"), source, Path::new("/project"));
        assert!(
            result
                .referenced_dependencies
                .contains(&"@angular/build".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@nx/vite".to_string())
        );
    }

    #[test]
    fn resolve_config_extracts_main() {
        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "main": "apps/client/src/main.ts"
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result =
            plugin.resolve_config(Path::new("project.json"), source, Path::new("/project"));
        assert!(has_entry_pattern(&result, "apps/client/src/main.ts"));
    }

    #[test]
    fn resolve_config_extracts_browser_as_entry() {
        // @angular/build:application (new Angular 17+ builder used via Nx)
        // uses `browser` instead of `main`. The Nx plugin must treat both
        // as entry points so the referenced source file is reachable.
        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "browser": "apps/client/src/main.ts"
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result =
            plugin.resolve_config(Path::new("project.json"), source, Path::new("/project"));
        assert!(has_entry_pattern(&result, "apps/client/src/main.ts"));
    }

    #[test]
    fn resolve_config_extracts_styles_as_entry() {
        // project.json's `styles` array declares global stylesheets that the
        // Angular build executor bundles into the application. They are
        // reachable entry points; without this extraction they are reported
        // as unused files. See issue #125 follow-up.
        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "styles": [
                            "src/styles.scss",
                            "apps/client/src/theme.css"
                        ]
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result =
            plugin.resolve_config(Path::new("project.json"), source, Path::new("/project"));
        assert!(has_entry_pattern(&result, "src/styles.scss"));
        assert!(has_entry_pattern(&result, "apps/client/src/theme.css"));
    }

    #[test]
    fn resolve_config_extracts_styles_object_form() {
        // Nx project.json inherits the Angular CLI schema under
        // `@angular/build:application` — `styles` entries can be
        // `{ input, bundleName, inject }` object form for vendor stylesheets.
        // See #126.
        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "styles": [
                            "src/styles.scss",
                            { "input": "src/theme.scss", "bundleName": "theme", "inject": false }
                        ]
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result =
            plugin.resolve_config(Path::new("project.json"), source, Path::new("/project"));
        assert!(has_entry_pattern(&result, "src/styles.scss"));
        assert!(
            has_entry_pattern(&result, "src/theme.scss"),
            "object-form entry `input` must be extracted as entry pattern"
        );
    }

    #[test]
    fn resolve_config_extracts_scripts_as_entry() {
        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "scripts": ["src/analytics.ts"]
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result =
            plugin.resolve_config(Path::new("project.json"), source, Path::new("/project"));
        assert!(has_entry_pattern(&result, "src/analytics.ts"));
    }

    #[test]
    fn resolve_config_expands_project_root_in_styles() {
        // `{projectRoot}` tokens must be expanded in `styles` entries just
        // like they are in `main`, `browser`, and include paths.
        let source = r#"{
            "targets": {
                "build": {
                    "options": {
                        "styles": ["{projectRoot}/src/styles.scss"]
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result = plugin.resolve_config(
            Path::new("/repo/apps/client/project.json"),
            source,
            Path::new("/repo"),
        );
        assert!(has_entry_pattern(&result, "apps/client/src/styles.scss"));
    }

    #[test]
    fn resolve_config_extracts_tsconfig() {
        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "tsConfig": "apps/client/tsconfig.app.json"
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result =
            plugin.resolve_config(Path::new("project.json"), source, Path::new("/project"));
        assert!(
            result
                .always_used_files
                .contains(&"apps/client/tsconfig.app.json".to_string())
        );
    }

    #[test]
    fn resolve_config_extracts_scss_include_paths() {
        // Issue #103: Nx's project.json mirrors Angular's
        // stylePreprocessorOptions.includePaths when an Angular executor is used.
        let tmp = tempfile::tempdir().expect("create temp dir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join("libs/shared/scss")).unwrap();

        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "stylePreprocessorOptions": {
                            "includePaths": ["libs/shared/scss", "missing/dir"]
                        }
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result = plugin.resolve_config(Path::new("project.json"), source, root);
        assert_eq!(result.scss_include_paths.len(), 1);
        assert_eq!(result.scss_include_paths[0], root.join("libs/shared/scss"));
    }

    #[test]
    fn resolve_config_empty_targets() {
        let source = r#"{ "targets": {} }"#;
        let plugin = NxPlugin;
        let result =
            plugin.resolve_config(Path::new("project.json"), source, Path::new("/project"));
        assert!(result.referenced_dependencies.is_empty());
        assert!(result.entry_patterns.is_empty());
    }

    #[test]
    fn resolve_config_expands_project_root_in_main() {
        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "main": "{projectRoot}/src/main.ts"
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        // project.json at apps/myapp/, so {projectRoot} = "apps/myapp"
        let result = plugin.resolve_config(
            Path::new("/workspace/apps/myapp/project.json"),
            source,
            Path::new("/workspace"),
        );
        assert!(has_entry_pattern(&result, "apps/myapp/src/main.ts"));
    }

    #[test]
    fn resolve_config_expands_project_root_in_tsconfig() {
        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "tsConfig": "{projectRoot}/tsconfig.app.json"
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result = plugin.resolve_config(
            Path::new("/workspace/apps/myapp/project.json"),
            source,
            Path::new("/workspace"),
        );
        assert!(
            result
                .always_used_files
                .contains(&"apps/myapp/tsconfig.app.json".to_string())
        );
    }

    #[test]
    fn resolve_config_expands_project_root_token() {
        // Issue #114: {projectRoot} placeholder in includePaths must be expanded.
        let tmp = tempfile::tempdir().expect("create temp dir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src/style-paths")).unwrap();

        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "stylePreprocessorOptions": {
                            "includePaths": ["{projectRoot}/src/style-paths"]
                        }
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        // project.json is at the workspace root, so {projectRoot} = ""
        let result = plugin.resolve_config(root.join("project.json").as_path(), source, root);
        assert_eq!(result.scss_include_paths.len(), 1);
        assert_eq!(result.scss_include_paths[0], root.join("src/style-paths"));
    }

    #[test]
    fn resolve_config_expands_project_root_token_in_subproject() {
        // {projectRoot} for a project.json inside apps/myapp/ should expand
        // to "apps/myapp" relative to the workspace root.
        let tmp = tempfile::tempdir().expect("create temp dir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join("apps/myapp/src/styles")).unwrap();

        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "stylePreprocessorOptions": {
                            "includePaths": ["{projectRoot}/src/styles"]
                        }
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let config_path = root.join("apps/myapp/project.json");
        let result = plugin.resolve_config(config_path.as_path(), source, root);
        assert_eq!(result.scss_include_paths.len(), 1);
        assert_eq!(
            result.scss_include_paths[0],
            root.join("apps/myapp/src/styles")
        );
    }

    #[test]
    fn resolve_config_expands_workspace_root_token() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join("shared/styles")).unwrap();

        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "stylePreprocessorOptions": {
                            "includePaths": ["{workspaceRoot}/shared/styles"]
                        }
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result = plugin.resolve_config(root.join("project.json").as_path(), source, root);
        assert_eq!(result.scss_include_paths.len(), 1);
        assert_eq!(result.scss_include_paths[0], root.join("shared/styles"));
    }

    #[test]
    fn expand_nx_tokens_no_braces_unchanged() {
        assert_eq!(expand_nx_tokens("src/styles", "apps/myapp"), "src/styles");
    }

    #[test]
    fn expand_nx_tokens_project_root_replaced() {
        assert_eq!(
            expand_nx_tokens("{projectRoot}/src/styles", "apps/myapp"),
            "apps/myapp/src/styles"
        );
    }

    #[test]
    fn expand_nx_tokens_workspace_root_replaced() {
        assert_eq!(
            expand_nx_tokens("{workspaceRoot}/shared/styles", ""),
            "shared/styles"
        );
    }

    #[test]
    fn expand_nx_tokens_empty_project_root() {
        // Standalone app: project.json at workspace root, {projectRoot} = ""
        assert_eq!(
            expand_nx_tokens("{projectRoot}/src/styles", ""),
            "src/styles"
        );
    }
}
