//! Angular framework plugin.
//!
//! Detects Angular projects and marks component, module, service, guard,
//! pipe, directive, resolver, and interceptor files as entry points.
//! Parses `angular.json` to extract styles, scripts, main, and polyfills
//! from build targets as additional entry points.

use std::path::Path;

use super::config_parser;
use super::{Plugin, PluginResult};

define_plugin!(
    struct AngularPlugin => "angular",
    enablers: &["@angular/core"],
    entry_patterns: &[
        // Standard Angular CLI layout
        "src/main.ts",
        "src/app/**/*.component.ts",
        "src/app/**/*.module.ts",
        "src/app/**/*.service.ts",
        "src/app/**/*.guard.ts",
        "src/app/**/*.pipe.ts",
        "src/app/**/*.directive.ts",
        "src/app/**/*.resolver.ts",
        "src/app/**/*.interceptor.ts",
        // Nx monorepo layout (apps and libs under arbitrary paths)
        "**/src/main.ts",
        "**/src/app/**/*.component.ts",
        "**/src/app/**/*.module.ts",
        "**/src/app/**/*.service.ts",
        "**/src/app/**/*.guard.ts",
        "**/src/app/**/*.pipe.ts",
        "**/src/app/**/*.directive.ts",
        "**/src/app/**/*.resolver.ts",
        "**/src/app/**/*.interceptor.ts",
    ],
    config_patterns: &["angular.json", ".angular.json"],
    always_used: &[
        "angular.json",
        ".angular.json",
        "src/polyfills.ts",
        "src/environments/**/*.ts",
        // Angular 17+ standalone app bootstrap config (runtime, not tool config)
        "src/app/app.config.ts",
        "src/app/app.config.server.ts",
    ],
    tooling_dependencies: &[
        "@angular/cli",
        "@angular-devkit/build-angular",
        "@angular/compiler-cli",
        "@angular/compiler",
        "@angular/build",
        "zone.js",
        "tslib",
        // Peer dependencies of @angular/core that may not be directly imported
        // but are required by the Angular framework at runtime
        "rxjs",
        "@angular/common",
        "@angular/platform-browser",
        "@angular/platform-browser-dynamic",
    ],
    resolve_config(config_path, source, _root) {
        let mut result = PluginResult::default();

        // angular.json: projects.*.architect.build.options.styles -> entry patterns
        // These are CSS/SCSS files loaded by the Angular CLI build system.
        let styles = config_parser::extract_config_object_nested_string_or_array(
            source,
            config_path,
            &["projects"],
            &["architect", "build", "options", "styles"],
        );
        for style in &styles {
            let path = style.trim_start_matches("./");
            result.push_entry_pattern(path.to_string());
        }

        // angular.json: projects.*.architect.build.options.scripts -> entry patterns
        let scripts = config_parser::extract_config_object_nested_string_or_array(
            source,
            config_path,
            &["projects"],
            &["architect", "build", "options", "scripts"],
        );
        for script in &scripts {
            let path = script.trim_start_matches("./");
            result.push_entry_pattern(path.to_string());
        }

        // angular.json: projects.*.architect.build.options.main -> entry patterns
        // Also check "browser" -- newer Angular CLI uses "browser" instead of "main"
        for field in &["main", "browser"] {
            let mains = config_parser::extract_config_object_nested_strings(
                source,
                config_path,
                &["projects"],
                &["architect", "build", "options", field],
            );
            for main in &mains {
                let path = main.trim_start_matches("./");
                result.push_entry_pattern(path.to_string());
            }
        }

        // angular.json: projects.*.architect.build.options.polyfills -> entry patterns
        // Can be a string or array
        let polyfills = config_parser::extract_config_object_nested_string_or_array(
            source,
            config_path,
            &["projects"],
            &["architect", "build", "options", "polyfills"],
        );
        for polyfill in &polyfills {
            let trimmed = polyfill.trim_start_matches("./");
            // Skip npm package references like "zone.js" -- only add file paths.
            // File paths contain "/" (directory separators) or start with "src/", etc.
            // Bare package names like "zone.js" have no "/" and shouldn't be entry points.
            if trimmed.contains('/') {
                result.push_entry_pattern(trimmed.to_string());
            }
        }

        // angular.json: projects.*.architect.test.options.main -> entry patterns
        let test_mains = config_parser::extract_config_object_nested_strings(
            source,
            config_path,
            &["projects"],
            &["architect", "test", "options", "main"],
        );
        for main in &test_mains {
            let path = main.trim_start_matches("./");
            result.push_entry_pattern(path.to_string());
        }

        // angular.json: projects.*.architect.build.options.stylePreprocessorOptions.includePaths
        // Angular CLI resolves bare SCSS imports (`@import 'variables'`) by
        // searching these directories. Without threading them into fallow's
        // SCSS resolver, the imports become false-positive unresolved imports.
        // Paths are resolved relative to the workspace/project root per the
        // Angular workspace configuration reference. See issue #103.
        let include_paths = config_parser::extract_config_object_nested_string_or_array(
            source,
            config_path,
            &["projects"],
            &[
                "architect",
                "build",
                "options",
                "stylePreprocessorOptions",
                "includePaths",
            ],
        );
        result
            .scss_include_paths
            .extend(resolve_scss_include_paths(&include_paths, _root));

        result
    },
);

/// Resolve each SCSS include path entry to an absolute directory.
///
/// Skips entries whose resolved directory does not exist on disk — a missing
/// include path cannot resolve anything and would only waste syscalls during
/// SCSS resolution.
fn resolve_scss_include_paths(entries: &[String], root: &Path) -> Vec<std::path::PathBuf> {
    entries
        .iter()
        .map(|entry| root.join(entry.trim_start_matches("./")))
        .filter(|path| path.is_dir())
        .collect()
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
    fn resolve_config_extracts_styles() {
        let source = r#"{
            "projects": {
                "my-app": {
                    "architect": {
                        "build": {
                            "options": {
                                "styles": ["src/styles.css", "src/theme.scss"]
                            }
                        }
                    }
                }
            }
        }"#;
        let plugin = AngularPlugin;
        let result =
            plugin.resolve_config(Path::new("angular.json"), source, Path::new("/project"));
        assert!(has_entry_pattern(&result, "src/styles.css"));
        assert!(has_entry_pattern(&result, "src/theme.scss"));
    }

    #[test]
    fn resolve_config_extracts_styles_object_form() {
        // Angular CLI schema: `styles` entries can be `{ input, bundleName, inject }`.
        // Used for vendor stylesheets that must opt out of auto-injection.
        // Previously silently dropped. See #126.
        let source = r#"{
            "projects": {
                "my-app": {
                    "architect": {
                        "build": {
                            "options": {
                                "styles": [
                                    "src/styles.scss",
                                    { "input": "src/theme.scss", "bundleName": "theme", "inject": false }
                                ]
                            }
                        }
                    }
                }
            }
        }"#;
        let plugin = AngularPlugin;
        let result =
            plugin.resolve_config(Path::new("angular.json"), source, Path::new("/project"));
        assert!(has_entry_pattern(&result, "src/styles.scss"));
        assert!(
            has_entry_pattern(&result, "src/theme.scss"),
            "object-form entry `input` must be extracted as entry pattern"
        );
    }

    #[test]
    fn resolve_config_extracts_main() {
        let source = r#"{
            "projects": {
                "my-app": {
                    "architect": {
                        "build": {
                            "options": {
                                "main": "src/main.ts"
                            }
                        }
                    }
                }
            }
        }"#;
        let plugin = AngularPlugin;
        let result =
            plugin.resolve_config(Path::new("angular.json"), source, Path::new("/project"));
        assert!(has_entry_pattern(&result, "src/main.ts"));
    }

    #[test]
    fn resolve_config_extracts_scripts() {
        let source = r#"{
            "projects": {
                "my-app": {
                    "architect": {
                        "build": {
                            "options": {
                                "scripts": ["node_modules/some-lib/dist/script.js"]
                            }
                        }
                    }
                }
            }
        }"#;
        let plugin = AngularPlugin;
        let result =
            plugin.resolve_config(Path::new("angular.json"), source, Path::new("/project"));
        assert!(has_entry_pattern(
            &result,
            "node_modules/some-lib/dist/script.js"
        ));
    }

    #[test]
    fn resolve_config_multiple_projects() {
        let source = r#"{
            "projects": {
                "app-one": {
                    "architect": {
                        "build": {
                            "options": {
                                "styles": ["apps/one/src/styles.css"],
                                "main": "apps/one/src/main.ts"
                            }
                        }
                    }
                },
                "app-two": {
                    "architect": {
                        "build": {
                            "options": {
                                "styles": ["apps/two/src/styles.css"],
                                "main": "apps/two/src/main.ts"
                            }
                        }
                    }
                }
            }
        }"#;
        let plugin = AngularPlugin;
        let result =
            plugin.resolve_config(Path::new("angular.json"), source, Path::new("/project"));
        assert!(has_entry_pattern(&result, "apps/one/src/styles.css"));
        assert!(has_entry_pattern(&result, "apps/two/src/styles.css"));
        assert!(has_entry_pattern(&result, "apps/one/src/main.ts"));
        assert!(has_entry_pattern(&result, "apps/two/src/main.ts"));
    }

    #[test]
    fn resolve_config_extracts_scss_include_paths() {
        // Issue #103: stylePreprocessorOptions.includePaths must be threaded
        // through to the SCSS resolver. On-disk existence is checked in the
        // plugin so the test creates the directory.
        let tmp = tempfile::tempdir().expect("create temp dir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src/styles")).unwrap();
        std::fs::create_dir_all(root.join("libs/shared/scss")).unwrap();

        let source = r#"{
            "projects": {
                "my-app": {
                    "architect": {
                        "build": {
                            "options": {
                                "stylePreprocessorOptions": {
                                    "includePaths": ["src/styles", "./libs/shared/scss"]
                                }
                            }
                        }
                    }
                }
            }
        }"#;
        let plugin = AngularPlugin;
        let result = plugin.resolve_config(Path::new("angular.json"), source, root);
        assert_eq!(result.scss_include_paths.len(), 2);
        assert!(result.scss_include_paths.contains(&root.join("src/styles")));
        assert!(
            result
                .scss_include_paths
                .contains(&root.join("libs/shared/scss"))
        );
    }

    #[test]
    fn resolve_config_scss_include_paths_skips_missing_dirs() {
        // Missing directories are filtered out so they don't trigger pointless
        // filesystem lookups during SCSS resolution.
        let tmp = tempfile::tempdir().expect("create temp dir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src/styles")).unwrap();

        let source = r#"{
            "projects": {
                "my-app": {
                    "architect": {
                        "build": {
                            "options": {
                                "stylePreprocessorOptions": {
                                    "includePaths": ["src/styles", "missing/dir"]
                                }
                            }
                        }
                    }
                }
            }
        }"#;
        let plugin = AngularPlugin;
        let result = plugin.resolve_config(Path::new("angular.json"), source, root);
        assert_eq!(result.scss_include_paths.len(), 1);
        assert_eq!(result.scss_include_paths[0], root.join("src/styles"));
    }

    #[test]
    fn resolve_config_polyfills_skips_packages() {
        let source = r#"{
            "projects": {
                "my-app": {
                    "architect": {
                        "build": {
                            "options": {
                                "polyfills": ["zone.js", "src/polyfills.ts"]
                            }
                        }
                    }
                }
            }
        }"#;
        let plugin = AngularPlugin;
        let result =
            plugin.resolve_config(Path::new("angular.json"), source, Path::new("/project"));
        // zone.js is a package, not a file — should be skipped
        assert!(!has_entry_pattern(&result, "zone.js"));
        // src/polyfills.ts is a file path — should be included
        assert!(has_entry_pattern(&result, "src/polyfills.ts"));
    }
}
