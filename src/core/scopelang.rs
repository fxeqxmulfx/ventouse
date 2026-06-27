//! The language-driven scope-graph **builder** (the bridge from a language's surface to the L2
//! substrate). The locality RULES and all nesting bookkeeping (block ids, depth, scope threading)
//! live HERE; a frontend only LOWERS each of its syntax nodes into a flat list of [`Action`]s
//! ("this binds X", "this opens a loop block", "recurse these children") via [`ScopeLang`]. The
//! core driver interprets the actions against a block/scope stack and emits a [`ScopeGraph`].
//!
//! This is how "particulars under language" are derived: the per-language profile is exactly the
//! `actions` lowering; everything structural (the walk, the nesting, the scoring) is shared.

use crate::core::scopegraph::{BindKind, ScopeGraph};

/// One scope action a node lowers to. Executed in order against the current (scope, block) on the
/// driver's stack. `OpenBlock`/`OpenScope` push; `Close` pops — so a frontend brackets a body with
/// `OpenBlock { .. }` … `Close` and emits the body via `Recurse` in between.
pub enum Action<N> {
    /// Introduce a binding in the current scope/block. `is_class_def` marks a `Decl` that is a
    /// class (vs a function), so the entity list can tell Class from Function/Method.
    Bind { name: String, kind: BindKind, line: u32, intro: bool, deps: Vec<String>, is_class_def: bool },
    /// Reference a name in the current scope/block. `member` marks a `self.`/`cls.` reference
    /// (resolved in the enclosing class scope). Every reference to a definition is a graph edge.
    Use { name: String, line: u32, member: bool },
    /// Lower these child nodes in the CURRENT scope/block.
    Recurse(Vec<N>),
    /// Open a child block (depth +1); `is_loop` marks a for/while body (no narrowing into it).
    OpenBlock { is_loop: bool },
    /// Open a child lexical scope (function/class/lambda/comprehension/namespace), binding `params`
    /// in its entry block. `leaf` is the scope's own name (joined onto the parent qualname);
    /// `is_class` marks a class body (for `self`/`cls` member resolution); `is_namespace` marks a
    /// definition container (C++ `namespace`, Rust `mod`) whose value bindings are top-level DATA,
    /// not narrowable locals; `nonlocals` are names declared `global`/`nonlocal` in it.
    OpenScope { leaf: String, params: Vec<(String, u32)>, is_class: bool, is_namespace: bool, nonlocals: Vec<String> },
    /// Pop the most recently opened block/scope.
    Close,
    /// Attribute the references emitted until the matching `CloseAttrib` to the entity `leaf`
    /// (in the current scope) — used for a definition's header (decorator / base / default), which
    /// resolves in the enclosing scope but belongs to the entity being defined.
    ///
    /// `fallback_to_scope` is for a VALUE's right-hand side: `leaf` is a definition (and the target
    /// of the attribution) only when the value is module-/class-level DATA; a function-local value
    /// is not a definition, so its RHS references belong to the enclosing function instead. With the
    /// flag set, the driver attributes to `leaf` in a module/class scope and to the enclosing scope
    /// inside a function.
    OpenAttrib { leaf: String, fallback_to_scope: bool },
    /// End the most recent `OpenAttrib`.
    CloseAttrib,
}

/// A language profile: how to lower one of its syntax nodes into scope [`Action`]s. The ONLY
/// per-language code the scope analysis needs — the walk, nesting, and scoring are language-agnostic.
pub trait ScopeLang {
    type Node;
    fn actions(&self, node: &Self::Node) -> Vec<Action<Self::Node>>;
}

fn drive<L: ScopeLang>(
    g: &mut ScopeGraph,
    lang: &L,
    node: &L::Node,
    stack: &mut Vec<(usize, usize)>,
    attrib: &mut Vec<String>,
) {
    for act in lang.actions(node) {
        let (scope, block) = *stack.last().unwrap();
        match act {
            Action::Bind { name, kind, line, intro, deps, is_class_def } => {
                g.bind(scope, &name, kind, block, line, intro, is_class_def);
                if !deps.is_empty() {
                    g.set_deps(scope, &name, &deps);
                }
            }
            Action::Use { name, line, member } => g.add_use(&name, scope, block, line, member, attrib.last().cloned()),
            Action::Recurse(nodes) => {
                for n in &nodes {
                    drive(g, lang, n, stack, attrib);
                }
            }
            Action::OpenBlock { is_loop } => {
                let b = g.new_block(block, is_loop);
                stack.push((scope, b));
            }
            Action::OpenScope { leaf, params, is_class, is_namespace, nonlocals } => {
                let entry = g.new_block(block, false);
                let qual = g.child_qual(scope, &leaf);
                let child = g.new_scope(Some(scope), qual, false, is_class, is_namespace, nonlocals);
                for (p, l) in &params {
                    g.bind(child, p, BindKind::Param, entry, *l, false, false);
                }
                stack.push((child, entry));
            }
            Action::Close => {
                stack.pop();
            }
            // resolve in the current scope, but attribute to `leaf` (the entity being defined)
            Action::OpenAttrib { leaf, fallback_to_scope } => {
                let q = if fallback_to_scope { g.data_attrib(scope, &leaf) } else { g.child_qual(scope, &leaf) };
                attrib.push(q);
            }
            Action::CloseAttrib => {
                attrib.pop();
            }
        }
    }
}

/// Build a whole-module [`ScopeGraph`] by driving `lang` over `roots` (the module's top-level
/// nodes). Caller scores the result via [`ScopeGraph::score`].
pub fn build<L: ScopeLang>(lang: &L, roots: &[L::Node]) -> ScopeGraph {
    let mut g = ScopeGraph::new();
    let module = g.new_scope(None, "<module>".to_string(), true, false, false, Vec::new());
    let mut stack = vec![(module, g.module_block())];
    let mut attrib: Vec<String> = Vec::new();
    for n in roots {
        drive(&mut g, lang, n, &mut stack, &mut attrib);
    }
    g
}
