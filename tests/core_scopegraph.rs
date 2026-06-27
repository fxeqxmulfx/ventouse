//! Language-agnostic core: `ScopeGraph::score` tested DIRECTLY via the public builder API (no
//! parser, no Python). Exercises levels (narrow/loop-aware/clamp/LCA), wedges, edges, entities,
//! declare-before-use, member resolution, the class-scope lexical skip, and nonlocal pinning.
//!
//! Scope-debt (levels/wedges) is for FUNCTION-LOCAL variables; module-/class-level values are DATA
//! definitions (placed/ordered, not narrowed). So the value tests build a function scope `f`.

use ventouse::core::raw::{RawEntity, RawKind, RawScope};
use ventouse::core::scopegraph::{BindKind, ScopeGraph};

fn debt(g: ScopeGraph) -> Vec<RawScope> {
    let mut d = g.score().debt;
    d.sort_by(|a, b| a.entity.cmp(&b.entity));
    d
}

/// A graph with a module + one function `f`; returns `(graph, f_scope, f_entry_block)`. Values bound
/// in `f` are local variables (which narrow); the entity qualname prefix is `f`.
fn in_function() -> (ScopeGraph, usize, usize) {
    let mut g = ScopeGraph::new();
    let m = g.new_scope(None, "<module>".into(), true, false, false, vec![]);
    let f = g.new_scope(Some(m), "f".into(), false, false, false, vec![]);
    let fb = g.new_block(g.module_block(), false);
    (g, f, fb)
}

// --- levels -----------------------------------------------------------------------------

#[test]
fn levels_narrow_into_conditional() {
    // a local declared at the function body, used only one block deeper -> 1 level too high.
    let (mut g, f, fb) = in_function();
    g.bind(f, "x", BindKind::Value, fb, 1, false, false);
    let inner = g.new_block(fb, false);
    g.add_use("x", f, inner, 3, false, None);
    assert_eq!(
        debt(g),
        [RawScope { entity: "f.x".into(), name: "x".into(), line: 1, levels_excess: 1, wedges: 0, use_wedges: 0, first_use: 3, independent: true }]
    );
}

#[test]
fn levels_loop_aware_caps_above_loop() {
    let (mut g, f, fb) = in_function();
    g.bind(f, "acc", BindKind::Value, fb, 1, false, false);
    let loop_body = g.new_block(fb, /*is_loop*/ true);
    g.add_use("acc", f, loop_body, 3, false, None);
    assert!(debt(g).is_empty());
}

#[test]
fn levels_clamp_when_use_shallower_than_decl() {
    let (mut g, f, fb) = in_function();
    let inner = g.new_block(fb, false);
    g.bind(f, "x", BindKind::Value, inner, 2, false, false); // declared in the inner block
    g.add_use("x", f, fb, 4, false, None); // used at the (shallower) function body
    assert!(debt(g).is_empty());
}

#[test]
fn levels_lca_of_two_branches_is_zero() {
    let (mut g, f, fb) = in_function();
    g.bind(f, "x", BindKind::Value, fb, 1, false, false);
    let a = g.new_block(fb, false);
    let b = g.new_block(fb, false);
    g.add_use("x", f, a, 3, false, None);
    g.add_use("x", f, b, 5, false, None);
    assert!(debt(g).is_empty());
}

#[test]
fn lca_with_asymmetric_use_depths() {
    // used at depth 1 and at depth 3 (a descendant of the depth-1 block) -> LCA is depth 1.
    let (mut g, f, fb) = in_function();
    g.bind(f, "x", BindKind::Value, fb, 1, false, false);
    let a = g.new_block(fb, false);
    let a2 = g.new_block(a, false);
    let b = g.new_block(a2, false);
    g.add_use("x", f, a, 3, false, None);
    g.add_use("x", f, b, 5, false, None);
    assert_eq!(debt(g)[0].levels_excess, 1);
}

#[test]
fn intro_binding_has_no_levels() {
    let (mut g, f, fb) = in_function();
    let inner = g.new_block(fb, false);
    g.bind(f, "i", BindKind::Value, fb, 1, /*intro*/ true, false);
    g.add_use("i", f, inner, 3, false, None);
    assert!(debt(g).is_empty());
}

// --- wedges -----------------------------------------------------------------------------

#[test]
fn wedge_unrelated_definition_between_value_and_its_dep() {
    let (mut g, f, fb) = in_function();
    g.bind(f, "a", BindKind::Value, fb, 1, false, false);
    g.bind(f, "junk", BindKind::Value, fb, 2, false, false);
    g.bind(f, "b", BindKind::Value, fb, 3, false, false);
    g.set_deps(f, "b", &["a".into()]);
    g.add_use("b", f, fb, 9, false, None);
    g.add_use("a", f, fb, 3, false, None);
    g.add_use("junk", f, fb, 9, false, None);
    let d = debt(g);
    assert_eq!(d.iter().find(|s| s.name == "b").expect("b scored").wedges, 1);
}

#[test]
fn cousin_block_binding_is_not_a_wedge() {
    // `b` (in block B) depends on `a` (function body); `junk` sits textually between them but in a
    // COUSIN block A (a different, mutually-exclusive branch) — not on b's path, so it never wedges.
    let (mut g, f, fb) = in_function();
    g.bind(f, "a", BindKind::Value, fb, 1, false, false);
    let blk_a = g.new_block(fb, false);
    g.bind(f, "junk", BindKind::Value, blk_a, 2, false, false);
    let blk_b = g.new_block(fb, false);
    g.bind(f, "b", BindKind::Value, blk_b, 3, false, false);
    g.set_deps(f, "b", &["a".into()]);
    g.add_use("a", f, blk_b, 4, false, None);
    g.add_use("b", f, blk_b, 4, false, None);
    g.add_use("junk", f, blk_a, 2, false, None);
    let d = debt(g);
    assert!(d.iter().find(|s| s.name == "b").map(|s| s.wedges).unwrap_or(0) == 0, "{d:?}");
}

#[test]
fn shared_dependency_is_not_a_wedge() {
    let (mut g, f, fb) = in_function();
    g.bind(f, "a", BindKind::Value, fb, 1, false, false);
    g.bind(f, "c", BindKind::Value, fb, 2, false, false);
    g.bind(f, "b", BindKind::Value, fb, 3, false, false);
    g.set_deps(f, "c", &["a".into()]);
    g.set_deps(f, "b", &["a".into()]);
    g.add_use("a", f, fb, 2, false, None);
    g.add_use("b", f, fb, 9, false, None);
    g.add_use("c", f, fb, 9, false, None);
    assert!(debt(g).iter().all(|s| s.wedges == 0));
}

#[test]
fn first_binding_keeps_the_earliest_when_rebound_out_of_order() {
    let (mut g, f, fb) = in_function();
    g.bind(f, "x", BindKind::Value, fb, 5, false, false);
    g.bind(f, "x", BindKind::Value, fb, 2, false, false);
    let inner = g.new_block(fb, false);
    g.add_use("x", f, inner, 9, false, None);
    assert_eq!(debt(g)[0].line, 2);
}

#[test]
fn declare_before_use_for_a_value() {
    let (mut g, f, fb) = in_function();
    g.bind(f, "x", BindKind::Value, fb, 5, false, false); // bound at line 5
    g.add_use("x", f, fb, 2, false, None); // used at line 2 (before)
    assert_eq!(g.score().decl_warnings, [("f.x".to_string(), 2u32)]);
}

#[test]
fn nonlocal_reference_pins_a_closure_local() {
    // `n` is a local of `outer`; `inner` references it via `nonlocal` -> shared state, not narrowed.
    let mut g = ScopeGraph::new();
    let m = g.new_scope(None, "<module>".into(), true, false, false, vec![]);
    let outer = g.new_scope(Some(m), "outer".into(), false, false, false, vec![]);
    let outer_b = g.new_block(g.module_block(), false);
    g.bind(outer, "n", BindKind::Value, outer_b, 1, false, false);
    let inner = g.new_scope(Some(outer), "outer.inner".into(), false, false, false, vec!["n".into()]);
    let inner_b = g.new_block(outer_b, false);
    g.add_use("n", inner, inner_b, 3, false, None);
    assert!(debt(g).is_empty(), "a nonlocal reference pins the closure local");
}

// --- entities + edges (definitions: functions AND module/class data) --------------------

#[test]
fn decl_binding_becomes_an_entity_and_reference_becomes_an_edge() {
    let mut g = ScopeGraph::new();
    let m = g.new_scope(None, "<module>".into(), true, false, false, vec![]);
    let mb = g.module_block();
    g.bind(m, "g", BindKind::Decl, mb, 5, false, false);
    g.add_use("g", m, mb, 2, false, None);
    let out = g.score();
    assert_eq!(out.entities, [RawEntity { qualname: "g".into(), kind: RawKind::Function, line: 5 }]);
    assert_eq!(out.edges, [("<module>".to_string(), "g".to_string())]);
}

#[test]
fn module_level_value_is_a_data_definition_not_narrowed() {
    // a module constant read by one function is DATA (an entity), not a narrowed value.
    let mut g = ScopeGraph::new();
    let m = g.new_scope(None, "<module>".into(), true, false, false, vec![]);
    let mb = g.module_block();
    g.bind(m, "CONFIG", BindKind::Value, mb, 1, false, false);
    g.bind(m, "f", BindKind::Decl, mb, 2, false, false); // a real `def f` binds its name + opens a scope
    let f = g.new_scope(Some(m), "f".into(), false, false, false, vec![]);
    let fb = g.new_block(mb, false);
    g.add_use("CONFIG", f, fb, 3, false, None);
    let out = g.score();
    assert!(out.debt.is_empty(), "module data must not narrow");
    assert!(out.entities.contains(&RawEntity { qualname: "CONFIG".into(), kind: RawKind::Data, line: 1 }));
    assert_eq!(out.edges, [("f".to_string(), "CONFIG".to_string())]); // f depends on the data
}

#[test]
fn class_scope_marks_methods_and_classes() {
    let mut g = ScopeGraph::new();
    let m = g.new_scope(None, "<module>".into(), true, false, false, vec![]);
    let mb = g.module_block();
    g.bind(m, "C", BindKind::Decl, mb, 1, false, /*is_class_def*/ true);
    let c = g.new_scope(Some(m), "C".into(), false, /*is_class*/ true, false, vec![]);
    let c_entry = g.new_block(mb, false);
    g.bind(c, "method", BindKind::Decl, c_entry, 2, false, false);
    g.bind(c, "ATTR", BindKind::Value, c_entry, 3, false, false); // class attribute = data def
    let mut ents = g.score().entities;
    ents.sort_by(|a, b| a.qualname.cmp(&b.qualname));
    assert_eq!(
        ents,
        [
            RawEntity { qualname: "C".into(), kind: RawKind::Class, line: 1 },
            RawEntity { qualname: "C.ATTR".into(), kind: RawKind::Data, line: 3 },
            RawEntity { qualname: "C.method".into(), kind: RawKind::Method, line: 2 },
        ]
    );
}

// --- member resolution (self.X) ---------------------------------------------------------

#[test]
fn member_reference_is_an_edge_but_not_a_value_use() {
    // a class attribute read via a member reference is an edge to it (data), never narrowed.
    let mut g = ScopeGraph::new();
    let m = g.new_scope(None, "<module>".into(), true, false, false, vec![]);
    let mb = g.module_block();
    g.bind(m, "C", BindKind::Decl, mb, 1, false, true);
    let c = g.new_scope(Some(m), "C".into(), false, true, false, vec![]);
    let c_entry = g.new_block(mb, false);
    g.bind(c, "attr", BindKind::Value, c_entry, 2, false, false);
    g.bind(c, "m", BindKind::Decl, c_entry, 3, false, false); // a real method binds its name + opens a scope
    let method = g.new_scope(Some(c), "C.m".into(), false, false, false, vec![]);
    let m_entry = g.new_block(c_entry, false);
    g.add_use("attr", method, m_entry, 4, /*member*/ true, None);
    let out = g.score();
    assert!(out.debt.iter().all(|s| s.name != "attr"), "{:?}", out.debt);
    assert!(out.edges.contains(&("C.m".to_string(), "C.attr".to_string())));
}

#[test]
fn member_reference_with_no_enclosing_class_is_dropped() {
    let mut g = ScopeGraph::new();
    let m = g.new_scope(None, "<module>".into(), true, false, false, vec![]);
    let mb = g.module_block();
    g.bind(m, "x", BindKind::Decl, mb, 1, false, false);
    g.add_use("x", m, mb, 2, /*member*/ true, None);
    assert!(g.score().edges.is_empty());
}
