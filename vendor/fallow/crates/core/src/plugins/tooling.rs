//! General tooling dependency detection.
//!
//! Known dev dependencies that are tooling (used by CLI/config, not imported in
//! application code). These complement the per-plugin `tooling_dependencies()`
//! lists with dependencies that aren't tied to any single plugin.

/// Prefixes of package names that are always dev tooling.
const GENERAL_TOOLING_PREFIXES: &[&str] = &[
    "@types/",
    "husky",
    "lint-staged",
    "commitlint",
    "@commitlint",
    "stylelint",
    "@vitest/",
    "@jest/",
    "@tapjs/",
    "@testing-library/",
    "@playwright/",
    "@react-native-community/cli",
    "@react-native/",
    "secretlint",
    "@secretlint/",
    "oxlint",
    "@semantic-release/",
    "semantic-release",
    "@release-it/",
    "@lerna-lite/",
    "@changesets/",
    "@graphql-codegen/",
    "@biomejs/",
    "@electron-forge/",
    "@electron/",
    "@formatjs/",
];

/// Exact package names that are always dev tooling.
const GENERAL_TOOLING_EXACT: &[&str] = &[
    "typescript",
    "prettier",
    "turbo",
    "concurrently",
    "cross-env",
    "rimraf",
    "npm-run-all",
    "npm-run-all2",
    "nodemon",
    "ts-node",
    "tsx",
    "knip",
    "fallow",
    "jest",
    "vitest",
    "tap",
    "happy-dom",
    "jsdom",
    "vite",
    "sass",
    "sass-embedded",
    "webpack",
    "webpack-cli",
    "webpack-dev-server",
    "esbuild",
    "rollup",
    "swc",
    "@swc/core",
    "@swc/jest",
    "terser",
    "cssnano",
    "sharp",
    "release-it",
    "lerna",
    "dotenv-cli",
    "dotenv-flow",
    "oxfmt",
    "jscpd",
    "npm-check-updates",
    "markdownlint-cli",
    "npm-package-json-lint",
    "synp",
    "flow-bin",
    "i18next-parser",
    "i18next-conv",
    "webpack-bundle-analyzer",
    "vite-plugin-svgr",
    "vite-plugin-eslint",
    "@vitejs/plugin-vue",
    "@vitejs/plugin-react",
    "next-sitemap",
    "tsup",
    "unbuild",
    "typedoc",
    "nx",
    "@manypkg/cli",
    "vue-tsc",
    "@vue/tsconfig",
    "@tsconfig/node20",
    "@tsconfig/react-native",
    "@typescript/native-preview",
    "tw-animate-css",
    "@ianvs/prettier-plugin-sort-imports",
    "prettier-plugin-tailwindcss",
    "prettier-plugin-organize-imports",
    "@vitejs/plugin-react-swc",
    "@vitejs/plugin-legacy",
    "rolldown",
    "rolldown-vite",
    "oxc-transform",
    "playwright",
    "puppeteer",
    "madge",
    "patch-package",
    "electron",
    "electron-builder",
    "electron-vite",
];

/// Lazily-built set for O(1) exact-match lookups.
fn tooling_exact_set() -> &'static rustc_hash::FxHashSet<&'static str> {
    static SET: std::sync::OnceLock<rustc_hash::FxHashSet<&'static str>> =
        std::sync::OnceLock::new();
    SET.get_or_init(|| GENERAL_TOOLING_EXACT.iter().copied().collect())
}

/// Check whether a package is a known tooling/dev dependency by name.
///
/// This is the single source of truth for general tooling detection.
/// Per-plugin tooling dependencies are declared via `Plugin::tooling_dependencies()`
/// and aggregated separately in `AggregatedPluginResult`.
#[must_use]
pub fn is_known_tooling_dependency(name: &str) -> bool {
    GENERAL_TOOLING_PREFIXES.iter().any(|p| name.starts_with(p))
        || tooling_exact_set().contains(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Prefix matching ──────────────────────────────────────────

    #[test]
    fn types_prefix_matches_scoped() {
        assert!(is_known_tooling_dependency("@types/node"));
        assert!(is_known_tooling_dependency("@types/react"));
        assert!(is_known_tooling_dependency("@types/express"));
    }

    #[test]
    fn types_prefix_does_not_match_similar_names() {
        // "type-fest" should NOT match "@types/" prefix
        assert!(!is_known_tooling_dependency("type-fest"));
        assert!(!is_known_tooling_dependency("typesafe-actions"));
    }

    #[test]
    fn storybook_not_blanket_matched() {
        // @storybook/ and storybook prefixes removed — handled by StorybookPlugin config parsing
        assert!(!is_known_tooling_dependency("@storybook/react"));
        assert!(!is_known_tooling_dependency("@storybook/addon-essentials"));
        assert!(!is_known_tooling_dependency("storybook"));
    }

    #[test]
    fn testing_library_prefix_matches() {
        assert!(is_known_tooling_dependency("@testing-library/react"));
        assert!(is_known_tooling_dependency("@testing-library/jest-dom"));
    }

    #[test]
    fn babel_not_blanket_matched() {
        // @babel/ and babel- prefixes removed — handled by BabelPlugin config parsing
        assert!(!is_known_tooling_dependency("@babel/core"));
        assert!(!is_known_tooling_dependency("@babel/preset-env"));
        assert!(!is_known_tooling_dependency("babel-loader"));
        assert!(!is_known_tooling_dependency("babel-jest"));
    }

    #[test]
    fn vitest_prefix_matches() {
        assert!(is_known_tooling_dependency("@vitest/coverage-v8"));
        assert!(is_known_tooling_dependency("@vitest/ui"));
    }

    #[test]
    fn eslint_not_blanket_matched() {
        // eslint and @typescript-eslint prefixes removed — handled by EslintPlugin config parsing
        assert!(!is_known_tooling_dependency("eslint"));
        assert!(!is_known_tooling_dependency("eslint-plugin-react"));
        assert!(!is_known_tooling_dependency("eslint-config-next"));
        assert!(!is_known_tooling_dependency("@typescript-eslint/parser"));
    }

    #[test]
    fn biomejs_prefix_matches() {
        assert!(is_known_tooling_dependency("@biomejs/biome"));
    }

    // ── Exact matching ───────────────────────────────────────────

    #[test]
    fn exact_typescript_matches() {
        assert!(is_known_tooling_dependency("typescript"));
    }

    #[test]
    fn exact_prettier_matches() {
        assert!(is_known_tooling_dependency("prettier"));
    }

    #[test]
    fn exact_vitest_matches() {
        assert!(is_known_tooling_dependency("vitest"));
    }

    #[test]
    fn exact_jest_matches() {
        assert!(is_known_tooling_dependency("jest"));
    }

    #[test]
    fn exact_vite_matches() {
        assert!(is_known_tooling_dependency("vite"));
    }

    #[test]
    fn exact_esbuild_matches() {
        assert!(is_known_tooling_dependency("esbuild"));
    }

    #[test]
    fn exact_tsup_matches() {
        assert!(is_known_tooling_dependency("tsup"));
    }

    #[test]
    fn exact_turbo_matches() {
        assert!(is_known_tooling_dependency("turbo"));
    }

    // ── Non-tooling dependencies ─────────────────────────────────

    #[test]
    fn common_runtime_deps_not_tooling() {
        assert!(!is_known_tooling_dependency("react"));
        assert!(!is_known_tooling_dependency("react-dom"));
        assert!(!is_known_tooling_dependency("express"));
        assert!(!is_known_tooling_dependency("lodash"));
        assert!(!is_known_tooling_dependency("next"));
        assert!(!is_known_tooling_dependency("vue"));
        assert!(!is_known_tooling_dependency("axios"));
    }

    #[test]
    fn empty_string_not_tooling() {
        assert!(!is_known_tooling_dependency(""));
    }

    #[test]
    fn near_miss_not_tooling() {
        // These look similar to tooling but should NOT match
        assert!(!is_known_tooling_dependency("type-fest"));
        assert!(!is_known_tooling_dependency("typestyle"));
        assert!(!is_known_tooling_dependency("prettier-bytes")); // not the exact "prettier"
        // Note: "prettier-bytes" starts with "prettier" but only prefix matches
        // check the prefixes list — "prettier" is NOT in GENERAL_TOOLING_PREFIXES,
        // it's in GENERAL_TOOLING_EXACT. So "prettier-bytes" should not match.
    }

    #[test]
    fn sass_variants_are_tooling() {
        assert!(is_known_tooling_dependency("sass"));
        assert!(is_known_tooling_dependency("sass-embedded"));
    }

    #[test]
    fn prettier_plugins_are_tooling() {
        assert!(is_known_tooling_dependency(
            "@ianvs/prettier-plugin-sort-imports"
        ));
        assert!(is_known_tooling_dependency("prettier-plugin-tailwindcss"));
    }

    // ── Additional prefix matching ────────────────────────────────

    #[test]
    fn electron_forge_prefix_matches() {
        assert!(is_known_tooling_dependency("@electron-forge/cli"));
        assert!(is_known_tooling_dependency(
            "@electron-forge/maker-squirrel"
        ));
    }

    #[test]
    fn electron_prefix_matches() {
        assert!(is_known_tooling_dependency("@electron/rebuild"));
        assert!(is_known_tooling_dependency("@electron/notarize"));
    }

    #[test]
    fn formatjs_prefix_matches() {
        assert!(is_known_tooling_dependency("@formatjs/cli"));
        assert!(is_known_tooling_dependency("@formatjs/intl"));
    }

    #[test]
    fn rollup_not_blanket_matched() {
        // @rollup/ prefix removed — handled by RollupPlugin config parsing
        assert!(!is_known_tooling_dependency("@rollup/plugin-commonjs"));
        assert!(!is_known_tooling_dependency("@rollup/plugin-node-resolve"));
        assert!(!is_known_tooling_dependency("@rollup/plugin-typescript"));
    }

    #[test]
    fn semantic_release_prefix_matches() {
        assert!(is_known_tooling_dependency("@semantic-release/github"));
        assert!(is_known_tooling_dependency("@semantic-release/npm"));
        assert!(is_known_tooling_dependency("semantic-release"));
    }

    #[test]
    fn release_it_prefix_matches() {
        assert!(is_known_tooling_dependency(
            "@release-it/conventional-changelog"
        ));
    }

    #[test]
    fn lerna_lite_prefix_matches() {
        assert!(is_known_tooling_dependency("@lerna-lite/cli"));
        assert!(is_known_tooling_dependency("@lerna-lite/publish"));
    }

    #[test]
    fn changesets_prefix_matches() {
        assert!(is_known_tooling_dependency("@changesets/cli"));
        assert!(is_known_tooling_dependency("@changesets/changelog-github"));
    }

    #[test]
    fn graphql_codegen_prefix_matches() {
        assert!(is_known_tooling_dependency("@graphql-codegen/cli"));
        assert!(is_known_tooling_dependency(
            "@graphql-codegen/typescript-operations"
        ));
    }

    #[test]
    fn secretlint_prefix_matches() {
        assert!(is_known_tooling_dependency("secretlint"));
        assert!(is_known_tooling_dependency(
            "@secretlint/secretlint-rule-preset-recommend"
        ));
    }

    #[test]
    fn oxlint_prefix_matches() {
        assert!(is_known_tooling_dependency("oxlint"));
    }

    #[test]
    fn react_native_community_prefix_matches() {
        assert!(is_known_tooling_dependency("@react-native-community/cli"));
        assert!(is_known_tooling_dependency(
            "@react-native-community/cli-platform-android"
        ));
    }

    #[test]
    fn react_native_prefix_matches() {
        assert!(is_known_tooling_dependency("@react-native/metro-config"));
        assert!(is_known_tooling_dependency(
            "@react-native/typescript-config"
        ));
    }

    #[test]
    fn jest_prefix_matches() {
        assert!(is_known_tooling_dependency("@jest/globals"));
        assert!(is_known_tooling_dependency("@jest/types"));
    }

    #[test]
    fn playwright_prefix_matches() {
        assert!(is_known_tooling_dependency("@playwright/test"));
        assert!(is_known_tooling_dependency("playwright"));
    }

    #[test]
    fn tapjs_prefix_matches() {
        assert!(is_known_tooling_dependency("@tapjs/test"));
        assert!(is_known_tooling_dependency("@tapjs/snapshot"));
    }

    // ── Additional exact matching ─────────────────────────────────

    #[test]
    fn exact_tap_matches() {
        assert!(is_known_tooling_dependency("tap"));
    }

    #[test]
    fn exact_rolldown_matches() {
        assert!(is_known_tooling_dependency("rolldown"));
        assert!(is_known_tooling_dependency("rolldown-vite"));
    }

    #[test]
    fn exact_electron_matches() {
        assert!(is_known_tooling_dependency("electron"));
        assert!(is_known_tooling_dependency("electron-builder"));
        assert!(is_known_tooling_dependency("electron-vite"));
    }

    #[test]
    fn exact_sharp_matches() {
        assert!(is_known_tooling_dependency("sharp"));
    }

    #[test]
    fn exact_puppeteer_matches() {
        assert!(is_known_tooling_dependency("puppeteer"));
    }

    #[test]
    fn exact_madge_matches() {
        assert!(is_known_tooling_dependency("madge"));
    }

    #[test]
    fn exact_patch_package_matches() {
        assert!(is_known_tooling_dependency("patch-package"));
    }

    #[test]
    fn exact_nx_matches() {
        assert!(is_known_tooling_dependency("nx"));
    }

    #[test]
    fn exact_vue_tsc_matches() {
        assert!(is_known_tooling_dependency("vue-tsc"));
    }

    #[test]
    fn exact_tsconfig_packages_match() {
        assert!(is_known_tooling_dependency("@tsconfig/node20"));
        assert!(is_known_tooling_dependency("@tsconfig/react-native"));
        assert!(is_known_tooling_dependency("@vue/tsconfig"));
    }

    #[test]
    fn exact_vitejs_plugins_match() {
        assert!(is_known_tooling_dependency("@vitejs/plugin-vue"));
        assert!(is_known_tooling_dependency("@vitejs/plugin-react"));
        assert!(is_known_tooling_dependency("@vitejs/plugin-react-swc"));
        assert!(is_known_tooling_dependency("@vitejs/plugin-legacy"));
    }

    #[test]
    fn exact_oxc_transform_matches() {
        assert!(is_known_tooling_dependency("oxc-transform"));
    }

    #[test]
    fn exact_typescript_native_preview_matches() {
        assert!(is_known_tooling_dependency("@typescript/native-preview"));
    }

    #[test]
    fn exact_tw_animate_css_matches() {
        assert!(is_known_tooling_dependency("tw-animate-css"));
    }

    #[test]
    fn exact_manypkg_cli_matches() {
        assert!(is_known_tooling_dependency("@manypkg/cli"));
    }

    #[test]
    fn exact_swc_variants_match() {
        assert!(is_known_tooling_dependency("@swc/core"));
        assert!(is_known_tooling_dependency("@swc/jest"));
    }

    // ── Negative tests for near-misses ────────────────────────────

    #[test]
    fn runtime_deps_with_similar_names_not_tooling() {
        // These are NOT tooling — they don't match any prefix or exact entry
        assert!(!is_known_tooling_dependency("react-scripts"));
        assert!(!is_known_tooling_dependency("express-validator"));
        assert!(!is_known_tooling_dependency("sass-loader")); // "sass" is exact, not prefix
    }

    #[test]
    fn postcss_not_blanket_matched() {
        // postcss, autoprefixer, tailwindcss, @tailwindcss prefixes removed —
        // handled by PostCssPlugin and TailwindPlugin config parsing
        assert!(!is_known_tooling_dependency("postcss-modules"));
        assert!(!is_known_tooling_dependency("postcss-import"));
        assert!(!is_known_tooling_dependency("autoprefixer"));
        assert!(!is_known_tooling_dependency("tailwindcss"));
        assert!(!is_known_tooling_dependency("@tailwindcss/typography"));
    }

    #[test]
    fn tooling_exact_set_is_deterministic() {
        // Calling the lazy set multiple times returns the same result
        let set1 = tooling_exact_set();
        let set2 = tooling_exact_set();
        assert_eq!(set1.len(), set2.len());
        assert!(set1.contains("typescript"));
    }
}
