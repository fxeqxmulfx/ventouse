//! The C++ frontend: the same core (one `ScopeGraph` per file, same scoring) driven by libclang.
//! These lock in the C++-specific surface — block-scoped `levels` narrowing (loop-aware),
//! `struct`/`class` member resolution and in-class method-order warnings, `static` members as data
//! definitions, and that instance fields are order-independent state (no spurious warning).
//!
//! Every test needs a loadable libclang (the `clang` crate); on a machine without one each `extract`
//! returns a `ParseError` instead and these would fail — that is the one environmental prerequisite.

use ventouse::core::{Category, Weights};
use ventouse::lang::cpp::{analyze_project, scope_of, warnings_of};

#[test]
fn clean_local_used_right_after_is_zero() {
    let src = "void f() { int x = make(); sink(x); }\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn block_scoped_local_narrows_into_conditional() {
    // `x` is used only inside the `if` — C++'s REAL block scope is what the `levels` term narrows
    // (a local that could move into the braced block it is used in).
    let src = "int deep_only(bool flag, int a) {\n    int x = a + 1;\n    if (flag) {\n        return x;\n    }\n    return 0;\n}\n";
    assert_eq!(scope_of(src), 10);
}

#[test]
fn local_not_narrowed_into_loop() {
    // loop-carried / accumulator state legitimately lives outside the loop body (loop-aware levels).
    let src = "int total(const int* items, int n) {\n    int acc = 0;\n    for (int i = 0; i < n; i++) {\n        acc += items[i];\n    }\n    return acc;\n}\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn in_class_method_calling_below_is_declare_order() {
    // `handle` calls `helper` declared BELOW it in the same class — the in-class method-order warning
    // (an implicit `this->helper()` resolves to the member in the enclosing class scope).
    let src = "struct Service {\n    int handle() { return helper(); }\n    int helper() { return 1; }\n};\n";
    assert_eq!(warnings_of(src), 1);
}

#[test]
fn in_class_method_calling_above_is_clean() {
    let src = "struct Tidy {\n    int helper() { return 1; }\n    int handle() { return helper(); }\n};\n";
    assert_eq!(warnings_of(src), 0);
}

#[test]
fn static_member_is_data_used_once_not_narrowed() {
    // a `static` data member is class-level DATA (like a Rust associated const): referenced like a
    // definition, not narrowed like a local.
    let src = "struct C {\n    static int N;\n    int f() { return N; }\n};\n";
    assert_eq!(scope_of(src), 0);
    assert_eq!(warnings_of(src), 0);
}

#[test]
fn instance_field_is_order_independent_state() {
    // `get` reads the field `n` declared BELOW it. Instance fields are order-independent class state
    // (like Rust struct fields) — not referenceable defs, so this is NOT a declare-order warning.
    let src = "struct S {\n    int get() { return n; }\n    int n;\n};\n";
    assert_eq!(warnings_of(src), 0);
}

#[test]
fn free_function_forward_call_is_compiler_owned() {
    // A free function called before its declaration is a C++ compile error — the compiler owns that
    // ordering, so the call does not resolve and there is no declare-order warning (unlike in-class
    // methods, where a body may legally reference a member declared later).
    let src = "void caller() { helper(); }\nvoid helper() {}\n";
    assert_eq!(warnings_of(src), 0);
}

#[test]
fn namespace_level_const_is_data_not_a_narrowable_local() {
    // a `const` inside a `namespace` is a definition CONTAINER member (top-level data), exactly like
    // a global — it must NOT be narrowed into the one function/block that reads it. Regression: C++
    // wraps everything in namespaces, so misclassifying these as locals floods real code with false
    // "declared N levels too high" findings (caught dogfooding muduo).
    let src = "namespace ns {\nconst int K = 60;\nint f(bool flag) {\n    if (flag) { return K; }\n    return 0;\n}\n}\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn namespace_function_forward_call_is_compiler_owned() {
    // namespace free functions are ordered by the compiler just like file-scope ones — a forward
    // call does not resolve, so there is no declare-order warning (whereas in-class methods do warn).
    let src = "namespace ns {\nint a() { return b(); }\nint b() { return 1; }\n}\n";
    assert_eq!(warnings_of(src), 0);
}

#[test]
fn parse_error_is_reported_not_panicked() {
    // a malformed file yields a ParseError finding (and no scope-debt), never a panic.
    let src = "int f( {\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn parallel_project_parse_is_correct_and_deterministic() {
    // `analyze_project` parses files across worker threads (libclang parsing is the bottleneck and
    // is run in parallel). Many files at once must (a) not panic / race, (b) match the per-file
    // result, and (c) be deterministic run-to-run. Use enough files to span several worker threads.
    let bad = "struct Service {\n    int handle() { return helper(); }\n    int helper() { return 1; }\n};\n";
    let mut files: Vec<(String, String)> = (0..24).map(|i| (format!("svc{i}.cpp"), bad.to_string())).collect();
    files.push(("clean.cpp".into(), "int add(int a, int b) { return a + b; }\n".into()));
    let refs: Vec<(&str, &str)> = files.iter().map(|(f, s)| (f.as_str(), s.as_str())).collect();

    let run = || {
        let mut w: Vec<(String, u32)> = analyze_project(&refs, &Weights::default())
            .into_iter()
            .filter(|f| f.category == Category::DeclBeforeUse)
            .map(|f| (f.entity, f.line))
            .collect();
        w.sort();
        w
    };
    let first = run();
    // one declare-order warning per `svc*` file (handle calls helper below it); clean.cpp adds none.
    assert_eq!(first.len(), 24);
    assert!(first.iter().all(|(e, _)| e.ends_with("Service.handle")));
    assert_eq!(first, run(), "parallel parsing must be deterministic");
}

/// The `NarrowToBlock` suggestions' `first_use` targets for a source (the narrow-into-block tests).
fn narrows(src: &str) -> Vec<u32> {
    use ventouse::core::model::Reason;
    ventouse::core::analyze::analyze(&[ventouse::lang::cpp::extract(src, "t.cpp").unwrap()], &Weights::default())
        .into_iter()
        .filter_map(|f| match f.reason {
            Reason::NarrowToBlock { first_use, .. } => Some(first_use),
            _ => None,
        })
        .collect()
}

#[test]
fn narrow_into_block_suggests_declaring_at_the_nested_use() {
    // `x` is declared at function top but used only inside the `if` -> suggest declaring it there (L4).
    let src = "int deep_only(bool flag, int a) {\n    int x = a + 1;\n    if (flag) {\n        return x;\n    }\n    return 0;\n}\n";
    assert_eq!(narrows(src), [4]);
}

#[test]
fn narrow_not_suggested_for_loop_carried_or_function_level_use() {
    // loop-carried accumulator (used inside the loop, but the seed must stay outside) -> no narrow.
    let loop_src = "int total(const int* items, int n) {\n    int acc = 0;\n    for (int i = 0; i < n; i++) { acc += items[i]; }\n    return acc;\n}\n";
    assert!(narrows(loop_src).is_empty());
    // used inside the `if` AND returned at function level -> its real scope is the function -> no narrow.
    let ret_src = "int f(bool c) {\n    int x = 0;\n    if (c) { x = 1; }\n    return x;\n}\n";
    assert!(narrows(ret_src).is_empty());
}
