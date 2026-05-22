use crate::tests::parse_ts_with_complexity as parse_source;

// ── Cyclomatic & cognitive complexity (via ModuleInfo.complexity) ──

#[test]
fn complexity_basic_if_else_for_while_switch() {
    let info = parse_source(
        r"function basic(x: number) {
            if (x > 10) {
                return 'big';
            } else {
                for (let i = 0; i < x; i++) {}
                while (x > 0) { x--; }
                switch (x) {
                    case 0: break;
                    case 1: break;
                    default: break;
                }
            }
        }",
    );
    let f = info.complexity.iter().find(|c| c.name == "basic").unwrap();
    // 1 (base) + if + for + while + case + case = 6 (default: not counted)
    assert_eq!(f.cyclomatic, 6);
}

#[test]
fn complexity_nested_if_in_for_loop() {
    let info = parse_source(
        r"function nested(items: number[]) {
            for (const item of items) {
                if (item > 0) {
                    return item;
                }
            }
        }",
    );
    let f = info.complexity.iter().find(|c| c.name == "nested").unwrap();
    // Cyclomatic: 1 + for_of + if = 3
    assert_eq!(f.cyclomatic, 3);
    // Cognitive: for_of +1 (n=0), if +1+1 (n=1) = 3
    assert_eq!(f.cognitive, 3);
}

#[test]
fn complexity_deeply_nested_three_levels() {
    let info = parse_source(
        r"function deep(a: boolean, b: boolean, c: boolean) {
            if (a) {
                for (let i = 0; i < 10; i++) {
                    while (b) {
                        if (c) {
                            break;
                        }
                    }
                }
            }
        }",
    );
    let f = info.complexity.iter().find(|c| c.name == "deep").unwrap();
    // Cyclomatic: 1 + if + for + while + if = 5
    assert_eq!(f.cyclomatic, 5);
    // Cognitive: if +1 (n=0), for +1+1 (n=1), while +1+2 (n=2), if +1+3 (n=3) = 1+2+3+4 = 10
    assert_eq!(f.cognitive, 10);
}

#[test]
fn complexity_boolean_same_operator_sequence() {
    let info = parse_source(
        "function sameBool(a: boolean, b: boolean, c: boolean) { return a && b && c; }",
    );
    let f = info
        .complexity
        .iter()
        .find(|c| c.name == "sameBool")
        .unwrap();
    // Cyclomatic: 1 + && + && = 3
    assert_eq!(f.cyclomatic, 3);
    // Cognitive: same operator throughout = +1
    assert_eq!(f.cognitive, 1);
}

#[test]
fn complexity_boolean_mixed_operator_sequence() {
    let info = parse_source(
        "function mixedBool(a: boolean, b: boolean, c: boolean) { return a && b || c; }",
    );
    let f = info
        .complexity
        .iter()
        .find(|c| c.name == "mixedBool")
        .unwrap();
    // Cyclomatic: 1 + && + || = 3
    assert_eq!(f.cyclomatic, 3);
    // Cognitive: && starts sequence +1, || changes operator +1 = 2
    assert_eq!(f.cognitive, 2);
}

#[test]
fn complexity_boolean_three_operator_changes() {
    let info = parse_source(
        "function threeBool(a: boolean, b: boolean, c: boolean, d: boolean) { return a && b || c && d; }",
    );
    let f = info
        .complexity
        .iter()
        .find(|c| c.name == "threeBool")
        .unwrap();
    // Cyclomatic: 1 + && + || + && = 4
    assert_eq!(f.cyclomatic, 4);
    // Cognitive: && +1, || +1, && +1 = 3
    assert_eq!(f.cognitive, 3);
}

#[test]
fn complexity_ternary_operator() {
    let info = parse_source("function tern(x: number) { return x > 0 ? 'pos' : 'non-pos'; }");
    let f = info.complexity.iter().find(|c| c.name == "tern").unwrap();
    // Cyclomatic: 1 + ternary = 2
    assert_eq!(f.cyclomatic, 2);
    // Cognitive: ternary +1 (n=0) = 1
    assert_eq!(f.cognitive, 1);
}

#[test]
fn complexity_nested_ternary() {
    let info = parse_source(
        "function nestedTern(x: number) { return x > 0 ? 'pos' : x < 0 ? 'neg' : 'zero'; }",
    );
    let f = info
        .complexity
        .iter()
        .find(|c| c.name == "nestedTern")
        .unwrap();
    // Cyclomatic: 1 + ternary + ternary = 3
    assert_eq!(f.cyclomatic, 3);
    // Cognitive: outer ternary +1 (n=0), inner ternary +1+1 (n=1) = 3
    assert_eq!(f.cognitive, 3);
}

#[test]
fn complexity_try_catch() {
    let info = parse_source(
        r"function tryCatch() {
            try {
                riskyOp();
            } catch (e) {
                handleError(e);
            }
        }",
    );
    let f = info
        .complexity
        .iter()
        .find(|c| c.name == "tryCatch")
        .unwrap();
    // Cyclomatic: 1 + catch = 2
    assert_eq!(f.cyclomatic, 2);
    // Cognitive: catch +1 (n=0) = 1
    assert_eq!(f.cognitive, 1);
}

#[test]
fn complexity_try_catch_with_nested_if() {
    let info = parse_source(
        r"function tryCatchNested(x: boolean) {
            try {
                if (x) { riskyOp(); }
            } catch (e) {
                if (e instanceof Error) { log(e); }
            }
        }",
    );
    let f = info
        .complexity
        .iter()
        .find(|c| c.name == "tryCatchNested")
        .unwrap();
    // Cyclomatic: 1 + if + catch + if = 4
    assert_eq!(f.cyclomatic, 4);
    // Cognitive: if +1 (n=0), catch +1 (n=0), if inside catch +1+1 (n=1) = 4
    assert_eq!(f.cognitive, 4);
}

#[test]
fn complexity_nested_functions_independent() {
    let info = parse_source(
        r"function outer(x: boolean) {
            if (x) {}
            function inner(y: boolean) {
                if (y) {
                    if (y) {}
                }
            }
        }",
    );
    let outer = info.complexity.iter().find(|c| c.name == "outer").unwrap();
    let inner = info.complexity.iter().find(|c| c.name == "inner").unwrap();
    // outer: 1 + if = 2 cyclomatic, if +1 = 1 cognitive
    assert_eq!(outer.cyclomatic, 2);
    assert_eq!(outer.cognitive, 1);
    // inner: 1 + if + if = 3 cyclomatic, if +1 (n=0) + if +1+1 (n=1) = 3 cognitive
    assert_eq!(inner.cyclomatic, 3);
    assert_eq!(inner.cognitive, 3);
}

#[test]
fn complexity_arrow_function_in_callback() {
    let info = parse_source(
        r"function process(items: number[]) {
            items.map((item) => {
                if (item > 0) {
                    return item * 2;
                }
                return 0;
            });
        }",
    );
    let outer = info
        .complexity
        .iter()
        .find(|c| c.name == "process")
        .unwrap();
    let arrow = info
        .complexity
        .iter()
        .find(|c| c.name == "<arrow>")
        .unwrap();
    // outer: base 1 only (no decisions in outer scope)
    assert_eq!(outer.cyclomatic, 1);
    assert_eq!(outer.cognitive, 0);
    // arrow: 1 + if = 2 cyclomatic, if +1 (n=0, reset for new function) = 1 cognitive
    assert_eq!(arrow.cyclomatic, 2);
    assert_eq!(arrow.cognitive, 1);
}

#[test]
fn complexity_named_arrow_in_variable() {
    let info = parse_source(
        r"function process(items: number[]) {
            const filter = (item: number) => item > 0;
            return items.filter(filter);
        }",
    );
    let arrow = info.complexity.iter().find(|c| c.name == "filter").unwrap();
    // Arrow with no decisions: base 1 cyclomatic, 0 cognitive
    assert_eq!(arrow.cyclomatic, 1);
    assert_eq!(arrow.cognitive, 0);
}

#[test]
fn complexity_class_methods_independent() {
    let info = parse_source(
        r"class Parser {
            parse(input: string) {
                if (input.length === 0) { return null; }
                for (let i = 0; i < input.length; i++) {
                    if (input[i] === '{') { return this.parseObject(input); }
                }
                return input;
            }
            validate(input: string) {
                return input ? true : false;
            }
        }",
    );
    let parse = info.complexity.iter().find(|c| c.name == "parse").unwrap();
    let validate = info
        .complexity
        .iter()
        .find(|c| c.name == "validate")
        .unwrap();
    // parse: 1 + if + for + if = 4
    assert_eq!(parse.cyclomatic, 4);
    // parse cognitive: if +1 (n=0), for +1 (n=0), if +1+1 (n=1) = 4
    assert_eq!(parse.cognitive, 4);
    // validate: 1 + ternary = 2
    assert_eq!(validate.cyclomatic, 2);
    // validate cognitive: ternary +1 (n=0) = 1
    assert_eq!(validate.cognitive, 1);
}

#[test]
fn complexity_class_property_arrow() {
    let info = parse_source(
        r"class Handler {
            handle = (x: number) => {
                if (x > 0) { return x; }
                return 0;
            };
        }",
    );
    let handle = info.complexity.iter().find(|c| c.name == "handle").unwrap();
    // 1 + if = 2
    assert_eq!(handle.cyclomatic, 2);
    assert_eq!(handle.cognitive, 1);
}

#[test]
fn complexity_nullish_coalescing() {
    let info = parse_source("function nc(a?: string) { return a ?? 'default'; }");
    let f = info.complexity.iter().find(|c| c.name == "nc").unwrap();
    // Cyclomatic: 1 + ?? = 2
    assert_eq!(f.cyclomatic, 2);
    // Cognitive: ?? is a logical operator, gets +1 for the sequence
    assert_eq!(f.cognitive, 1);
}

#[test]
fn complexity_nullish_coalescing_chain() {
    let info =
        parse_source("function ncChain(a?: string, b?: string) { return a ?? b ?? 'default'; }");
    let f = info
        .complexity
        .iter()
        .find(|c| c.name == "ncChain")
        .unwrap();
    // Cyclomatic: 1 + ?? + ?? = 3
    assert_eq!(f.cyclomatic, 3);
    // Cognitive: same operator ?? throughout = +1
    assert_eq!(f.cognitive, 1);
}

#[test]
fn complexity_logical_and_assignment() {
    let info = parse_source("function la(obj: any) { obj.value &&= 'assigned'; }");
    let f = info.complexity.iter().find(|c| c.name == "la").unwrap();
    // Cyclomatic: 1 + &&= = 2
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn complexity_logical_or_assignment() {
    let info = parse_source("function lo(obj: any) { obj.value ||= 'fallback'; }");
    let f = info.complexity.iter().find(|c| c.name == "lo").unwrap();
    // Cyclomatic: 1 + ||= = 2
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn complexity_nullish_assignment() {
    let info = parse_source("function na(obj: any) { obj.value ??= 'default'; }");
    let f = info.complexity.iter().find(|c| c.name == "na").unwrap();
    // Cyclomatic: 1 + ??= = 2
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn complexity_all_logical_assignments() {
    let info = parse_source("function allAssign(o: any) { o.a &&= 1; o.b ||= 2; o.c ??= 3; }");
    let f = info
        .complexity
        .iter()
        .find(|c| c.name == "allAssign")
        .unwrap();
    // Cyclomatic: 1 + &&= + ||= + ??= = 4
    assert_eq!(f.cyclomatic, 4);
}

#[test]
fn complexity_optional_chaining_cyclomatic_only() {
    let info = parse_source("function oc(obj: any) { return obj?.a?.b; }");
    let f = info.complexity.iter().find(|c| c.name == "oc").unwrap();
    // Cyclomatic: optional chaining adds to cyclomatic
    assert!(
        f.cyclomatic >= 2,
        "optional chaining should add to cyclomatic"
    );
    // Cognitive: optional chaining is NOT counted (Principle 3)
    assert_eq!(f.cognitive, 0);
}

#[test]
fn complexity_do_while_loop() {
    let info = parse_source(
        r"function doWhile(x: number) {
            do {
                x--;
            } while (x > 0);
        }",
    );
    let f = info
        .complexity
        .iter()
        .find(|c| c.name == "doWhile")
        .unwrap();
    // Cyclomatic: 1 + do-while = 2
    assert_eq!(f.cyclomatic, 2);
    // Cognitive: do-while +1 (n=0) = 1
    assert_eq!(f.cognitive, 1);
}

#[test]
fn complexity_for_in_loop() {
    let info = parse_source(
        r"function forIn(obj: Record<string, number>) {
            for (const key in obj) {
                if (obj[key] > 0) {}
            }
        }",
    );
    let f = info.complexity.iter().find(|c| c.name == "forIn").unwrap();
    // Cyclomatic: 1 + for-in + if = 3
    assert_eq!(f.cyclomatic, 3);
    // Cognitive: for-in +1 (n=0), if +1+1 (n=1) = 3
    assert_eq!(f.cognitive, 3);
}

#[test]
fn complexity_switch_cognitive_is_flat() {
    let info = parse_source(
        r"function sw(x: number) {
            switch (x) {
                case 1: return 'one';
                case 2: return 'two';
                case 3: return 'three';
                default: return 'other';
            }
        }",
    );
    let f = info.complexity.iter().find(|c| c.name == "sw").unwrap();
    // Cyclomatic: 1 + case + case + case = 4 (default: not counted)
    assert_eq!(f.cyclomatic, 4);
    // Cognitive: switch +1 (not per-case)
    assert_eq!(f.cognitive, 1);
}

#[test]
fn complexity_else_if_chain_cognitive_flat() {
    let info = parse_source(
        r"function elseIfChain(x: number) {
            if (x === 1) {
                return 'one';
            } else if (x === 2) {
                return 'two';
            } else if (x === 3) {
                return 'three';
            } else {
                return 'other';
            }
        }",
    );
    let f = info
        .complexity
        .iter()
        .find(|c| c.name == "elseIfChain")
        .unwrap();
    // Cyclomatic: 1 + if + else-if + else-if = 4
    assert_eq!(f.cyclomatic, 4);
    // Cognitive: if +1, else if +1 (flat), else if +1 (flat), else +1 (flat) = 4
    assert_eq!(f.cognitive, 4);
}

#[test]
fn complexity_break_with_label() {
    let info = parse_source(
        r"function labeled() {
            outer: for (let i = 0; i < 10; i++) {
                for (let j = 0; j < 10; j++) {
                    if (i + j > 5) {
                        break outer;
                    }
                }
            }
        }",
    );
    let f = info
        .complexity
        .iter()
        .find(|c| c.name == "labeled")
        .unwrap();
    // Cyclomatic: 1 + for + for + if = 4
    assert_eq!(f.cyclomatic, 4);
    // Cognitive: for +1 (n=0), for +1+1 (n=1), if +1+2 (n=2), break label +1 (flat) = 7
    assert_eq!(f.cognitive, 7);
}

#[test]
fn complexity_continue_with_label() {
    let info = parse_source(
        r"function labeledContinue() {
            outer: for (let i = 0; i < 10; i++) {
                for (let j = 0; j < 10; j++) {
                    if (j === 3) {
                        continue outer;
                    }
                }
            }
        }",
    );
    let f = info
        .complexity
        .iter()
        .find(|c| c.name == "labeledContinue")
        .unwrap();
    // Cognitive includes +1 for continue label
    assert_eq!(f.cognitive, 7);
}

#[test]
fn complexity_mixed_boolean_with_nullish() {
    let info = parse_source(
        "function mixedNullish(a: boolean, b?: string) { return a && b ?? 'default'; }",
    );
    let f = info
        .complexity
        .iter()
        .find(|c| c.name == "mixedNullish")
        .unwrap();
    // Cyclomatic: 1 + && + ?? = 3
    assert_eq!(f.cyclomatic, 3);
    // Cognitive: && starts +1, ?? changes operator +1 = 2
    assert_eq!(f.cognitive, 2);
}

#[test]
fn complexity_boolean_in_if_condition() {
    let info = parse_source(
        r"function boolInIf(a: boolean, b: boolean) {
            if (a && b) {
                return true;
            }
            return false;
        }",
    );
    let f = info
        .complexity
        .iter()
        .find(|c| c.name == "boolInIf")
        .unwrap();
    // Cyclomatic: 1 + if + && = 3
    assert_eq!(f.cyclomatic, 3);
    // Cognitive: if +1 (n=0) + && +1 (flat, boolean sequence) = 2
    assert_eq!(f.cognitive, 2);
}

#[test]
fn complexity_multiple_independent_functions() {
    let info = parse_source(
        r"
        function a(x: boolean) { if (x) {} }
        function b(x: boolean, y: boolean) { if (x) { if (y) {} } }
        function c() {}
        ",
    );
    let fa = info.complexity.iter().find(|c| c.name == "a").unwrap();
    let fb = info.complexity.iter().find(|c| c.name == "b").unwrap();
    let fc = info.complexity.iter().find(|c| c.name == "c").unwrap();
    assert_eq!(fa.cyclomatic, 2);
    assert_eq!(fa.cognitive, 1);
    assert_eq!(fb.cyclomatic, 3);
    assert_eq!(fb.cognitive, 3);
    assert_eq!(fc.cyclomatic, 1);
    assert_eq!(fc.cognitive, 0);
}

#[test]
fn complexity_export_default_anonymous_function() {
    let info = parse_source("export default function() { if (true) { while (true) {} } }");
    let f = info
        .complexity
        .iter()
        .find(|c| c.name == "default")
        .unwrap();
    // Cyclomatic: 1 + if + while = 3
    assert_eq!(f.cyclomatic, 3);
    // Cognitive: if +1 (n=0), while +1+1 (n=1) = 3
    assert_eq!(f.cognitive, 3);
}

#[test]
fn complexity_object_method_shorthand() {
    let info = parse_source(
        r"const obj = {
            process(x: number) {
                if (x > 0) { return x; }
                return 0;
            }
        };",
    );
    let f = info
        .complexity
        .iter()
        .find(|c| c.name == "process")
        .unwrap();
    assert_eq!(f.cyclomatic, 2);
    assert_eq!(f.cognitive, 1);
}

#[test]
fn complexity_catch_increases_nesting() {
    let info = parse_source(
        r"function tryCatchDeep() {
            try {
                riskyOp();
            } catch (e) {
                if (e instanceof Error) {
                    for (const c of e.message) {
                        log(c);
                    }
                }
            }
        }",
    );
    let f = info
        .complexity
        .iter()
        .find(|c| c.name == "tryCatchDeep")
        .unwrap();
    // Cyclomatic: 1 + catch + if + for_of = 4
    assert_eq!(f.cyclomatic, 4);
    // Cognitive: catch +1 (n=0), if +1+1 (n=1), for_of +1+2 (n=2) = 6
    assert_eq!(f.cognitive, 6);
}
