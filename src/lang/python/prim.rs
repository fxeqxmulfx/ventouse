//! The Python lowering VOCABULARY: the ruff line index, the node type the driver walks (`Py`), the
//! `PyLang` profile carrier, the `Action` constructors, and the small AST helpers (`scope_globals`,
//! `collect_loads`). Shared by every rule in `super::lower`.
//!
//! It lives in its own module on ventouse's own `ExtractShared` suggestion (it flagged `PyLang::line`,
//! `recurse_exprs` as referenced-all-over): per-module analysis doesn't penalize cross-file
//! references, so the substrate no longer wedges the rules — and the split is genuinely cleaner
//! (vocabulary vs rules), mirroring `lang/rust`.

use ruff_python_ast as ast;
use ruff_text_size::Ranged;

use crate::core::scopegraph::BindKind;
use crate::core::scopelang::Action;

// --- line index: byte offset -> 1-based row ---------------------------------------------

pub(super) struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    pub(super) fn new(src: &str) -> LineIndex {
        let mut starts = vec![0usize];
        for (i, b) in src.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        LineIndex { starts }
    }

    fn row(&self, offset: usize) -> u32 {
        match self.starts.binary_search(&offset) {
            Ok(i) => (i + 1) as u32,
            Err(i) => i as u32,
        }
    }
}

pub(super) fn start_row(idx: &LineIndex, node: &impl Ranged) -> u32 {
    idx.row(node.range().start().to_usize())
}

// --- node type + profile ----------------------------------------------------------------

/// A ruff node to lower — a statement or an expression (the profile handles both uniformly).
#[derive(Clone, Copy)]
pub(super) enum Py<'a> {
    Stmt(&'a ast::Stmt),
    Expr(&'a ast::Expr),
}

/// The Python scope profile: it knows how to lower ruff syntax into scope actions.
pub(super) struct PyLang<'a> {
    pub(super) idx: &'a LineIndex,
}

// --- AST helpers ------------------------------------------------------------------------

/// Names declared `global`/`nonlocal` in a body (descending into blocks but NOT into nested
/// function/class scopes) — they are not local bindings of this scope.
pub(super) fn scope_globals(body: &[ast::Stmt]) -> Vec<String> {
    let mut out = Vec::new();
    fn go(body: &[ast::Stmt], out: &mut Vec<String>) {
        for stmt in body {
            match stmt {
                ast::Stmt::Global(g) => out.extend(g.names.iter().map(|n| n.as_str().to_string())),
                ast::Stmt::Nonlocal(g) => out.extend(g.names.iter().map(|n| n.as_str().to_string())),
                ast::Stmt::If(s) => {
                    go(&s.body, out);
                    for c in &s.elif_else_clauses {
                        go(&c.body, out);
                    }
                }
                ast::Stmt::For(s) => {
                    go(&s.body, out);
                    go(&s.orelse, out);
                }
                ast::Stmt::While(s) => {
                    go(&s.body, out);
                    go(&s.orelse, out);
                }
                ast::Stmt::With(s) => go(&s.body, out),
                ast::Stmt::Try(s) => {
                    go(&s.body, out);
                    for h in &s.handlers {
                        let ast::ExceptHandler::ExceptHandler(h) = h;
                        go(&h.body, out);
                    }
                    go(&s.orelse, out);
                    go(&s.finalbody, out);
                }
                _ => {} // nested FunctionDef/ClassDef are their own scopes
            }
        }
    }
    go(body, &mut out);
    out
}

/// Bare names read (Load) in an expression — a binding's RHS dependencies (shallow; skips
/// lambda/comprehension scopes).
pub(super) fn collect_loads(expr: &ast::Expr) -> Vec<String> {
    let mut out = Vec::new();
    fn go(e: &ast::Expr, out: &mut Vec<String>) {
        match e {
            ast::Expr::Name(n) => {
                if matches!(n.ctx, ast::ExprContext::Load) {
                    out.push(n.id.as_str().to_string());
                }
            }
            ast::Expr::Named(x) => go(&x.value, out),
            ast::Expr::BoolOp(x) => x.values.iter().for_each(|v| go(v, out)),
            ast::Expr::BinOp(x) => {
                go(&x.left, out);
                go(&x.right, out);
            }
            ast::Expr::UnaryOp(x) => go(&x.operand, out),
            ast::Expr::Compare(x) => {
                go(&x.left, out);
                x.comparators.iter().for_each(|c| go(c, out));
            }
            ast::Expr::If(x) => {
                go(&x.test, out);
                go(&x.body, out);
                go(&x.orelse, out);
            }
            ast::Expr::Call(x) => {
                go(&x.func, out);
                x.arguments.args.iter().for_each(|a| go(a, out));
                x.arguments.keywords.iter().for_each(|k| go(&k.value, out));
            }
            ast::Expr::Attribute(x) => go(&x.value, out),
            ast::Expr::Subscript(x) => {
                go(&x.value, out);
                go(&x.slice, out);
            }
            ast::Expr::Starred(x) => go(&x.value, out),
            ast::Expr::Await(x) => go(&x.value, out),
            ast::Expr::List(x) => x.elts.iter().for_each(|v| go(v, out)),
            ast::Expr::Tuple(x) => x.elts.iter().for_each(|v| go(v, out)),
            ast::Expr::Set(x) => x.elts.iter().for_each(|v| go(v, out)),
            ast::Expr::Dict(x) => {
                for it in &x.items {
                    if let Some(k) = &it.key {
                        go(k, out);
                    }
                    go(&it.value, out);
                }
            }
            _ => {}
        }
    }
    go(expr, &mut out);
    out
}

// --- the Action constructors (the vocabulary the rules speak) ---------------------------

impl<'a> PyLang<'a> {
    pub(super) fn line(&self, node: &impl Ranged) -> u32 {
        self.idx.row(node.range().start().to_usize())
    }

    pub(super) fn bind(name: &str, kind: BindKind, line: u32, intro: bool, deps: Vec<String>) -> Action<Py<'a>> {
        Action::Bind { name: name.to_string(), kind, line, intro, deps, is_class_def: false }
    }

    /// A plain (lexically-resolved) name reference.
    pub(super) fn use_(name: &str, line: u32) -> Action<Py<'a>> {
        Action::Use { name: name.to_string(), line, member: false }
    }

    /// A `self.`/`cls.` member reference — resolved in the enclosing class scope.
    pub(super) fn member_use(name: &str, line: u32) -> Action<Py<'a>> {
        Action::Use { name: name.to_string(), line, member: true }
    }

    pub(super) fn recurse_exprs(exprs: impl IntoIterator<Item = &'a ast::Expr>) -> Action<Py<'a>> {
        Action::Recurse(exprs.into_iter().map(Py::Expr).collect())
    }

    pub(super) fn recurse_stmts(stmts: &'a [ast::Stmt]) -> Action<Py<'a>> {
        Action::Recurse(stmts.iter().map(Py::Stmt).collect())
    }
}
