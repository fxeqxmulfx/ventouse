//! M4 — scope-debt (P5 locality): levels (narrowest block) + wedges (unrelated definitions
//! between a binding and its dependencies / its first use) for Python variables. Functions/classes
//! get gap-to-deps only (declaring code is free of nesting). Default SCOPE_LEVEL=10 per unit.

use ventouse::core::Reason;
use ventouse::lang::python::{analyze_source, scope_of};

fn misplaced_count(src: &str) -> usize {
    analyze_source(src, "t.py", &Default::default())
        .iter()
        .filter(|f| matches!(f.reason, Reason::Misplaced(_)))
        .count()
}

#[test]
fn s1_declared_too_high() {
    // x at function top, used only inside `if` -> levels_excess 1 -> 10 (no unrelated def wedged;
    // the `if` header is not a definition, so wedges = 0)
    let src = "def f(flag):\n    x = 1\n    if flag:\n        print(x)\n";
    assert_eq!(scope_of(src), 10);
}

#[test]
fn s2_tight_narrowest_block() {
    let src = "def f(flag):\n    if flag:\n        x = 1\n        print(x)\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn bare_attribute_annotation_is_not_a_data_definition() {
    // `val: int` (no value) DECLARES an instance attribute (a field), not a class datum — so a method
    // reading `self.val` must NOT see it as a definition (no edge, no placement debt). Modern typed
    // Python annotates attributes at the class top; treating them as data flagged every method using
    // one as "far from its declaration" (caught dogfooding `requests`).
    let bare = "class C:\n    val: int\n    def a(self): pass\n    def b(self): pass\n    def use(self):\n        return self.val\n";
    assert_eq!(misplaced_count(bare), 0);
    // a VALUED class attribute DOES bind (a real datum), so `use` is placed far from it with `a`/`b`
    // wedged between — confirming the difference is specifically the missing value.
    let valued = "class C:\n    val = 0\n    def a(self): pass\n    def b(self): pass\n    def use(self):\n        return self.val\n";
    assert_eq!(misplaced_count(valued), 1);
}

#[test]
fn s3_declared_too_early_wedge() {
    // x is used only in `return x + s`; `r` is wedged between x and that use and is NOT co-used
    // there (r's own first use is `s = r * 2`) -> x misplaced -> 10. `s` is co-used at the return
    // -> exempt; r/s sit right after their deps -> 0.
    let src = "def f(p, q):\n    x = 0\n    r = p + q\n    s = r * 2\n    return x + s\n";
    assert_eq!(scope_of(src), 10);
}

#[test]
fn s6_unused_not_scored() {
    let src = "def f():\n    x = 1\n    return 0\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn s7_params_excluded() {
    let src = "def f(a, b):\n    return a + b\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn s8_use_in_two_branches_lca() {
    // x used in both branches -> min_block = function body (LCA) -> levels 0; no unrelated wedge -> 0
    let src = "def f(flag):\n    x = 1\n    if flag:\n        print(x)\n    else:\n        print(x)\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn ec12_levels_clamp_declared_deeper_than_used() {
    // x bound in `if`, used after -> min_block shallower than decl -> levels clamped 0; gap 0
    let src = "def f(c):\n    if c:\n        x = 1\n    print(x)\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn ec13_walrus_tight() {
    // walrus binds an un-narrowable name, used right there -> 0
    let src = "def f(data):\n    if (n := len(data)) > 0:\n        return n\n    return 0\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn sb6_first_binding_rule() {
    // decl_block = function body (first binding at top), not the `if`; uses span both -> levels 0;
    // nothing unrelated wedged between x and its use (the `if` / `x=1` are not unrelated defs) -> 0
    let src = "def f(c):\n    x = 0\n    if c:\n        x = 1\n    return x\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn for_target_not_penalized() {
    // a for-target is un-narrowable -> no levels penalty (used in the loop body)
    let src = "def f(xs):\n    for i in xs:\n        print(i)\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn loop_carried_state_not_penalized() {
    // `seen` is initialized before the loop and used only inside it — it MUST live outside the
    // loop (loop-carried). levels must NOT penalize "declared above the loop". -> 0
    let src = "def has_dup(data):\n    seen = set()\n    for x in data:\n        if x in seen:\n            return True\n        seen.add(x)\n    return False\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn loop_invariant_not_penalized() {
    // `factor` is a loop-invariant used inside the loop — hoisting it OUT is good, so declaring it
    // before the loop is not debt. -> 0
    let src = "def scale(items):\n    factor = 2\n    out = []\n    for x in items:\n        out.append(x * factor)\n    return out\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn binding_right_before_a_loop_is_not_wedged_by_the_loop_itself() {
    // `v` sits immediately before the loop and is used inside it (NOT co-used with anything). The
    // loop VARIABLE `x` and in-loop `noise` are inside the loop, so they are not on the path `v`
    // could move along — `v` is already maximally close. -> 0. (Regression: the for-target used to
    // be bound in the enclosing block, phantom-wedging a binding sitting just before the loop.)
    let src = "def f(xs):\n    v = setup()\n    for x in xs:\n        noise = work()\n        use(x, v)\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn pre_loop_junk_still_wedges_a_binding_that_could_move_closer() {
    // `v` with two unrelated definitions between it and the loop COULD move down to just before the
    // loop -> 2 wedges. The loop is the floor for narrowing, not for wedges on THIS side.
    let src = "def f(xs):\n    v = setup()\n    j1 = a()\n    j2 = b()\n    for x in xs:\n        use(x, v)\n";
    assert_eq!(scope_of(src), 20);
}

// --- cross-scope definition placement is NOT penalized (S4/S5) --------------------------
// Scope-debt is within-scope only: a module-level def/import used inside a function is fine
// (nesting it into its caller would kill reuse, and the metric is minimized — we never push the
// agent to do that). Only within-scope placement (SB5) and local variables are scored.

#[test]
fn s4_module_helper_cross_scope_not_penalized() {
    // helper at module level, used inside main (a different scope) -> cross-scope -> 0
    let src = "def helper(x):\n    return x + 1\n\ndef main():\n    return helper(5)\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn s5_import_cross_scope_not_penalized() {
    let src = "import math\n\ndef area(r):\n    return math.pi * r * r\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn sb5_nested_function_declaration_is_free() {
    // declaring a function is free (no nesting penalty) — a nested helper with no dependencies
    // costs nothing regardless of where in its enclosing function it sits.
    let src = "def f(flag):\n    def helper():\n        return 1\n    if flag:\n        return helper()\n    return 0\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn functional_decomposition_is_free() {
    // a flat chain of single-use pure helpers (good-code shape) -> 0 scope-debt
    let src = "def helper(x):\n    return x + 1\n\ndef a():\n    return helper(1)\n\ndef b():\n    return helper(2)\n";
    assert_eq!(scope_of(src), 0);
}

// --- introduced binding kinds at their natural block (SB1–SB4): no false penalty ---------

#[test]
fn sb2_with_as_target_no_penalty() {
    let src = "def f(p):\n    with open(p) as h:\n        return h.read()\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn sb3_except_as_target_no_penalty() {
    let src = "def f(x):\n    try:\n        return x\n    except KeyError as e:\n        return repr(e)\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn sb4_comprehension_target_no_penalty() {
    let src = "def f(xs):\n    return [j for j in xs]\n";
    assert_eq!(scope_of(src), 0);
}

// --- gap-to-deps (P5 locality): declared right after dependencies ------------------------

#[test]
fn fn_gap_to_deps_unrelated_wedge() {
    // b depends on a; junk (unrelated) is wedged between a and b -> b misplaced -> 10
    let src = "def a():\n    return 1\n\ndef junk():\n    return 2\n\ndef b():\n    return a()\n";
    assert_eq!(scope_of(src), 10);
    assert_eq!(misplaced_count(src), 1);
}

#[test]
fn fn_shared_dependency_is_not_a_wedge() {
    // a and b both call helper -> same cluster, order free -> no misplacement
    let src = "def helper(x):\n    return x + 1\n\ndef a():\n    return helper(1)\n\ndef b():\n    return helper(2)\n";
    assert_eq!(misplaced_count(src), 0);
    assert_eq!(scope_of(src), 0);
}

#[test]
fn var_gap_to_deps_unrelated_wedge() {
    // junk is wedged between a and b: it disrupts BOTH b's dep-cluster (a→b) and a's live range
    // (a→its use in b). Two misplacements — `junk` is doubly out of place. (`junk` is co-used with
    // b at the return, so it is exempt from b's use side.)
    let src = "def f():\n    a = g()\n    junk = h()\n    b = a + 1\n    return b + junk\n";
    assert_eq!(misplaced_count(src), 2);
}

#[test]
fn var_shared_dependency_is_not_a_wedge() {
    // c and b both computed from a -> same cluster -> no misplacement
    let src = "def f():\n    a = g()\n    c = a + 1\n    b = a + 2\n    return b + c\n";
    assert_eq!(misplaced_count(src), 0);
}

// --- class members are not lexical locals (references model regressions) ----------------

#[test]
fn class_attr_via_self_is_not_narrowed() {
    // `self.x` references the class attribute but is NOT a lexical use — you can't move a class
    // attribute into the method that reads it. So no scope-debt.
    let src = "class C:\n    x = 1\n    def m(self):\n        return self.x\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn class_attr_via_self_in_two_methods_is_clean() {
    let src = "class C:\n    cache = {}\n    def a(self):\n        return self.cache\n    def b(self):\n        return self.cache\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn bare_name_in_method_does_not_see_class_scope() {
    // a method does not see class-body names lexically (Python scoping): bare `x` is NOT the class
    // attribute `C.x`, so `C.x` is unused and accrues no scope-debt.
    let src = "class C:\n    x = 1\n    def m(self):\n        return x\n";
    assert_eq!(scope_of(src), 0);
}

// --- global / nonlocal state, shadowing, data-narrowing (probe regressions) -------------

fn scope_entities(src: &str) -> Vec<String> {
    analyze_source(src, "t.py", &Default::default())
        .into_iter()
        .filter(|f| f.category == ventouse::core::Category::ScopeDebt)
        .map(|f| f.entity)
        .collect()
}

#[test]
fn global_state_written_in_one_function_is_pinned() {
    // a module variable mutated via `global` is intentional shared state — never narrowed.
    let src = "COUNT = 0\ndef inc():\n    global COUNT\n    COUNT = COUNT + 1\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn global_state_across_two_functions_is_clean() {
    let src = "_s = 0\ndef set_it():\n    global _s\n    _s = 1\ndef get_it():\n    return _s\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn module_const_is_data_not_narrowed() {
    // a module-level constant is a DATA definition, never narrowed (only function-LOCAL variables
    // narrow). It is placed/ordered like a definition instead.
    let src = "CONFIG = 1\ndef outer():\n    def inner():\n        return CONFIG\n    return inner\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn function_near_shared_data_wedge() {
    // TIMEOUT is read by a and b; `junk` wedged between TIMEOUT and them -> a and b each get one
    // gap-to-deps wedge (a function should sit next to the data it reads).
    let src = "TIMEOUT = 30\ndef junk():\n    return 1\ndef a():\n    return get(TIMEOUT)\ndef b():\n    return get(TIMEOUT)\n";
    assert_eq!(misplaced_count(src), 2);
}

#[test]
fn class_attr_is_data_not_narrowed() {
    let src = "class C:\n    MAX = 100\n    def m(self):\n        return self.MAX\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn bare_name_in_method_shadowed_by_class_attr_resolves_to_module() {
    // `return TIMEOUT` in a method is the MODULE TIMEOUT — so C.TIMEOUT is unused, never scored.
    let src = "TIMEOUT = 30\nclass C:\n    TIMEOUT = 60\n    def m(self):\n        return TIMEOUT\n";
    assert!(!scope_entities(src).contains(&"C.TIMEOUT".to_string()));
}

#[test]
fn module_var_used_in_two_functions_is_clean() {
    let src = "SHARED = 1\ndef a():\n    return SHARED\ndef b():\n    return SHARED\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn placement_counts_reference_not_only_call() {
    // b references a by NAME (not a call); junk is wedged between a and b -> 1 misplacement.
    // c also references a (shares the dependency) so it is not a wedge.
    let src = "def a():\n    return 1\ndef c():\n    return a\ndef junk():\n    return 99\ndef b():\n    return a\n";
    assert_eq!(misplaced_count(src), 1);
}

#[test]
fn value_used_only_in_comprehension_narrows_toward_it() {
    // A comprehension is its own (Py3) scope; a value read only inside it is captured, so it narrows
    // toward the comprehension — exactly like a value read only in a nested function/lambda.
    let src = "def f(xs):\n    total = compute()\n    return [total + i for i in xs]\n";
    assert_eq!(scope_of(src), 10);
}

#[test]
fn global_augassign_is_pinned() {
    let src = "C = 0\ndef bump():\n    global C\n    C += 1\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn nonlocal_state_across_functions_is_pinned() {
    let src = "def outer():\n    n = 0\n    def a():\n        nonlocal n\n        n += 1\n    def b():\n        return n\n    return a, b\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn async_def_narrows_like_def() {
    let src = "async def f(c):\n    x = 1\n    if c:\n        return x\n";
    assert_eq!(scope_of(src), 10);
}

#[test]
fn closure_var_read_only_in_inner_narrows() {
    // data-narrowing (C2) applies to closures too: a plain local read only in a nested function
    // narrows toward it (only `nonlocal`/`global` shared state is pinned).
    let src = "def outer():\n    x = compute()\n    def inner():\n        return x\n    return inner\n";
    assert!(scope_of(src) > 0);
}

#[test]
fn walrus_at_module_level_is_clean() {
    let src = "if (n := compute()) > 0:\n    print(n)\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn value_used_only_in_fstring_is_missed_known_l2_hole() {
    // KNOWN LIMITATION (L2): f-string interpolations are not descended into.
    let src = "def f():\n    name = compute()\n    return f\"hi {name}\"\n";
    assert_eq!(scope_of(src), 0);
}

#[test]
fn match_case_body_narrows_a_value_used_only_there() {
    // `match`/`case` are lowered: each case is a block, so a value read only inside one narrows
    // toward it (like any conditional branch).
    let src = "def f(v):\n    x = 1\n    match v:\n        case 1:\n            return x\n";
    assert_eq!(scope_of(src), 10);
}

#[test]
fn data_computed_from_data_is_placed() {
    // TIMEOUT is computed from DEFAULT; JUNK is wedged between them -> TIMEOUT accrues one
    // gap-to-deps wedge (a constant should sit next to the constants it is computed from).
    let src = "DEFAULT = 10\nJUNK = 99\nTIMEOUT = DEFAULT * 2\n";
    assert_eq!(misplaced_count(src), 1);
}

#[test]
fn suggests_reordering_a_local_declared_far_from_its_use() {
    // q is bound at line 2 but first used only at the return (line 6); a, b, c are three unrelated
    // locals wedged between -> a ReorderBinding suggestion to push `q` down to its use.
    let src = "def f():\n    q = compute()\n    a = one()\n    b = two()\n    c = three()\n    return use(q)\n";
    let suggestions: Vec<(String, Reason)> = analyze_source(src, "t.py", &Default::default())
        .into_iter()
        .filter(|f| matches!(f.reason, Reason::ReorderBinding { .. }))
        .map(|f| (f.entity, f.reason))
        .collect();
    assert_eq!(suggestions, [("f.q".to_string(), Reason::ReorderBinding { first_use: 6, wedged: 3 })]);
}

#[test]
fn no_reorder_suggestion_for_a_loop_carried_accumulator() {
    // `total` is bound before the loop and first USED inside it; a/b/c wedge it (real debt), but
    // pushing `total = 0` down into the loop would reset it every iteration — so NO ReorderBinding.
    let src = "def f(rows):\n    total = 0\n    a = one()\n    b = two()\n    c = three()\n    log(a, b, c)\n    for r in rows:\n        total += r\n    return total\n";
    let reorders = analyze_source(src, "t.py", &Default::default())
        .into_iter()
        .filter(|f| matches!(f.reason, Reason::ReorderBinding { .. }))
        .count();
    assert_eq!(reorders, 0, "a loop-carried accumulator cannot move down into the loop");
    // the wedge debt still stands (it is what feeds the CrowdedScope signal).
    assert!(misplaced_count(src) >= 1);
}
