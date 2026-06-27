//! The C++ lowering VOCABULARY: the node the driver walks (a libclang `Entity`), the `ScopeLang`
//! carrier (`CppLang`), cursor→line, and the `Action` constructors. Shared by every lowering rule
//! in `super::lower`.
//!
//! It lives in its own module on purpose — the same `ExtractShared` split ventouse applies to the
//! Rust/Python frontends: these are the high-fan-in helpers the rules speak, so keeping them
//! cross-file means a rule's reference to one doesn't wedge. The lowered `Node` is just the
//! libclang cursor (`Entity`, a `Copy` handle into the translation unit), so the profile is a thin
//! map from cursor kind to `Action`s — the walk, nesting and scoring are the shared core's.

use std::marker::PhantomData;

use clang::Entity;

use crate::core::scopegraph::BindKind;
use crate::core::scopelang::Action;

/// The `ScopeLang` carrier. `Node = Entity<'tu>` (a cursor into the parsed translation unit); the
/// `PhantomData` ties the profile to that lifetime so lowered `Recurse` lists stay valid.
pub(super) struct CppLang<'tu>(pub(super) PhantomData<&'tu ()>);

/// 1-based start line of a cursor (its spelling location).
pub(super) fn line(e: &Entity) -> u32 {
    e.get_location().map(|l| l.get_spelling_location().line).unwrap_or(1)
}

/// Lower these child cursors in the CURRENT scope/block.
pub(super) fn recurse(nodes: Vec<Entity>) -> Action<Entity> {
    Action::Recurse(nodes)
}

// The `Bind`/`Use` actions carry no cursor, so they are valid at any node lifetime — hence the
// generic `<'a>` (lets them unify into any `Vec<Action<Entity<'a>>>`). No dependencies → free.

/// A value binding (a local variable, or a namespace/class-level datum — the scope position the
/// core places it in decides which). `deps` are the names its initializer reads (for wedges).
pub(super) fn bind_value<'a>(name: &str, line: u32, deps: Vec<String>) -> Action<Entity<'a>> {
    Action::Bind { name: name.to_string(), kind: BindKind::Value, line, intro: false, deps, is_class_def: false }
}

/// A declaration of reusable code (function/method/class) — free of nesting. `is_class_def` marks a
/// class/struct so the entity list can tell Class from Function/Method.
pub(super) fn bind_decl<'a>(name: &str, line: u32, is_class_def: bool) -> Action<Entity<'a>> {
    Action::Bind { name: name.to_string(), kind: BindKind::Decl, line, intro: false, deps: vec![], is_class_def }
}

/// A plain name reference (a `DeclRefExpr`).
pub(super) fn use_<'a>(name: &str, line: u32) -> Action<Entity<'a>> {
    Action::Use { name: name.to_string(), line, member: false }
}

/// A member reference on the implicit `this` (a `MemberRefExpr`/`MemberRef` with no object), resolved
/// in the enclosing class scope.
pub(super) fn member_use<'a>(name: &str, line: u32) -> Action<Entity<'a>> {
    Action::Use { name: name.to_string(), line, member: true }
}
