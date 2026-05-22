use std::path::Path;

use fallow_types::discover::FileId;
use fallow_types::extract::{ModuleInfo, VisibilityTag};

use crate::parse::parse_source_to_module;

// ── JSX retry fallback ──────────────────────────────────────────

/// Parse as a .js file (not .tsx) to test JSX retry fallback logic.
fn parse_as_js(source: &str) -> ModuleInfo {
    parse_source_to_module(FileId(0), Path::new("component.js"), source, 0, false)
}

#[test]
fn jsx_retry_extracts_exports_from_js_with_jsx() {
    // A .js file with JSX that the initial non-JSX parse can't extract anything from.
    // Must be >100 bytes and have total_extracted == 0 on first pass to trigger retry.
    // The initial parse of .js without JSX mode will fail on JSX tags and extract nothing.
    let source = r#"
export const App = () => <div className="app"><span>Hello World from JSX in a plain JS file</span></div>;
"#;
    let info = parse_as_js(source);
    assert!(
        !info.exports.is_empty(),
        "JSX retry should extract the App export from .js file with JSX"
    );
}

#[test]
fn jsx_retry_extracts_imports_from_js_with_jsx() {
    // File with both import and JSX — the initial .js parse may still extract the import
    // (imports before JSX tags often parse fine), so this tests robustness.
    let source = r#"
export default function Component() {
    return <main><section className="hero"><h1>Title</h1><p>Description paragraph</p></section></main>;
}
"#;
    let info = parse_as_js(source);
    assert!(
        !info.exports.is_empty(),
        "JSX retry should extract the default export from .js file with JSX"
    );
}

#[test]
fn jsx_retry_preserves_jsdoc_public_tag() {
    // Regression: @public tags were read from the original failed parse's comments
    // instead of the retry parse's comments, silently ignoring @public on JSX .js files.
    let source = r#"
/** @public */
export const Button = ({ children }) => <button className="btn">{children}</button>;
"#;
    let info = parse_as_js(source);
    assert!(
        !info.exports.is_empty(),
        "JSX retry should extract Button export"
    );
    assert_eq!(
        info.exports[0].visibility,
        VisibilityTag::Public,
        "@public JSDoc tag must be recognized on JSX exports in .js files"
    );
}

#[test]
fn jsx_retry_preserves_suppressions() {
    // Regression: suppression comments were parsed from the original failed parse's
    // comments instead of the retry parse's comments.
    let source = r#"
// fallow-ignore-next-line unused-export
export const Unused = ({ text }) => <span className="unused-component">{text}</span>;
"#;
    let info = parse_as_js(source);
    assert!(
        !info.suppressions.is_empty(),
        "Suppressions must be parsed from retry parse comments, not the original failed parse"
    );
}

// ── JSX retry logic ─────────────────────────────────────────

#[test]
fn jsx_in_js_file_retry_extracts_imports() {
    // Parse as .js file (not .jsx) with JSX content — should retry as JSX
    let info = parse_source_to_module(
        FileId(0),
        Path::new("component.js"),
        r"import React from 'react';
import { Button } from './Button';

const App = () => <Button>Hello</Button>;
export default App;",
        0,
        false,
    );
    assert!(
        info.imports.iter().any(|i| i.source == "react"),
        "JSX retry should extract imports from JSX in .js file"
    );
    assert!(
        info.imports.iter().any(|i| i.source == "./Button"),
        "JSX retry should extract all imports"
    );
}
