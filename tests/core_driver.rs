//! Language-agnostic core: the `scopelang::build` driver tested via a TOY `ScopeLang` (no Python).
//! Verifies block/scope nesting and the attribution stack (`OpenAttrib`) drive the graph correctly.

use ventouse::core::scopegraph::BindKind;
use ventouse::core::scopelang::{Action, ScopeLang, build};

/// A minimal node language that maps 1:1 onto driver actions.
#[derive(Clone)]
enum N {
    Bind(String, u32),
    Decl(String, u32),
    Use(String, u32),
    Block(bool, Vec<N>),                 // is_loop, body
    Scope(String, bool, Vec<N>),         // leaf, is_class, body
    Attrib(String, Vec<N>),              // attribute body references to `leaf`
}

struct Toy;

impl ScopeLang for Toy {
    type Node = N;
    fn actions(&self, node: &N) -> Vec<Action<N>> {
        match node {
            N::Bind(name, line) => vec![Action::Bind {
                name: name.clone(),
                kind: BindKind::Value,
                line: *line,
                intro: false,
                deps: vec![],
                is_class_def: false,
            }],
            N::Decl(name, line) => vec![Action::Bind {
                name: name.clone(),
                kind: BindKind::Decl,
                line: *line,
                intro: false,
                deps: vec![],
                is_class_def: false,
            }],
            N::Use(name, line) => vec![Action::Use { name: name.clone(), line: *line, member: false }],
            N::Block(is_loop, body) => {
                vec![Action::OpenBlock { is_loop: *is_loop }, Action::Recurse(body.clone()), Action::Close]
            }
            N::Scope(leaf, is_class, body) => vec![
                Action::OpenScope { leaf: leaf.clone(), params: vec![], is_class: *is_class, is_namespace: false, nonlocals: vec![] },
                Action::Recurse(body.clone()),
                Action::Close,
            ],
            N::Attrib(leaf, body) => {
                vec![
                    Action::OpenAttrib { leaf: leaf.clone(), fallback_to_scope: false },
                    Action::Recurse(body.clone()),
                    Action::CloseAttrib,
                ]
            }
        }
    }
}

fn run(roots: Vec<N>) -> ventouse::core::scopegraph::ScopeOutput {
    build(&Toy, &roots).score()
}

#[test]
fn driver_block_nesting_drives_levels() {
    // a LOCAL `x` (in function f) used one block deeper -> 1 level too high. (Module-level values
    // are data definitions, not narrowed — so the value must live in a function scope.)
    let body = vec![N::Bind("x".into(), 1), N::Block(false, vec![N::Use("x".into(), 3)])];
    let out = run(vec![N::Scope("f".into(), false, body)]);
    assert_eq!(out.debt.len(), 1);
    assert_eq!(out.debt[0].levels_excess, 1);
}

#[test]
fn driver_loop_block_is_not_narrowed_into() {
    let body = vec![N::Bind("x".into(), 1), N::Block(/*is_loop*/ true, vec![N::Use("x".into(), 3)])];
    let out = run(vec![N::Scope("f".into(), false, body)]);
    assert!(out.debt.is_empty());
}

#[test]
fn driver_attribution_sets_edge_referrer_to_the_entity() {
    // a reference under OpenAttrib("f") resolves in the module scope but is attributed to `f`.
    let out = run(vec![
        N::Decl("g".into(), 5),
        N::Attrib("f".into(), vec![N::Use("g".into(), 2)]),
    ]);
    assert_eq!(out.edges, [("f".to_string(), "g".to_string())]);
}

#[test]
fn driver_without_attribution_referrer_is_the_scope() {
    // the same reference NOT under attribution is attributed to the enclosing scope (`<module>`).
    let out = run(vec![N::Decl("g".into(), 5), N::Use("g".into(), 2)]);
    assert_eq!(out.edges, [("<module>".to_string(), "g".to_string())]);
}

#[test]
fn driver_nested_scope_resolves_reference_to_module() {
    // a reference inside a NAMED nested scope (a `def f` = its Decl + its body scope) resolves up to
    // the module-level definition and is attributed to `f`.
    let out = run(vec![
        N::Decl("g".into(), 1),
        N::Decl("f".into(), 2),
        N::Scope("f".into(), false, vec![N::Use("g".into(), 3)]),
    ]);
    // referrer is the nested scope `f`; referent the module `g`.
    assert_eq!(out.edges, [("f".to_string(), "g".to_string())]);
}

#[test]
fn driver_anonymous_scope_attributes_to_enclosing_def() {
    // a reference inside an ANONYMOUS body (an OpenScope with no backing Decl — a lambda/closure)
    // is attributed to the nearest enclosing NAMED definition (`f`), not to the anonymous scope.
    // This closes the locality blind spot where a callee used only inside a closure escaped the graph.
    let out = run(vec![
        N::Decl("g".into(), 1),
        N::Decl("f".into(), 2),
        N::Scope("f".into(), false, vec![N::Scope("<lambda>".into(), false, vec![N::Use("g".into(), 3)])]),
    ]);
    assert_eq!(out.edges, [("f".to_string(), "g".to_string())]);
}

#[test]
fn driver_balanced_open_close_keeps_blocks_independent() {
    // two sibling blocks: local x used in both -> LCA is the function body -> 0 (Close pops right).
    let body = vec![
        N::Bind("x".into(), 1),
        N::Block(false, vec![N::Use("x".into(), 3)]),
        N::Block(false, vec![N::Use("x".into(), 5)]),
    ];
    let out = run(vec![N::Scope("f".into(), false, body)]);
    assert!(out.debt.is_empty());
}
