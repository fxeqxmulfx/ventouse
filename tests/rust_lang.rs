//! The Rust frontend: the same core (one `ScopeGraph` per file, same scoring) driven by `syn`.
//! These lock in the Rust-specific surface — block-scoped `let` narrowing, `impl`/`self` member
//! resolution, `const`/`static` data definitions, and that mutually-exclusive `match` arms do not
//! wedge each other.

use ventouse::lang::rust::{scope_of, warnings_of};

#[test]
fn clean_local_used_right_after_is_zero() {
    let src = "fn f() { let x = make(); sink(x); }\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn block_scoped_let_narrows_into_conditional() {
    // a `let` used only inside an `if` could move into that block — Rust's REAL block scope is what
    // the `levels` term narrows (the headline difference from function-scoped Python).
    let src = "fn f(flag: bool) {\n    let x = compute();\n    if flag {\n        sink(x);\n    }\n}\n";
    assert_eq!(scope_of(src), 10);
}

#[test]
fn let_not_narrowed_into_loop() {
    // loop-carried / invariant state legitimately lives outside the loop body.
    let src = "fn f() {\n    let acc = seed();\n    for i in it() {\n        push(acc, i);\n    }\n}\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn free_function_call_below_is_declare_order() {
    let src = "fn caller() { helper(); }\nfn helper() {}\n";
    assert_eq!(warnings_of(src), 1);
}

#[test]
fn impl_self_member_resolves_and_orders() {
    // `self.b()` resolves to the method in the same impl; `b` declared below `a` → declare-order.
    let src = "struct S;\nimpl S {\n    fn a(&self) { self.b(); }\n    fn b(&self) {}\n}\n";
    assert_eq!(warnings_of(src), 1);
}

#[test]
fn impl_method_calling_earlier_method_is_clean() {
    let src = "struct S;\nimpl S {\n    fn b(&self) {}\n    fn a(&self) { self.b(); }\n}\n";
    assert_eq!(warnings_of(src), 0);
}

#[test]
fn self_field_access_is_not_a_method_reference() {
    // `self.data` is a FIELD read (a method would be `self.data()`), so it must NOT resolve to the
    // same-named `data()` accessor below it — no phantom declare-order warning.
    let src = "struct S { data: i32 }\nimpl S {\n    fn size(&self) -> i32 { self.data }\n    fn data(&self) -> i32 { self.data }\n}\n";
    assert_eq!(warnings_of(src), 0);
}

#[test]
fn const_is_data_used_once_not_narrowed() {
    let src = "const CONFIG: i32 = 1;\nfn f() -> i32 { CONFIG }\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn const_referenced_below_is_declare_order() {
    let src = "fn f() -> i32 { TIMEOUT }\nconst TIMEOUT: i32 = 30;\n";
    assert_eq!(warnings_of(src), 1);
}

#[test]
fn const_inside_mod_is_data_not_a_narrowable_local() {
    // a `mod` is a definition container (like a C++ namespace): a `const` declared in it is top-level
    // DATA, not a local of the mod — so a single deep use must not narrow it.
    let src = "mod cfg {\n    const K: i32 = 60;\n    fn f(flag: bool) -> i32 {\n        if flag { return K; }\n        0\n    }\n}\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn match_arms_do_not_cross_wedge() {
    // `a` and `b` live in mutually-exclusive arms; `shared` is read by both. No arm binding is on
    // the other arm's execution path, so nothing wedges — the cousin-block fix in the core.
    let src = "fn f(x: E) {\n    let shared = init();\n    match x {\n        E::A(a) => { use2(shared, a); }\n        E::B(b) => { use2(shared, b); }\n    }\n}\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn parse_error_is_reported_not_panicked() {
    // a malformed file yields a ParseError finding (and no scope-debt), never a panic.
    let src = "fn f( {\n";
    assert_eq!(scope_of(src), 0);
}
