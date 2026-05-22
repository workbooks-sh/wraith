//! `` html`...` `` tagged template literal asset extraction tests.
//!
//! Mirrors the HTML parser and the JSX `<script src>` / `<link href>` override
//! for SSR helpers like `hono/html`, `lit-html`, and `htm`, where layout
//! components emit HTML via a tagged template whose tag is the identifier
//! `html`. See issue #105 (till's follow-up comment).

use fallow_types::extract::ImportedName;

use crate::tests::parse_ts;

#[test]
fn html_tagged_template_script_src_extracted() {
    let info = parse_ts(
        r#"import { html } from "hono/html";
export const Layout = ({ title, body }) => html`
  <!doctype html>
  <html>
    <head>
      <title>${title}</title>
      <script defer src="/static/otp-input.js"></script>
    </head>
    <body>${body}</body>
  </html>
`;"#,
    );
    let imp = info
        .imports
        .iter()
        .find(|i| i.source == "/static/otp-input.js")
        .expect("html`` <script src> should produce an ImportInfo");
    assert!(matches!(imp.imported_name, ImportedName::SideEffect));
    assert!(imp.local_name.is_empty());
    assert!(!imp.is_type_only);
}

#[test]
fn html_tagged_template_link_stylesheet_extracted() {
    let info = parse_ts(
        r#"import { html } from "hono/html";
export const Layout = () => html`
  <head>
    <link rel="stylesheet" href="/static/global.css" />
  </head>
`;"#,
    );
    assert!(
        info.imports
            .iter()
            .any(|i| i.source == "/static/global.css")
    );
}

#[test]
fn html_tagged_template_link_modulepreload_extracted() {
    let info = parse_ts(
        r#"import { html } from "hono/html";
export const Layout = () => html`
  <head>
    <link rel="modulepreload" href="/static/vendor.js" />
  </head>
`;"#,
    );
    assert!(info.imports.iter().any(|i| i.source == "/static/vendor.js"));
}

#[test]
fn html_tagged_template_link_reversed_attr_order_extracted() {
    let info = parse_ts(
        r#"import { html } from "hono/html";
export const Layout = () => html`
  <head>
    <link href="./style.css" rel="stylesheet" />
  </head>
`;"#,
    );
    assert!(info.imports.iter().any(|i| i.source == "./style.css"));
}

#[test]
fn html_tagged_template_bare_src_normalized() {
    // Bare specifiers become `./foo.js` so the resolver doesn't treat them
    // as npm packages — same behavior as the HTML parser.
    let info = parse_ts(
        r#"import { html } from "hono/html";
export const Layout = () => html`
  <head>
    <script src="app.js"></script>
  </head>
`;"#,
    );
    assert!(info.imports.iter().any(|i| i.source == "./app.js"));
}

#[test]
fn html_tagged_template_multiple_assets() {
    let info = parse_ts(
        r#"import { html } from "hono/html";
export const Layout = () => html`
  <head>
    <link rel="stylesheet" href="/static/global.css" />
    <link rel="modulepreload" href="/static/vendor.js" />
    <script src="/static/app.js"></script>
  </head>
`;"#,
    );
    let sources: Vec<&str> = info.imports.iter().map(|i| i.source.as_str()).collect();
    assert!(sources.contains(&"/static/global.css"));
    assert!(sources.contains(&"/static/vendor.js"));
    assert!(sources.contains(&"/static/app.js"));
}

#[test]
fn html_tagged_template_remote_urls_skipped() {
    let info = parse_ts(
        r#"import { html } from "hono/html";
export const Layout = () => html`
  <head>
    <script src="https://cdn.example.com/lib.js"></script>
    <link rel="stylesheet" href="//cdn.example.com/style.css" />
    <script src="http://example.com/legacy.js"></script>
  </head>
`;"#,
    );
    assert!(
        info.imports
            .iter()
            .all(|i| !i.source.contains("example.com"))
    );
}

#[test]
fn html_tagged_template_multi_line_attributes() {
    let info = parse_ts(
        r#"import { html } from "hono/html";
export const Layout = () => html`
  <link
    rel="stylesheet"
    href="/static/multi-line.css"
  />
`;"#,
    );
    assert!(
        info.imports
            .iter()
            .any(|i| i.source == "/static/multi-line.css")
    );
}

#[test]
fn html_tagged_template_comments_stripped() {
    // HTML comments must not produce asset imports — the commented-out script
    // is dead markup that should never reach the graph.
    let info = parse_ts(
        r#"import { html } from "hono/html";
export const Layout = () => html`
  <head>
    <!-- <script src="/static/old.js"></script> -->
    <script src="/static/new.js"></script>
  </head>
`;"#,
    );
    assert!(info.imports.iter().all(|i| i.source != "/static/old.js"));
    assert!(info.imports.iter().any(|i| i.source == "/static/new.js"));
}

#[test]
fn html_tagged_template_interpolated_asset_across_boundary_skipped() {
    // An asset reference split across an interpolation boundary can't be
    // statically resolved, so both halves are ignored — preventing bogus
    // imports like `./${base}.js` from flooding the resolver.
    let info = parse_ts(
        r#"import { html } from "hono/html";
const base = "/static";
export const Layout = () => html`
  <script src="${base}/app.js"></script>
`;"#,
    );
    assert!(info.imports.iter().all(|i| !i.source.ends_with("/app.js")));
}

#[test]
fn html_tagged_template_rel_icon_ignored() {
    // Only stylesheet/modulepreload rel values are tracked — matching the
    // HTML parser's whitelist.
    let info = parse_ts(
        r#"import { html } from "hono/html";
export const Layout = () => html`
  <head>
    <link rel="icon" href="/static/favicon.ico" />
  </head>
`;"#,
    );
    assert!(
        info.imports
            .iter()
            .all(|i| !i.source.contains("favicon.ico"))
    );
}

#[test]
fn non_html_tag_ignored() {
    // `css`, `sql`, `gql`, `styled.div` tagged templates are completely
    // outside the scope of this override. No asset imports should be
    // emitted, even though their text could match the HTML regex.
    let info = parse_ts(
        r#"const css = (strings: TemplateStringsArray, ...values: unknown[]) => "";
const style = css`
  <script src="./should-not-track.js"></script>
`;"#,
    );
    assert!(
        info.imports
            .iter()
            .all(|i| !i.source.contains("should-not-track"))
    );
}

#[test]
fn html_tagged_template_in_jsx_file_also_works() {
    // Layouts can live in .tsx files and still use the html`` tag — make sure
    // the override fires regardless of source type.
    let info = crate::tests::parse_tsx(
        r#"import { html } from "hono/html";
export const Layout = () => html`
  <head>
    <script src="/static/from-tsx.js"></script>
  </head>
`;"#,
    );
    assert!(
        info.imports
            .iter()
            .any(|i| i.source == "/static/from-tsx.js")
    );
}

#[test]
fn html_tagged_template_empty_src_ignored() {
    let info = parse_ts(
        r#"import { html } from "hono/html";
export const Layout = () => html`
  <head>
    <script src=""></script>
  </head>
`;"#,
    );
    assert!(info.imports.iter().all(|i| !i.source.is_empty()));
}
