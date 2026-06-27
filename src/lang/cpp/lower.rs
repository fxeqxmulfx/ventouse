//! The C++ lowering RULES: how each libclang cursor maps to `core::scopelang::Action`s, plus the
//! `ScopeLang` impl that dispatches on cursor kind. The vocabulary it speaks (the `Action`
//! constructors, the cursor→line helper) lives in `super::prim`.
//!
//! C++ is block-scoped, so every `{ … }` is a `CompoundStmt` the driver turns into a block — that is
//! what the `levels` term narrows. A function/method body's outer `CompoundStmt` is unwrapped into
//! the scope's own entry block (no spurious extra level); every inner brace nests. Member access on
//! the implicit `this` (a childless `MemberRefExpr`/`MemberRef`) resolves in the enclosing class
//! scope — like Rust's `self.`; an access through an explicit object carries that object as a child,
//! so we recurse the object and emit no member reference (its member belongs to another type).
//!
//! Ordered dependencies-first — each rule sits under the in-file helpers it builds on, so the file
//! reads top-down and its references to `prim` are cross-file (free).

use clang::{Entity, EntityKind};

use crate::core::scopelang::{Action, ScopeLang};

use super::prim::{CppLang, bind_decl, bind_value, line, member_use, recurse, use_};

/// Children of a cursor (the libclang AST is concrete — statements, sub-expressions and nested
/// declarations are all children).
fn kids<'tu>(e: &Entity<'tu>) -> Vec<Entity<'tu>> {
    e.get_children()
}

/// The names an initializer reads — every `DeclRefExpr` in its subtree (a binding's RHS
/// dependencies, for wedges). Shallow w.r.t. nested scopes: it does not descend into a lambda body.
fn collect_loads(e: &Entity, out: &mut Vec<String>) {
    match e.get_kind() {
        EntityKind::DeclRefExpr => {
            if let Some(n) = e.get_name() {
                out.push(n);
            }
        }
        EntityKind::LambdaExpr => {}
        _ => {
            for c in e.get_children() {
                collect_loads(&c, out);
            }
        }
    }
}

fn loads_of(children: &[Entity]) -> Vec<String> {
    let mut out = Vec::new();
    for c in children {
        collect_loads(c, &mut out);
    }
    out
}

/// A variable / data binding (a `VarDecl`: a function-local, a namespace global, or a class `static`
/// member): bind the name, then recurse its initializer with the references attributed to it (so the
/// datum is placed near / ordered after what it reads; for a function-local the driver falls the
/// attribution back to the enclosing function). The scope the driver is in decides whether this is a
/// narrowable local or namespace/class-level data. Instance fields (`FieldDecl`) are deliberately NOT
/// bound — like Rust struct fields, they are order-independent class state, not referenceable defs;
/// `this.field` then simply does not resolve (no spurious member edge).
fn var_actions<'tu>(e: &Entity<'tu>) -> Vec<Action<Entity<'tu>>> {
    let name = e.get_name().unwrap_or_default();
    let children = kids(e);
    let mut acts = vec![bind_value(&name, line(e), loads_of(&children))];
    if !children.is_empty() {
        acts.push(Action::OpenAttrib { leaf: name, fallback_to_scope: true });
        acts.push(recurse(children));
        acts.push(Action::CloseAttrib);
    }
    acts
}

/// A free function, method, constructor or destructor: a `Decl` plus its own scope (parameters +
/// body). A declaration with no body (a prototype, `= default`/`= delete`) is just the `Decl`.
/// The body's outer `CompoundStmt` is unwrapped into the scope's entry block; other children (a
/// constructor's member-initializer list) are recursed there too.
fn fn_actions<'tu>(e: &Entity<'tu>) -> Vec<Action<Entity<'tu>>> {
    let name = e.get_name().unwrap_or_default();
    let children = kids(e);
    let mut acts = vec![bind_decl(&name, line(e), false)];
    if !children.iter().any(|c| c.get_kind() == EntityKind::CompoundStmt) {
        return acts; // prototype / defaulted / deleted — no body
    }
    let params: Vec<(String, u32)> = children
        .iter()
        .filter(|c| c.get_kind() == EntityKind::ParmDecl)
        .map(|p| (p.get_name().unwrap_or_default(), line(p)))
        .collect();
    acts.push(Action::OpenScope { leaf: name, params, is_class: false, is_namespace: false, nonlocals: vec![] });
    for c in children {
        match c.get_kind() {
            EntityKind::ParmDecl => {}                          // bound as params above
            EntityKind::CompoundStmt => acts.push(recurse(kids(&c))), // unwrap the body
            _ => acts.push(recurse(vec![c])),                  // ctor member-init list, etc.
        }
    }
    acts.push(Action::Close);
    acts
}

/// A `struct`/`class`/`union`: a `Decl` mapped onto a class scope `T` — methods become `T::method`
/// (member-resolved), data members and static members become `T::x` data definitions. A forward
/// declaration (no body) is just the `Decl`.
fn class_actions<'tu>(e: &Entity<'tu>) -> Vec<Action<Entity<'tu>>> {
    let name = e.get_name().unwrap_or_default();
    let mut acts = vec![bind_decl(&name, line(e), true)];
    if !e.is_definition() {
        return acts; // forward declaration
    }
    acts.push(Action::OpenScope { leaf: name, params: vec![], is_class: true, is_namespace: false, nonlocals: vec![] });
    acts.push(recurse(kids(e))); // members dispatch through `actions`
    acts.push(Action::Close);
    acts
}

/// A namespace: a lexical scope (no entity of its own), like a Rust `mod`.
fn namespace_actions<'tu>(e: &Entity<'tu>) -> Vec<Action<Entity<'tu>>> {
    vec![
        Action::OpenScope {
            leaf: e.get_name().unwrap_or_default(),
            params: vec![],
            is_class: false,
            is_namespace: true,
            nonlocals: vec![],
        },
        recurse(kids(e)),
        Action::Close,
    ]
}

/// A loop (`for`/`while`/`do`/range-`for`): one loop block holding the control parts and the body.
/// The body's `CompoundStmt` is unwrapped into that same block (one level, like the core's Rust
/// `for`), and the block is marked `is_loop` so a binding outside the loop is not narrowed into it
/// (loop-carried state legitimately lives outside).
fn loop_actions<'tu>(e: &Entity<'tu>) -> Vec<Action<Entity<'tu>>> {
    let mut acts = vec![Action::OpenBlock { is_loop: true }];
    for c in kids(e) {
        match c.get_kind() {
            EntityKind::CompoundStmt => acts.push(recurse(kids(&c))), // unwrap the body
            _ => acts.push(recurse(vec![c])),                         // init / condition / increment
        }
    }
    acts.push(Action::Close);
    acts
}

/// A `MemberRefExpr`/`MemberRef`: on the implicit `this` (no object child) it is a member reference
/// resolved in the enclosing class; through an explicit object it carries that object as a child, so
/// we recurse the object and emit nothing (the member belongs to another type).
fn member_actions<'tu>(e: &Entity<'tu>) -> Vec<Action<Entity<'tu>>> {
    let children = kids(e);
    if children.is_empty() {
        vec![member_use(&e.get_name().unwrap_or_default(), line(e))]
    } else {
        vec![recurse(children)]
    }
}

/// Lower one cursor. Declarations open their scopes; statements/expressions either delimit a block
/// (`CompoundStmt`, loops) or are descended into so their references surface — the catch-all recurses
/// children, so no reference is missed and only ref-less leaves (literals, operators) drop out.
fn entity_actions<'tu>(e: &Entity<'tu>) -> Vec<Action<Entity<'tu>>> {
    match e.get_kind() {
        EntityKind::FunctionDecl
        | EntityKind::Method
        | EntityKind::Constructor
        | EntityKind::Destructor
        | EntityKind::FunctionTemplate => fn_actions(e),
        EntityKind::StructDecl | EntityKind::ClassDecl | EntityKind::UnionDecl | EntityKind::ClassTemplate => {
            class_actions(e)
        }
        EntityKind::Namespace => namespace_actions(e),
        EntityKind::VarDecl => var_actions(e),
        EntityKind::FieldDecl => vec![], // instance state, like a Rust struct field — not a referenceable def
        EntityKind::ForStmt | EntityKind::WhileStmt | EntityKind::DoStmt | EntityKind::ForRangeStmt => loop_actions(e),
        EntityKind::CompoundStmt => {
            vec![Action::OpenBlock { is_loop: false }, recurse(kids(e)), Action::Close]
        }
        EntityKind::DeclRefExpr => match e.get_name() {
            Some(n) => vec![use_(&n, line(e))],
            None => vec![],
        },
        EntityKind::MemberRefExpr | EntityKind::MemberRef => member_actions(e),
        EntityKind::ParmDecl => vec![], // bound by the enclosing function scope
        _ => vec![recurse(kids(e))],    // DeclStmt, control flow, sub-expressions: descend for refs
    }
}

impl<'tu> ScopeLang for CppLang<'tu> {
    type Node = Entity<'tu>;
    fn actions(&self, node: &Entity<'tu>) -> Vec<Action<Entity<'tu>>> {
        entity_actions(node)
    }
}
