//! JSX asset reference extraction tests.
//!
//! Mirrors the HTML parser's `<script src>` and `<link rel="stylesheet|modulepreload" href>`
//! extraction for JSX/TSX files. SSR frameworks (Hono, custom render-to-string
//! setups) use JSX to emit HTML, and bare asset references in those templates
//! must be tracked as `SideEffect` imports so the referenced files stay
//! reachable. See issue #105 (till's comment).

use fallow_types::extract::ImportedName;

use crate::tests::parse_tsx;

#[test]
fn jsx_script_src_string_literal_extracted() {
    let info = parse_tsx(
        r#"export const Layout = () => (
          <html>
            <head>
              <script src="./app.js"></script>
            </head>
          </html>
        );"#,
    );
    let imp = info
        .imports
        .iter()
        .find(|i| i.source == "./app.js")
        .expect("JSX <script src> should produce an ImportInfo");
    assert!(matches!(imp.imported_name, ImportedName::SideEffect));
    assert!(imp.local_name.is_empty());
    assert!(!imp.is_type_only);
}

#[test]
fn jsx_link_stylesheet_href_extracted() {
    let info = parse_tsx(
        r#"export const Layout = () => (
          <html>
            <head>
              <link rel="stylesheet" href="./global.css" />
            </head>
          </html>
        );"#,
    );
    let imp = info
        .imports
        .iter()
        .find(|i| i.source == "./global.css")
        .expect("JSX <link rel=stylesheet> should produce an ImportInfo");
    assert!(matches!(imp.imported_name, ImportedName::SideEffect));
}

#[test]
fn jsx_link_modulepreload_href_extracted() {
    let info = parse_tsx(
        r#"export const Layout = () => (
          <head>
            <link rel="modulepreload" href="./vendor.js" />
          </head>
        );"#,
    );
    assert!(info.imports.iter().any(|i| i.source == "./vendor.js"));
}

#[test]
fn jsx_link_reversed_attr_order_extracted() {
    let info = parse_tsx(
        r#"export const Layout = () => (
          <head>
            <link href="./style.css" rel="stylesheet" />
          </head>
        );"#,
    );
    assert!(info.imports.iter().any(|i| i.source == "./style.css"));
}

#[test]
fn jsx_root_relative_href_preserved() {
    // Root-relative paths stay unchanged — the resolver's web-root-relative
    // branch (extended to JSX/TSX sources) handles resolution.
    let info = parse_tsx(
        r#"export const Layout = () => (
          <head>
            <link rel="stylesheet" href="/static/style.css" />
          </head>
        );"#,
    );
    assert!(info.imports.iter().any(|i| i.source == "/static/style.css"));
}

#[test]
fn jsx_bare_script_src_normalized_to_relative() {
    let info = parse_tsx(
        r#"export const Layout = () => (
          <head>
            <script src="app.js"></script>
          </head>
        );"#,
    );
    // Bare filename is normalized to "./app.js" (matches HTML parser behavior).
    assert!(info.imports.iter().any(|i| i.source == "./app.js"));
}

#[test]
fn jsx_script_src_remote_http_skipped() {
    let info = parse_tsx(
        r#"export const Layout = () => (
          <head>
            <script src="https://cdn.example.com/lib.js"></script>
          </head>
        );"#,
    );
    assert!(
        !info
            .imports
            .iter()
            .any(|i| i.source.contains("cdn.example.com"))
    );
}

#[test]
fn jsx_script_src_protocol_relative_skipped() {
    let info = parse_tsx(
        r#"export const Layout = () => (
          <head>
            <script src="//cdn.example.com/lib.js"></script>
          </head>
        );"#,
    );
    assert!(
        !info
            .imports
            .iter()
            .any(|i| i.source.contains("cdn.example.com"))
    );
}

#[test]
fn jsx_link_icon_not_extracted() {
    // Only `rel="stylesheet"` and `rel="modulepreload"` are tracked — other
    // rel values (icon, preload, canonical) are skipped to match the HTML
    // parser whitelist.
    let info = parse_tsx(
        r#"export const Layout = () => (
          <head>
            <link rel="icon" href="./favicon.ico" />
          </head>
        );"#,
    );
    assert!(info.imports.iter().all(|i| i.source != "./favicon.ico"));
}

#[test]
fn jsx_link_preload_not_extracted() {
    let info = parse_tsx(
        r#"export const Layout = () => (
          <head>
            <link rel="preload" href="./font.woff2" as="font" />
          </head>
        );"#,
    );
    assert!(info.imports.iter().all(|i| i.source != "./font.woff2"));
}

#[test]
fn jsx_script_src_expression_container_skipped() {
    // Dynamic attribute values are not statically resolvable — skipped.
    let info = parse_tsx(
        r#"const src = "./dynamic.js";
        export const Layout = () => (
          <head>
            <script src={src}></script>
          </head>
        );"#,
    );
    assert!(info.imports.iter().all(|i| i.source != "./dynamic.js"));
}

#[test]
fn jsx_link_href_expression_container_skipped() {
    let info = parse_tsx(
        r#"const css = "./dynamic.css";
        export const Layout = () => (
          <head>
            <link rel="stylesheet" href={css} />
          </head>
        );"#,
    );
    assert!(info.imports.iter().all(|i| i.source != "./dynamic.css"));
}

#[test]
fn jsx_capitalized_component_not_extracted() {
    // Only lowercase intrinsic elements (<script>, <link>) are handled.
    // React-style <Script> and <Link> components have their own prop semantics
    // and are out of scope.
    let info = parse_tsx(
        r#"import { Script, Link } from 'some-lib';
        export const Layout = () => (
          <>
            <Script src="./should-not-be-tracked.js" />
            <Link rel="stylesheet" href="./should-not-be-tracked.css" />
          </>
        );"#,
    );
    assert!(
        info.imports
            .iter()
            .all(|i| i.source != "./should-not-be-tracked.js")
    );
    assert!(
        info.imports
            .iter()
            .all(|i| i.source != "./should-not-be-tracked.css")
    );
}

#[test]
fn jsx_multiple_assets_in_one_layout() {
    let info = parse_tsx(
        r#"export const Layout = () => (
          <html>
            <head>
              <link rel="stylesheet" href="/static/global.css" />
              <link rel="modulepreload" href="/static/vendor.js" />
              <script src="/static/app.js"></script>
            </head>
          </html>
        );"#,
    );
    let sources: Vec<&str> = info.imports.iter().map(|i| i.source.as_str()).collect();
    assert!(sources.contains(&"/static/global.css"));
    assert!(sources.contains(&"/static/vendor.js"));
    assert!(sources.contains(&"/static/app.js"));
}

#[test]
fn jsx_script_without_src_ignored() {
    // Inline scripts with no `src` attribute are not asset references.
    let info = parse_tsx(
        "export const Layout = () => (
          <head>
            <script>{`console.log('inline');`}</script>
          </head>
        );",
    );
    // The only imports should be from Oxc JSX parsing noise, not asset refs.
    assert!(info.imports.iter().all(|i| !i.source.contains("inline")));
}

#[test]
fn jsx_empty_src_ignored() {
    let info = parse_tsx(
        r#"export const Layout = () => (
          <head>
            <script src=""></script>
          </head>
        );"#,
    );
    assert!(info.imports.iter().all(|i| !i.source.is_empty()));
}
