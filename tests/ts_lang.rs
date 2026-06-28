//! The TypeScript / JavaScript frontend (oxc) over the shared core. Verifies that the locality
//! rules hold through the TS/JS surface — block-scoped `let`/`const`, classes/methods, arrows named
//! after their binding, React hook pinning, and JSX references — exactly the same metric as Python.

use ventouse::core::{Category, Reason};
use ventouse::lang::ts::{analyze_source, scope_of, warnings_of};

fn findings(src: &str, file: &str) -> Vec<ventouse::core::Finding> {
    analyze_source(src, file, &Default::default())
}

fn has_reorder(src: &str) -> bool {
    findings(src, "t.ts").iter().any(|f| matches!(f.reason, Reason::ReorderBinding { .. }))
}

// --- value locality (block-scoped `let`/`const`) -----------------------------------------

#[test]
fn levels_value_used_one_block_deeper() {
    let src = "function f(flag: boolean) {\n  const x = compute();\n  if (flag) {\n    return use(x);\n  }\n}\n";
    assert_eq!(scope_of(src), 10); // ExcessLevels(1)
}

#[test]
fn tight_value_scores_zero() {
    let src = "function f() {\n  const x = compute();\n  return use(x);\n}\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn reorder_three_unrelated_defs_between_binding_and_use() {
    let src = "function f() {\n  const q = setup();\n  const a = one();\n  const b = two();\n  const c = three();\n  log(a, b, c);\n  return run(q);\n}\n";
    assert_eq!(scope_of(src), 30); // Misplaced(3)
    assert!(has_reorder(src), "a scattered local must get a ReorderBinding suggestion");
}

// --- definitions: placement + declare-order ----------------------------------------------

#[test]
fn forward_reference_between_functions() {
    let src = "function caller() { return leaf(); }\nfunction leaf() { return 1; }\n";
    assert_eq!(warnings_of(src), 1); // `leaf` is declared below its caller
}

#[test]
fn definition_wedged_from_its_dependency() {
    let src = "const GRAVITY = 9.81;\nfunction noise() { return 0; }\nfunction weight(m: number) { return m * GRAVITY; }\n";
    let f = findings(src, "t.ts");
    assert!(f.iter().any(|x| x.entity == "weight" && matches!(x.reason, Reason::Misplaced(1))));
}

// --- classes: methods, `this.` members, fields -------------------------------------------

#[test]
fn class_method_forward_reference_via_this() {
    let src = "class C {\n  a() { return this.b(); }\n  b() { return 1; }\n}\n";
    // bottom-up: `b` is defined below `a`, referenced via `this.b()` → one declare-order warning.
    assert_eq!(warnings_of(src), 1);
}

// --- arrows named after their binding (the dominant JS/React shape) -----------------------

#[test]
fn arrow_assigned_to_const_is_a_named_definition() {
    // `const caller = () => leaf()` is a definition `caller`; the forward reference to `leaf` (a
    // function declared below) is attributed to it — proving the arrow became a named scope.
    let src = "const caller = () => leaf();\nfunction leaf() { return 1; }\n";
    let f = findings(src, "t.ts");
    assert!(f.iter().any(|x| x.entity == "caller" && matches!(x.reason, Reason::ForwardRef { .. })));
}

// --- React: hook results are positionally pinned -----------------------------------------

#[test]
fn react_hook_result_is_not_flagged_for_reorder() {
    // A `useState` result must sit at the top (rules of hooks) and cannot move down — so unlike a
    // plain local, junk between it and its use is NOT counted.
    let hook = "function Comp() {\n  const x = useState();\n  const a = j1();\n  const b = j2();\n  const c = j3();\n  return run(x);\n}\n";
    let plain = "function Comp() {\n  const x = plainCall();\n  const a = j1();\n  const b = j2();\n  const c = j3();\n  return run(x);\n}\n";
    assert_eq!(scope_of(hook), 0, "a hook result is positionally pinned → no wedge debt");
    assert_eq!(scope_of(plain), 30, "a plain local with the same shape IS flagged");
}

// --- JSX (React): a component reference in a tag is an edge -------------------------------

#[test]
fn jsx_component_reference_participates_in_declare_order() {
    // `App` returns `<Helper/>`; `Helper` is declared below → the JSX reference is a real edge, so
    // the forward reference is warned (proving JSX is lowered, not skipped).
    let src = "function App() {\n  return <Helper />;\n}\nfunction Helper() {\n  return null;\n}\n";
    assert_eq!(warnings_of_file(src, "App.tsx"), 1);
}

fn warnings_of_file(src: &str, file: &str) -> usize {
    findings(src, file).iter().filter(|f| f.category == Category::DeclBeforeUse).count()
}

#[test]
fn jsx_expression_container_references_are_seen() {
    // a handler used only inside a `{onClick}` container is still referenced (an edge), so a callee
    // defined below its only JSX use is a forward reference.
    let src = "function App() {\n  return <button onClick={handleClick} />;\n}\nfunction handleClick() {}\n";
    assert_eq!(warnings_of_file(src, "App.tsx"), 1);
}

#[test]
fn parses_a_realistic_tsx_component_without_error() {
    let src = "import React, { useState } from 'react';\nexport const Counter = ({ start }: { start: number }) => {\n  const [n, setN] = useState(start);\n  const inc = () => setN(n + 1);\n  return <button onClick={inc}>{n}</button>;\n};\n";
    assert!(!findings(src, "Counter.tsx").iter().any(|f| f.category == Category::ParseError));
}
