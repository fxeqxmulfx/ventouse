//! Language-agnostic core: `DefGraph` + `placement` (gap-to-deps) + `declorder` (declare-before-use)
//! tested DIRECTLY on synthetic `(entities, edges)` — no parser, no Python.

use ventouse::core::declorder;
use ventouse::core::defgraph::DefGraph;
use ventouse::core::finding::Category;
use ventouse::core::placement;
use ventouse::core::raw::{RawEntity, RawKind};

fn ent(qual: &str, line: u32) -> RawEntity {
    RawEntity { qualname: qual.into(), kind: RawKind::Function, line }
}

/// A class member (`Method`) — the kind that accessor-pinning and the scope-aware extract rule key on.
fn method(qual: &str, line: u32) -> RawEntity {
    RawEntity { qualname: qual.into(), kind: RawKind::Method, line }
}

fn edge(a: &str, b: &str) -> (String, String) {
    (a.into(), b.into())
}

fn graph(entities: &[RawEntity], edges: &[(String, String)]) -> DefGraph {
    DefGraph::build("t.py", entities, edges)
}

// --- DefGraph::build --------------------------------------------------------------------

#[test]
fn build_resolves_edges_and_drops_unknown() {
    let dg = graph(&[ent("a", 1), ent("b", 2)], &[edge("a", "b"), edge("a", "missing")]);
    assert_eq!(dg.defs.len(), 2);
    let a = dg.defs.iter().find(|d| d.qualname == "a").unwrap();
    assert_eq!(a.calls.len(), 1); // a->b kept, a->missing dropped
}

// --- placement (gap-to-deps) ------------------------------------------------------------

#[test]
fn placement_counts_unrelated_wedge() {
    // a(1), junk(2), b(3); b depends on a -> junk is one wedge on b.
    let dg = graph(&[ent("a", 1), ent("junk", 2), ent("b", 3)], &[edge("b", "a")]);
    let w = placement::wedges(&dg, &Weights::default());
    let b_idx = dg.defs.iter().position(|d| d.qualname == "b").unwrap();
    assert_eq!(w, [(b_idx, 1)]);
}

#[test]
fn placement_shared_dep_is_not_a_wedge() {
    // c and b both depend on a -> c is not a wedge for b.
    let dg = graph(&[ent("a", 1), ent("c", 2), ent("b", 3)], &[edge("c", "a"), edge("b", "a")]);
    assert!(placement::wedges(&dg, &Weights::default()).is_empty());
}

#[test]
fn placement_no_dependency_above_is_free() {
    // b depends on a, but a is BELOW b -> no dependency span -> free to sit anywhere.
    let dg = graph(&[ent("b", 1), ent("junk", 2), ent("a", 3)], &[edge("b", "a")]);
    assert!(placement::wedges(&dg, &Weights::default()).is_empty());
}

// --- declorder (declare-before-use) -----------------------------------------------------

fn warn_entities(dg: &DefGraph) -> Vec<String> {
    declorder::warnings(dg, ventouse::core::DeclOrder::BottomUp)
        .into_iter()
        .filter(|f| f.category == Category::DeclBeforeUse)
        .map(|f| f.entity)
        .collect()
}

#[test]
fn declorder_forward_reference_warns() {
    // f references g defined below -> warning on f.
    let dg = graph(&[ent("f", 1), ent("g", 3)], &[edge("f", "g")]);
    assert_eq!(warn_entities(&dg), ["f"]);
}

#[test]
fn declorder_reason_names_the_callee() {
    // The warning is on the referrer `f`, but the reason names the callee `g` (declared below) —
    // so the wording stays true ("f references g") instead of falsely saying f is used early.
    let dg = graph(&[ent("f", 1), ent("g", 3)], &[edge("f", "g")]);
    let w = declorder::warnings(&dg, ventouse::core::DeclOrder::BottomUp);
    assert_eq!(w.len(), 1);
    assert_eq!(w[0].entity, "f");
    assert_eq!(w[0].reason, ventouse::core::model::Reason::ForwardRef { callee: "g".into(), below: true });
}

#[test]
fn declorder_callee_above_is_clean() {
    let dg = graph(&[ent("g", 1), ent("f", 3)], &[edge("f", "g")]);
    assert!(warn_entities(&dg).is_empty());
}

#[test]
fn declorder_cycle_is_exempt() {
    // a->b and b->a (mutual) -> unavoidable cycle -> no warning.
    let dg = graph(&[ent("a", 1), ent("b", 3)], &[edge("a", "b"), edge("b", "a")]);
    assert!(warn_entities(&dg).is_empty());
}

#[test]
fn declorder_self_reference_is_exempt() {
    let dg = graph(&[ent("f", 1)], &[edge("f", "f")]);
    assert!(warn_entities(&dg).is_empty());
}

#[test]
fn declorder_different_scope_is_not_a_forward_ref() {
    // "C.m" references "g": different enclosing scope ("C" vs "") -> filtered, no warning.
    let entities = [
        RawEntity { qualname: "C.m".into(), kind: RawKind::Method, line: 1 },
        ent("g", 3),
    ];
    let dg = graph(&entities, &[edge("C.m", "g")]);
    assert!(warn_entities(&dg).is_empty());
}

#[test]
fn declorder_dedups_repeated_reference() {
    // f references g (below) twice -> a single warning.
    let dg = graph(&[ent("f", 1), ent("g", 3)], &[edge("f", "g"), edge("f", "g")]);
    assert_eq!(warn_entities(&dg).len(), 1);
}

#[test]
fn declorder_reaches_handles_revisited_nodes() {
    // a -> b (forward); b's subgraph is a diamond (b->c, b->d, c->e, d->e) with no path back to a,
    // so `reaches(b, a)` revisits `e` (exercises the BFS visited-set) and still returns false.
    let entities = [ent("a", 1), ent("b", 2), ent("c", 3), ent("d", 4), ent("e", 5)];
    let edges = [edge("a", "b"), edge("b", "c"), edge("b", "d"), edge("c", "e"), edge("d", "e")];
    let dg = graph(&entities, &edges);
    // a references b below and the cycle-check does not find a back-edge -> a is warned.
    assert!(warn_entities(&dg).contains(&"a".to_string()));
}

#[test]
fn placement_tight_tree_of_shared_callers_is_zero() {
    // f5 is a shared callee; f1..f4 all call it, packed tight right below it. Every sibling between
    // a caller and f5 also calls f5 (shares the dependency) -> no wedges anywhere.
    let dg = graph(
        &[ent("f5", 1), ent("f1", 2), ent("f2", 3), ent("f3", 4), ent("f4", 5)],
        &[edge("f1", "f5"), edge("f2", "f5"), edge("f3", "f5"), edge("f4", "f5")],
    );
    assert!(placement::wedges(&dg, &Weights::default()).is_empty(), "{:?}", placement::wedges(&dg, &Weights::default()));
}

#[test]
fn placement_foreign_function_wedged_into_a_tight_tree_raises_score() {
    // g (from another tree: g -> g9, declared below) is wedged between f5 and the f3/f4 that depend
    // on it. g shares no dependency with them -> a wedge for each caller below it.
    let dg = graph(
        &[ent("f5", 1), ent("f1", 2), ent("f2", 3), ent("g", 4), ent("f3", 5), ent("f4", 6), ent("g9", 7)],
        &[edge("f1", "f5"), edge("f2", "f5"), edge("f3", "f5"), edge("f4", "f5"), edge("g", "g9")],
    );
    let w: std::collections::HashMap<usize, u32> = placement::wedges(&dg, &Weights::default()).into_iter().collect();
    let idx = |q: &str| dg.defs.iter().position(|d| d.qualname == q).unwrap();
    assert_eq!(w.get(&idx("f3")).copied(), Some(1), "g wedged between f5 and f3");
    assert_eq!(w.get(&idx("f4")).copied(), Some(1), "g wedged between f5 and f4");
    assert_eq!(w.get(&idx("f1")), None);
    assert_eq!(w.get(&idx("f2")), None);
}

// --- accessor pinning (a high-fan-in class-member leaf is de-facto data) -----------------

#[test]
fn high_fanin_leaf_accessor_is_pinned_no_debt_for_callers() {
    // C.acc is a leaf method used by 3 siblings declared far below, with junk between. As a de-facto
    // field accessor it is PINNED: its callers do not owe gap-to-deps debt for being far from it
    // (otherwise a couple of shared accessors inflate the placement debt of a whole class).
    let dg = graph(
        &[
            method("C.acc", 1), ent("C.junk1", 2), ent("C.junk2", 3),
            method("C.m1", 4), method("C.m2", 5), method("C.m3", 6),
        ],
        &[edge("C.m1", "C.acc"), edge("C.m2", "C.acc"), edge("C.m3", "C.acc")],
    );
    assert!(placement::wedges(&dg, &Weights::default()).is_empty(), "{:?}", placement::wedges(&dg, &Weights::default()));
}

#[test]
fn low_fanin_leaf_is_not_pinned_so_its_caller_still_anchors() {
    // C.swap is a leaf used by only ONE sibling -> not a shared accessor (fan-in 1), so it is NOT
    // pinned: C.user, declared far below with junk between, still owes the gap-to-deps to it.
    let dg = graph(
        &[method("C.swap", 1), ent("C.junk1", 2), ent("C.junk2", 3), method("C.user", 4)],
        &[edge("C.user", "C.swap")],
    );
    let u = dg.defs.iter().position(|d| d.qualname == "C.user").unwrap();
    assert_eq!(placement::wedges(&dg, &Weights::default()), [(u, 2)]); // junk1, junk2 wedged between swap(1) and user(4)
}

// --- definition-level reorder (declared far above its first use) -------------------------

#[test]
fn use_reorder_flags_definition_declared_far_above_its_use() {
    // h(1) is referenced only by u(5); junk1/2/3 sit between -> h is scattered too early, so the
    // suggestion is to push h DOWN to its first use (line 5), past the 3 independent wedges.
    let dg = graph(
        &[ent("h", 1), ent("junk1", 2), ent("junk2", 3), ent("junk3", 4), ent("u", 5)],
        &[edge("u", "h")],
    );
    let h = dg.defs.iter().position(|d| d.qualname == "h").unwrap();
    assert_eq!(placement::use_reorder(&dg, &Weights::default()), [(h, 5, 3)]);
}

#[test]
fn use_reorder_skips_a_forward_reference() {
    // u(1) references h(3) BELOW it -> h is forward-referenced (a declare-order matter), not a
    // declared-too-early definition, so there is nothing to move down.
    let dg = graph(&[ent("u", 1), ent("junk", 2), ent("h", 3)], &[edge("u", "h")]);
    assert!(placement::use_reorder(&dg, &Weights::default()).is_empty());
}

#[test]
fn use_reorder_exempts_siblings_co_used_at_the_first_use() {
    // u(5) calls BOTH h(1) and the sibling s(3) -> s is part of the same cluster pulled to line 5,
    // not junk wedging h. Only junk2 remains -> below threshold-free raw count is 1, not 2.
    let dg = graph(
        &[ent("h", 1), ent("junk2", 2), ent("s", 3), ent("u", 5)],
        &[edge("u", "h"), edge("u", "s")],
    );
    let h = dg.defs.iter().position(|d| d.qualname == "h").unwrap();
    assert_eq!(placement::use_reorder(&dg, &Weights::default()), [(h, 5, 1)]); // junk2 wedges; s is co-used, exempt
}

// --- extract-shared suggestions (via the full analyze pipeline) -------------------------

use ventouse::core::analyze::analyze;
use ventouse::core::model::Reason;
use ventouse::core::raw::RawModule;
use ventouse::core::score::Weights;

fn module(entities: Vec<RawEntity>, edges: Vec<(String, String)>) -> RawModule {
    RawModule { file: "t.rs".into(), module: "t".into(), entities, scope: vec![], def_edges: edges, decl_warnings: vec![] }
}

fn suggestions(m: RawModule) -> Vec<(String, Reason)> {
    analyze(&[m], &Weights::default())
        .into_iter()
        .filter(|f| f.category == Category::Suggestion)
        .map(|f| (f.entity, f.reason))
        .collect()
}

#[test]
fn suggests_extracting_a_high_fanin_definition() {
    // D is referenced by 4 distinct definitions (shared infrastructure); the file also carries
    // placement debt (y sits below x with junk wedged) -> suggest extracting D.
    let ents = vec![
        ent("x", 1), ent("junk", 2), ent("y", 3),
        ent("D", 4), ent("a", 5), ent("b", 6), ent("c", 7), ent("e", 8),
    ];
    let edges = vec![edge("y", "x"), edge("a", "D"), edge("b", "D"), edge("c", "D"), edge("e", "D")];
    let s = suggestions(module(ents, edges));
    assert_eq!(s, [("D".to_string(), Reason::ExtractShared(4))]);
}

#[test]
fn high_fanin_class_member_is_not_extracted() {
    // The SAME shape as above but with class members: "extract into its own module" is a module-scope
    // remedy that does not apply to a method (you cannot lift it out of its class), so no suggestion
    // — even though `C.y`/`C.x` give the file real placement debt.
    let ents = vec![
        method("C.x", 1), method("C.junk", 2), method("C.y", 3),
        method("C.D", 4), method("C.a", 5), method("C.b", 6), method("C.c", 7), method("C.e", 8),
    ];
    let edges = vec![
        edge("C.y", "C.x"), edge("C.a", "C.D"), edge("C.b", "C.D"), edge("C.c", "C.D"), edge("C.e", "C.D"),
    ];
    let s = suggestions(module(ents, edges));
    assert!(!s.iter().any(|(_, r)| matches!(r, Reason::ExtractShared(_))), "{s:?}");
}

#[test]
fn no_suggestion_below_the_fanin_threshold() {
    // D shared by only 3 -> below the threshold, even with placement debt present.
    let ents = vec![ent("x", 1), ent("junk", 2), ent("y", 3), ent("D", 4), ent("a", 5), ent("b", 6), ent("c", 7)];
    let edges = vec![edge("y", "x"), edge("a", "D"), edge("b", "D"), edge("c", "D")];
    assert!(suggestions(module(ents, edges)).is_empty());
}

#[test]
fn no_suggestion_without_placement_debt() {
    // D shared by 4 but the file is tight (no placement debt) -> nothing to extract.
    let ents = vec![ent("D", 1), ent("a", 2), ent("b", 3), ent("c", 4), ent("e", 5)];
    let edges = vec![edge("a", "D"), edge("b", "D"), edge("c", "D"), edge("e", "D")];
    assert!(suggestions(module(ents, edges)).is_empty());
}

#[test]
fn self_recursion_is_not_sharing() {
    // r calls itself many times but is referenced by only ONE other definition -> not shared.
    let ents = vec![ent("x", 1), ent("junk", 2), ent("y", 3), ent("r", 4), ent("caller", 5)];
    let edges = vec![
        edge("y", "x"),
        edge("r", "r"), edge("r", "r"), edge("r", "r"), edge("r", "r"), edge("r", "r"),
        edge("caller", "r"),
    ];
    assert!(suggestions(module(ents, edges)).is_empty());
}

// --- crowded-scope suggestions ----------------------------------------------------------

use ventouse::core::raw::RawScope;

fn module_with_scope(entities: Vec<RawEntity>, scope: Vec<RawScope>) -> RawModule {
    RawModule { file: "t.rs".into(), module: "t".into(), entities, scope, def_edges: vec![], decl_warnings: vec![] }
}

fn local(owner_dot_name: &str, levels: u32, wedges: u32, independent: bool) -> RawScope {
    RawScope { entity: owner_dot_name.into(), name: "x".into(), line: 2, levels_excess: levels, wedges, use_wedges: 0, first_use: 0, independent }
}

#[test]
fn suggests_bundling_a_bag_of_independent_locals() {
    // f's INDEPENDENT locals carry 10*(5+5)=100 scope-debt -> over the threshold -> a CrowdedScope
    // suggestion (bundle them into a struct).
    let m = module_with_scope(vec![ent("f", 1)], vec![local("f.a", 0, 5, true), local("f.b", 0, 5, true)]);
    assert_eq!(suggestions(m), [("f".to_string(), Reason::CrowdedScope(100))]);
}

#[test]
fn no_crowded_suggestion_for_interdependent_locals() {
    // same debt, but the locals read something (interdependent / sequential) — they can't be bundled,
    // so the function is NOT flagged (splitting it wouldn't lower locality debt).
    let m = module_with_scope(vec![ent("f", 1)], vec![local("f.a", 0, 5, false), local("f.b", 0, 5, false)]);
    assert!(suggestions(m).is_empty());
}

#[test]
fn no_crowded_suggestion_below_threshold() {
    let m = module_with_scope(vec![ent("f", 1)], vec![local("f.a", 0, 5, true), local("f.b", 0, 4, true)]); // 90
    assert!(suggestions(m).is_empty());
}

#[test]
fn crowded_suggestion_reports_a_method_owner() {
    // a method (C.m) owner is reported as a Method, via the per-binding qualname `C.m.local`.
    let mut ents = vec![ent("C", 1)];
    ents.push(RawEntity { qualname: "C.m".into(), kind: RawKind::Method, line: 2 });
    let m = module_with_scope(ents, vec![local("C.m.a", 6, 4, true)]); // 100
    let out = analyze(&[m], &Weights::default());
    let f = out.iter().find(|f| f.category == Category::Suggestion).unwrap();
    assert_eq!((f.entity.as_str(), f.entity_kind), ("C.m", ventouse::core::model::EntityKind::Method));
}

#[test]
fn crowded_suggestion_skips_an_owner_missing_from_entities() {
    // defensive: a local whose owner isn't in the entity list is skipped (no panic, no finding).
    let m = module_with_scope(vec![], vec![local("ghost.a", 0, 10, true)]); // debt 100 but no `ghost` entity
    assert!(suggestions(m).is_empty());
}

#[test]
fn suggests_extracting_a_heavily_used_helper_from_few_dispatchers() {
    // `line` is referenced 8 times but by only 2 distinct callers (big dispatchers) — broad fan-in
    // (>=4 distinct) misses it, heavy total use catches it. `y` below `dep` (junk wedged) gives the
    // module the placement debt that gates suggestions.
    let ents = vec![ent("line", 1), ent("a", 2), ent("b", 3), ent("dep", 4), ent("junk", 5), ent("y", 6)];
    let mut edges = vec![edge("y", "dep")];
    for _ in 0..4 {
        edges.push(edge("a", "line"));
        edges.push(edge("b", "line"));
    }
    assert_eq!(suggestions(module(ents, edges)), [("line".to_string(), Reason::ExtractShared(8))]);
}

// --- reorder-binding suggestions --------------------------------------------------------

/// A local binding declared at line 2 with a `use_wedges`-sized use-side gap, first used at `first_use`.
fn scattered(owner_dot_name: &str, use_wedges: u32, first_use: u32) -> RawScope {
    RawScope {
        entity: owner_dot_name.into(), name: "x".into(), line: 2,
        levels_excess: 0, wedges: use_wedges, use_wedges, first_use, independent: false,
    }
}

#[test]
fn suggests_reordering_a_binding_declared_far_from_its_use() {
    // q is declared at line 2 but first used at line 30, with 3 unrelated definitions between —
    // push the declaration down to its use.
    let m = module_with_scope(vec![ent("f", 1)], vec![scattered("f.q", 3, 30)]);
    assert_eq!(suggestions(m), [("f.q".to_string(), Reason::ReorderBinding { first_use: 30, wedged: 3 })]);
}

#[test]
fn no_reorder_suggestion_below_the_wedge_threshold() {
    // a 2-wedge gap stays a silent score (kept off the suggestion list as noise).
    let m = module_with_scope(vec![ent("f", 1)], vec![scattered("f.q", 2, 30)]);
    assert!(suggestions(m).is_empty());
}

#[test]
fn no_reorder_suggestion_when_use_is_not_below_the_declaration() {
    // first_use at/above the declaration (here: none recorded, first_use 0) — there is nothing to
    // move the declaration DOWN toward, so no suggestion even with a large use-side count.
    let m = module_with_scope(vec![ent("f", 1)], vec![scattered("f.q", 4, 0)]);
    assert!(suggestions(m).is_empty());
}

#[test]
fn heavy_use_from_a_single_caller_is_not_shared() {
    // `h` is referenced 8 times but all from ONE function — a private helper, not infrastructure.
    let ents = vec![ent("h", 1), ent("a", 2), ent("dep", 3), ent("junk", 4), ent("y", 5)];
    let mut edges = vec![edge("y", "dep")];
    for _ in 0..8 {
        edges.push(edge("a", "h"));
    }
    assert!(suggestions(module(ents, edges)).is_empty());
}
