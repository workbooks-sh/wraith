use fallow_types::extract::ImportedName;

use crate::tests::parse_ts as parse_source;

// -- Whole-object use detection --

#[test]
fn detects_object_values_whole_use() {
    let info = parse_source("import { Status } from './types';\nObject.values(Status);");
    assert!(info.whole_object_uses.contains(&"Status".to_string()));
}

#[test]
fn detects_object_keys_whole_use() {
    let info = parse_source("import { Dir } from './types';\nObject.keys(Dir);");
    assert!(info.whole_object_uses.contains(&"Dir".to_string()));
}

#[test]
fn detects_object_entries_whole_use() {
    let info = parse_source("import { E } from './types';\nObject.entries(E);");
    assert!(info.whole_object_uses.contains(&"E".to_string()));
}

#[test]
fn detects_for_in_whole_use() {
    let info = parse_source("import { Color } from './types';\nfor (const k in Color) {}");
    assert!(info.whole_object_uses.contains(&"Color".to_string()));
}

#[test]
fn detects_spread_whole_use() {
    let info = parse_source("import { X } from './types';\nconst y = { ...X };");
    assert!(info.whole_object_uses.contains(&"X".to_string()));
}

#[test]
fn computed_member_string_literal_resolves() {
    let info = parse_source("import { Status } from './types';\nStatus[\"Active\"];");
    let has_access = info
        .member_accesses
        .iter()
        .any(|a| a.object == "Status" && a.member == "Active");
    assert!(
        has_access,
        "Status[\"Active\"] should resolve to a static member access"
    );
}

#[test]
fn computed_member_variable_marks_whole_use() {
    let info = parse_source("import { Status } from './types';\nconst k = 'foo';\nStatus[k];");
    assert!(info.whole_object_uses.contains(&"Status".to_string()));
}

// -- Namespace destructuring detection --

#[test]
fn namespace_destructuring_generates_member_accesses() {
    let info = parse_source("import * as utils from './utils';\nconst { foo, bar } = utils;");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].imported_name, ImportedName::Namespace);
    let has_foo = info
        .member_accesses
        .iter()
        .any(|a| a.object == "utils" && a.member == "foo");
    let has_bar = info
        .member_accesses
        .iter()
        .any(|a| a.object == "utils" && a.member == "bar");
    assert!(
        has_foo,
        "Should capture destructured 'foo' as member access"
    );
    assert!(
        has_bar,
        "Should capture destructured 'bar' as member access"
    );
}

#[test]
fn namespace_destructuring_with_rest_marks_whole_object() {
    let info = parse_source("import * as utils from './utils';\nconst { foo, ...rest } = utils;");
    assert!(
        info.whole_object_uses.contains(&"utils".to_string()),
        "Rest pattern should mark namespace as whole-object use"
    );
}

#[test]
fn namespace_destructuring_from_dynamic_import() {
    let info = parse_source(
        "async function f() {\n  const mod = await import('./mod');\n  const { a, b } = mod;\n}",
    );
    let has_a = info
        .member_accesses
        .iter()
        .any(|a| a.object == "mod" && a.member == "a");
    let has_b = info
        .member_accesses
        .iter()
        .any(|a| a.object == "mod" && a.member == "b");
    assert!(
        has_a,
        "Should capture destructured 'a' from dynamic import namespace"
    );
    assert!(
        has_b,
        "Should capture destructured 'b' from dynamic import namespace"
    );
}

#[test]
fn namespace_destructuring_from_require() {
    let info = parse_source("const mod = require('./mod');\nconst { x, y } = mod;");
    let has_x = info
        .member_accesses
        .iter()
        .any(|a| a.object == "mod" && a.member == "x");
    let has_y = info
        .member_accesses
        .iter()
        .any(|a| a.object == "mod" && a.member == "y");
    assert!(
        has_x,
        "Should capture destructured 'x' from require namespace"
    );
    assert!(
        has_y,
        "Should capture destructured 'y' from require namespace"
    );
}

#[test]
fn non_namespace_destructuring_not_captured() {
    let info =
        parse_source("import { foo } from './utils';\nconst obj = { a: 1 };\nconst { a } = obj;");
    // 'obj' is not a namespace import, so destructuring should not add member_accesses for it
    let has_obj_a = info
        .member_accesses
        .iter()
        .any(|a| a.object == "obj" && a.member == "a");
    assert!(
        !has_obj_a,
        "Should not capture destructuring of non-namespace variables"
    );
}
