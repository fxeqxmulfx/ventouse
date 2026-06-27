//! The Rust lowering VOCABULARY: the node type the driver walks (`Ru`), the `ScopeLang` carrier
//! (`RuLang`), span→line, the `Action` constructors, and the pattern primitives. Shared by every
//! lowering rule in `super::lower`.
//!
//! It lives in its own module on purpose: these are the high-fan-in helpers ventouse's own
//! `ExtractShared` suggestion flagged in `lang/rust.rs`. Per-module analysis doesn't penalize
//! cross-file references, so the substrate no longer wedges the rules that use it — and the split is
//! genuinely cleaner (vocabulary vs rules). Within the file each helper sits right under what it
//! builds on (the `Ru` constructors under `Ru`, `pat_binds` under its walker), so it scores ~0 too.

use proc_macro2::Span;
use syn::spanned::Spanned;

use crate::core::scopegraph::BindKind;
use crate::core::scopelang::Action;

/// A syn node to lower — the profile handles items, statements and expressions uniformly.
#[derive(Clone, Copy)]
pub(super) enum Ru<'a> {
    Item(&'a syn::Item),
    Stmt(&'a syn::Stmt),
    Expr(&'a syn::Expr),
}

pub(super) fn recurse_exprs<'a>(exprs: impl IntoIterator<Item = &'a syn::Expr>) -> Action<Ru<'a>> {
    Action::Recurse(exprs.into_iter().map(Ru::Expr).collect())
}

pub(super) fn recurse_stmts(stmts: &[syn::Stmt]) -> Action<Ru<'_>> {
    Action::Recurse(stmts.iter().map(Ru::Stmt).collect())
}

/// `OpenBlock … body … Close` for a brace block.
pub(super) fn block_actions(block: &syn::Block, is_loop: bool) -> Vec<Action<Ru<'_>>> {
    vec![Action::OpenBlock { is_loop }, recurse_stmts(&block.stmts), Action::Close]
}

pub(super) struct RuLang<'a>(pub(super) std::marker::PhantomData<&'a ()>);

/// 1-based start line of any spanned node.
pub(super) fn line(node: &impl Spanned) -> u32 {
    node.span().start().line as u32
}

pub(super) fn span_line(s: Span) -> u32 {
    s.start().line as u32
}

fn go_pat(pat: &syn::Pat, out: &mut Vec<(String, u32)>) {
    use syn::Pat;
    match pat {
        Pat::Ident(p) => {
            out.push((p.ident.to_string(), span_line(p.ident.span())));
            if let Some((_, sub)) = &p.subpat {
                go_pat(sub, out);
            }
        }
        Pat::Tuple(p) => p.elems.iter().for_each(|e| go_pat(e, out)),
        Pat::TupleStruct(p) => p.elems.iter().for_each(|e| go_pat(e, out)),
        Pat::Slice(p) => p.elems.iter().for_each(|e| go_pat(e, out)),
        Pat::Struct(p) => p.fields.iter().for_each(|f| go_pat(&f.pat, out)),
        Pat::Reference(p) => go_pat(&p.pat, out),
        Pat::Or(p) => p.cases.iter().for_each(|c| go_pat(c, out)),
        Pat::Paren(p) => go_pat(&p.pat, out),
        Pat::Type(p) => go_pat(&p.pat, out),
        _ => {} // Wild / Rest / Lit / Path (constructor) / Range / Const / Macro: no bindings
    }
}

/// Names a pattern binds, with their lines (destructuring, `ref`/`mut`, sub-patterns).
pub(super) fn pat_binds(pat: &syn::Pat) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    go_pat(pat, &mut out);
    out
}

/// The sole bound identifier of a pattern, if it is exactly one bare name (so a `let`'s RHS can be
/// attributed to it). Tuple/struct destructuring → `None` (no single owner).
pub(super) fn single_ident(pat: &syn::Pat) -> Option<String> {
    match pat {
        syn::Pat::Ident(p) if p.subpat.is_none() => Some(p.ident.to_string()),
        syn::Pat::Type(p) => single_ident(&p.pat),
        syn::Pat::Reference(p) => single_ident(&p.pat),
        _ => None,
    }
}

// The `Bind`/`Use` actions carry no `Ru` node, so they are valid at any node lifetime — hence the
// generic `<'a>` (lets them unify into any `Vec<Action<Ru<'a>>>`). No dependencies → free.
pub(super) fn bind_value<'a>(name: &str, line: u32, intro: bool, deps: Vec<String>) -> Action<Ru<'a>> {
    Action::Bind { name: name.to_string(), kind: BindKind::Value, line, intro, deps, is_class_def: false }
}

pub(super) fn bind_decl<'a>(name: &str, line: u32, is_class_def: bool) -> Action<Ru<'a>> {
    Action::Bind { name: name.to_string(), kind: BindKind::Decl, line, intro: false, deps: vec![], is_class_def }
}

pub(super) fn bind_import<'a>(name: &str, line: u32) -> Action<Ru<'a>> {
    Action::Bind { name: name.to_string(), kind: BindKind::Import, line, intro: false, deps: vec![], is_class_def: false }
}

pub(super) fn use_<'a>(name: &str, line: u32) -> Action<Ru<'a>> {
    Action::Use { name: name.to_string(), line, member: false }
}

pub(super) fn member_use<'a>(name: &str, line: u32) -> Action<Ru<'a>> {
    Action::Use { name: name.to_string(), line, member: true }
}
