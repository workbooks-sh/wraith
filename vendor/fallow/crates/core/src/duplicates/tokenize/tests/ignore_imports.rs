use super::*;

fn tokenize_skip_imports(code: &str) -> Vec<SourceToken> {
    let path = PathBuf::from("test.ts");
    tokenize_file(&path, code, true).tokens
}

fn tokenize_no_skip(code: &str) -> Vec<SourceToken> {
    let path = PathBuf::from("test.ts");
    tokenize_file(&path, code, false).tokens
}

// ── Basic import filtering ────────────────────────────────────

#[test]
fn skip_imports_removes_value_import() {
    let tokens = tokenize_skip_imports("import { useState } from 'react';");
    let has_import = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Import)));
    assert!(
        !has_import,
        "Value import should be stripped when skip_imports is true"
    );
    assert!(
        tokens.is_empty(),
        "File with only an import should produce no tokens"
    );
}

#[test]
fn skip_imports_removes_default_import() {
    let tokens = tokenize_skip_imports("import React from 'react';");
    assert!(tokens.is_empty(), "Default import should be fully stripped");
}

#[test]
fn skip_imports_removes_namespace_import() {
    let tokens = tokenize_skip_imports("import * as React from 'react';");
    assert!(
        tokens.is_empty(),
        "Namespace import should be fully stripped"
    );
}

#[test]
fn skip_imports_removes_side_effect_import() {
    let tokens = tokenize_skip_imports("import './polyfill';");
    assert!(
        tokens.is_empty(),
        "Side-effect import should be fully stripped"
    );
}

#[test]
fn skip_imports_removes_type_import() {
    let tokens = tokenize_skip_imports("import type { Foo } from './foo';");
    assert!(
        tokens.is_empty(),
        "Type import should be stripped when skip_imports is true"
    );
}

// ── Non-import code preserved ─────────────────────────────────

#[test]
fn skip_imports_preserves_runtime_code() {
    let tokens = tokenize_skip_imports("import { useState } from 'react';\nconst x = useState(0);");
    let has_const = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Const)));
    assert!(has_const, "Runtime code after import should be preserved");
    let has_import = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Import)));
    assert!(!has_import, "Import keyword should be stripped");
}

#[test]
fn skip_imports_preserves_export_declaration() {
    let tokens = tokenize_skip_imports("export function foo() { return 1; }");
    let has_export = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Export)));
    assert!(has_export, "Local export declaration should be preserved");
}

#[test]
fn skip_imports_preserves_export_default() {
    let tokens = tokenize_skip_imports("export default class Foo {}");
    let has_export = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Export)));
    assert!(has_export, "Export default should be preserved");
}

#[test]
fn skip_imports_preserves_reexport() {
    let tokens = tokenize_skip_imports("export { foo } from './foo';");
    let has_export = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Export)));
    assert!(has_export, "Re-export should be preserved (not an import)");
}

#[test]
fn skip_imports_preserves_export_all() {
    let tokens = tokenize_skip_imports("export * from './mod';");
    let has_export = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Export)));
    assert!(has_export, "Export * should be preserved (not an import)");
}

// ── require() NOT filtered ────────────────────────────────────

#[test]
fn skip_imports_does_not_filter_require() {
    let tokens = tokenize_skip_imports("const x = require('foo');");
    let has_const = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Const)));
    assert!(has_const, "require() call should NOT be filtered");
    assert!(
        !tokens.is_empty(),
        "require() is a CallExpression, not an ImportDeclaration"
    );
}

// ── Token count comparison ────────────────────────────────────

#[test]
fn skip_imports_reduces_token_count() {
    let code = "import { a } from 'a';\nimport { b } from 'b';\nconst x = a + b;";
    let with_imports = tokenize_no_skip(code);
    let without_imports = tokenize_skip_imports(code);
    assert!(
        without_imports.len() < with_imports.len(),
        "Skipping imports should produce fewer tokens: with={}, without={}",
        with_imports.len(),
        without_imports.len()
    );
}

#[test]
fn skip_imports_disabled_preserves_imports() {
    let code = "import { useState } from 'react';";
    let tokens = tokenize_no_skip(code);
    let has_import = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Import)));
    assert!(
        has_import,
        "With skip_imports=false, imports should be tokenized"
    );
}

// ── Multiple imports ──────────────────────────────────────────

#[test]
fn skip_imports_removes_sorted_import_block() {
    let code = r"import { A } from './a';
import { B } from './b';
import { C } from './c';
import { D } from './d';
import { E } from './e';

export function process() {
    return A + B + C + D + E;
}";
    let tokens = tokenize_skip_imports(code);
    let import_count = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Import)))
        .count();
    assert_eq!(import_count, 0, "All import declarations should be removed");
    let has_export = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Export)));
    assert!(has_export, "Export function should be preserved");
}

// ── Dynamic import() NOT filtered ─────────────────────────────

#[test]
fn skip_imports_does_not_filter_dynamic_import() {
    let tokens = tokenize_skip_imports("const mod = import('./module');");
    assert!(
        !tokens.is_empty(),
        "Dynamic import() expression should NOT be filtered (it's a CallExpression)"
    );
}

// ── Cross-language + skip_imports combined ─────────────────────

#[test]
fn skip_imports_with_cross_language() {
    let path = PathBuf::from("test.ts");
    let code =
        "import type { Foo } from './foo';\nimport { bar } from './bar';\nconst x: Foo = bar();";
    let tokens = tokenize_file_cross_language(&path, code, true, true).tokens;
    let import_count = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Import)))
        .count();
    assert_eq!(
        import_count, 0,
        "Both type and value imports should be removed when both flags are active"
    );
    let has_const = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Const)));
    assert!(has_const, "Runtime code should be preserved");
}
