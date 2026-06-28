//! The TypeScript/JavaScript lowering VOCABULARY: the line index (oxc spans are byte offsets, so
//! line numbers need the source), the node type the driver walks (`Ts`), the `ScopeLang` carrier
//! (`TsLang`, holding the line index), and the `Action` constructors. The per-node RULES live in
//! `super::lower`.

use oxc_ast::ast::{Expression, JSXChild, JSXExpression, Statement};

use crate::core::scopegraph::BindKind;
use crate::core::scopelang::Action;

/// Byte-offset → 1-based line. Built once per file from the source (oxc `Span`s are byte offsets).
pub(super) struct LineIndex {
    /// Byte offset of the start of each line (line 1 starts at 0).
    starts: Vec<u32>,
}

impl LineIndex {
    pub(super) fn new(src: &str) -> LineIndex {
        let mut starts = vec![0u32];
        for (i, b) in src.bytes().enumerate() {
            if b == b'\n' {
                starts.push((i + 1) as u32);
            }
        }
        LineIndex { starts }
    }

    /// 1-based line containing byte `offset` = number of line-starts at or before it.
    pub(super) fn line(&self, offset: u32) -> u32 {
        self.starts.partition_point(|&s| s <= offset) as u32
    }
}

/// An oxc node to lower — the profile handles statements, expressions and JSX uniformly.
#[derive(Clone, Copy)]
pub(super) enum Ts<'a> {
    Stmt(&'a Statement<'a>),
    Expr(&'a Expression<'a>),
    /// A JSX expression container's inner expression (`{ … }`) — a superset of `Expression`.
    JsxExpr(&'a JSXExpression<'a>),
    /// A JSX child (`<Foo/>`, text, `{expr}`) inside an element body.
    JsxChild(&'a JSXChild<'a>),
}

/// The `ScopeLang` carrier: holds the line index the lowering needs to turn spans into lines.
pub(super) struct TsLang<'a> {
    pub(super) lines: &'a LineIndex,
}

// The `Bind`/`Use` actions carry no `Ts` node, so they unify into any `Vec<Action<Ts<'a>>>`.
pub(super) fn bind_value<'a>(name: &str, line: u32, intro: bool, deps: Vec<String>) -> Action<Ts<'a>> {
    Action::Bind { name: name.to_string(), kind: BindKind::Value, line, intro, deps, is_class_def: false }
}

pub(super) fn bind_decl<'a>(name: &str, line: u32, is_class_def: bool) -> Action<Ts<'a>> {
    Action::Bind { name: name.to_string(), kind: BindKind::Decl, line, intro: false, deps: vec![], is_class_def }
}

pub(super) fn bind_import<'a>(name: &str, line: u32) -> Action<Ts<'a>> {
    Action::Bind { name: name.to_string(), kind: BindKind::Import, line, intro: false, deps: vec![], is_class_def: false }
}

pub(super) fn use_<'a>(name: &str, line: u32) -> Action<Ts<'a>> {
    Action::Use { name: name.to_string(), line, member: false }
}

pub(super) fn member_use<'a>(name: &str, line: u32) -> Action<Ts<'a>> {
    Action::Use { name: name.to_string(), line, member: true }
}
