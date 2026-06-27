//! The Python lowering RULES: how each ruff node maps to `core::scopelang::Action`s, plus the
//! `ScopeLang` impl that dispatches statements/expressions. The vocabulary it speaks (node type,
//! `Action` constructors, line index, AST helpers) lives in `super::prim` — references to it are
//! cross-file, so the substrate doesn't wedge these rules.

use ruff_python_ast as ast;

use crate::core::scopegraph::BindKind;
use crate::core::scopelang::{Action, ScopeLang};

use super::prim::{Py, PyLang, collect_loads, scope_globals};

impl<'a> PyLang<'a> {
    /// The sole `Name` target of an assignment, if it has exactly one (so its RHS can be attributed
    /// to it). Multi-target (`a = b = ...`) / tuple unpacking is ambiguous → no attribution.
    fn single_name(targets: &'a [ast::Expr]) -> Option<&'a str> {
        match targets {
            [ast::Expr::Name(n)] => Some(n.id.as_str()),
            _ => None,
        }
    }

    /// Recurse a value's RHS, attributing its references to `leaf` when present — the driver routes
    /// the attribution to the data definition (module/class) or back to the enclosing function.
    fn rhs_attributed(leaf: Option<&str>, value: Option<&'a ast::Expr>) -> Vec<Action<Py<'a>>> {
        let Some(v) = value else { return vec![] };
        match leaf {
            Some(name) => vec![
                Action::OpenAttrib { leaf: name.to_string(), fallback_to_scope: true },
                Self::recurse_exprs([v]),
                Action::CloseAttrib,
            ],
            None => vec![Self::recurse_exprs([v])],
        }
    }

    /// A binding target (Store positions): Names bind; subscript/attribute bases are reads.
    fn target_actions(&self, e: &'a ast::Expr, intro: bool) -> Vec<Action<Py<'a>>> {
        match e {
            ast::Expr::Name(n) => vec![Self::bind(n.id.as_str(), BindKind::Value, self.line(n), intro, vec![])],
            ast::Expr::Tuple(t) => t.elts.iter().flat_map(|x| self.target_actions(x, intro)).collect(),
            ast::Expr::List(l) => l.elts.iter().flat_map(|x| self.target_actions(x, intro)).collect(),
            ast::Expr::Starred(s) => self.target_actions(&s.value, intro),
            ast::Expr::Subscript(s) => vec![Self::recurse_exprs([s.value.as_ref(), s.slice.as_ref()])],
            ast::Expr::Attribute(a) => vec![Self::recurse_exprs([a.value.as_ref()])],
            _ => vec![],
        }
    }

    /// Positional + var/kw parameter (name, line) pairs — bound in the scope's entry block.
    fn params(&self, params: &ast::Parameters) -> Vec<(String, u32)> {
        let mut out = Vec::new();
        for p in params.posonlyargs.iter().chain(&params.args).chain(&params.kwonlyargs) {
            out.push((p.parameter.name.as_str().to_string(), self.line(&p.parameter)));
        }
        if let Some(v) = &params.vararg {
            out.push((v.name.as_str().to_string(), self.line(v.as_ref())));
        }
        if let Some(k) = &params.kwarg {
            out.push((k.name.as_str().to_string(), self.line(k.as_ref())));
        }
        out
    }

    /// Default-value expressions (evaluated at def-time, in the enclosing scope).
    fn param_defaults(params: &'a ast::Parameters) -> Vec<&'a ast::Expr> {
        params
            .posonlyargs
            .iter()
            .chain(&params.args)
            .chain(&params.kwonlyargs)
            .filter_map(|p| p.default.as_deref())
            .collect()
    }

    /// What a match-`case` pattern contributes: the names it binds (capture / `*`/`**` rest, as
    /// introducers) plus the references it reads (literal/enum values, the matched class, mapping
    /// keys). Captures bind in the case block; references resolve in the enclosing scope.
    fn pattern_actions(&self, pat: &'a ast::Pattern) -> Vec<Action<Py<'a>>> {
        use ast::Pattern;
        let capture = |id: &'a ast::Identifier| Self::bind(id.as_str(), BindKind::Value, self.line(id), true, vec![]);
        match pat {
            Pattern::MatchValue(p) => vec![Self::recurse_exprs([p.value.as_ref()])],
            Pattern::MatchSingleton(_) => vec![],
            Pattern::MatchSequence(p) => p.patterns.iter().flat_map(|x| self.pattern_actions(x)).collect(),
            Pattern::MatchMapping(p) => {
                let mut acts = vec![Self::recurse_exprs(p.keys.iter())];
                for sub in &p.patterns {
                    acts.extend(self.pattern_actions(sub));
                }
                acts.extend(p.rest.iter().map(capture));
                acts
            }
            Pattern::MatchClass(p) => {
                let mut acts = vec![Self::recurse_exprs([p.cls.as_ref()])];
                for sub in p.arguments.patterns.iter().chain(p.arguments.keywords.iter().map(|k| &k.pattern)) {
                    acts.extend(self.pattern_actions(sub));
                }
                acts
            }
            Pattern::MatchStar(p) => p.name.iter().map(capture).collect(),
            Pattern::MatchAs(p) => {
                let mut acts = Vec::new();
                if let Some(sub) = &p.pattern {
                    acts.extend(self.pattern_actions(sub));
                }
                acts.extend(p.name.iter().map(capture));
                acts
            }
            Pattern::MatchOr(p) => p.patterns.iter().flat_map(|x| self.pattern_actions(x)).collect(),
        }
    }

    fn stmt_actions(&self, stmt: &'a ast::Stmt) -> Vec<Action<Py<'a>>> {
        match stmt {
            ast::Stmt::FunctionDef(f) => {
                // Header (decorators, default args) — resolves in the enclosing scope but is
                // attributed to the function being defined.
                let a = vec![
                    Self::bind(f.name.as_str(), BindKind::Decl, self.line(f), false, vec![]),
                    Action::OpenAttrib { leaf: f.name.to_string(), fallback_to_scope: false },
                    Self::recurse_exprs(f.decorator_list.iter().map(|d| &d.expression)),
                    Self::recurse_exprs(Self::param_defaults(&f.parameters)),
                    Action::CloseAttrib,
                    Action::OpenScope {
                        leaf: f.name.to_string(),
                        params: self.params(&f.parameters),
                        is_class: false,
                        is_namespace: false,
                        nonlocals: scope_globals(&f.body),
                    },
                    Self::recurse_stmts(&f.body),
                    Action::Close,
                ];
                a
            }
            ast::Stmt::ClassDef(c) => {
                let mut a = vec![
                    Action::Bind {
                        name: c.name.to_string(),
                        kind: BindKind::Decl,
                        line: self.line(c),
                        intro: false,
                        deps: vec![],
                        is_class_def: true,
                    },
                    // Header (decorators, base classes) — attributed to the class being defined.
                    Action::OpenAttrib { leaf: c.name.to_string(), fallback_to_scope: false },
                    Self::recurse_exprs(c.decorator_list.iter().map(|d| &d.expression)),
                ];
                if let Some(args) = &c.arguments {
                    a.push(Self::recurse_exprs(args.args.iter()));
                    a.push(Self::recurse_exprs(args.keywords.iter().map(|k| &k.value)));
                }
                a.push(Action::CloseAttrib);
                a.push(Action::OpenScope {
                    leaf: c.name.to_string(),
                    params: vec![],
                    is_class: true,
                    is_namespace: false,
                    nonlocals: scope_globals(&c.body),
                });
                a.push(Self::recurse_stmts(&c.body));
                a.push(Action::Close);
                a
            }
            ast::Stmt::Import(im) => {
                let line = self.line(stmt);
                im.names
                    .iter()
                    .map(|a| {
                        let bound = a.asname.as_ref().unwrap_or(&a.name);
                        let top = bound.as_str().split('.').next().unwrap_or(bound.as_str());
                        Self::bind(top, BindKind::Import, line, false, vec![])
                    })
                    .collect()
            }
            ast::Stmt::ImportFrom(im) => {
                let line = self.line(stmt);
                im.names
                    .iter()
                    .filter(|a| a.name.as_str() != "*")
                    .map(|a| {
                        let bound = a.asname.as_ref().unwrap_or(&a.name);
                        Self::bind(bound.as_str(), BindKind::Import, line, false, vec![])
                    })
                    .collect()
            }
            ast::Stmt::Assign(a) => {
                let rhs = collect_loads(&a.value);
                let mut acts = Vec::new();
                for t in &a.targets {
                    if let ast::Expr::Name(n) = t {
                        acts.push(Self::bind(n.id.as_str(), BindKind::Value, self.line(n), false, rhs.clone()));
                    } else {
                        acts.extend(self.target_actions(t, false));
                    }
                }
                // A single-name binding's RHS belongs to it WHEN it is a module/class DATA definition
                // (so a constant is placed near / ordered after what it is computed from); inside a
                // function the references belong to the function (`fallback_to_scope`).
                acts.extend(Self::rhs_attributed(Self::single_name(&a.targets), Some(a.value.as_ref())));
                acts
            }
            ast::Stmt::AnnAssign(a) => {
                let mut acts = Vec::new();
                // A BARE annotation `x: T` (no value) DECLARES — it does not bind: at class scope it
                // is an instance-attribute (field) declaration, not a class datum, so it must not
                // become a definition (else every `self.x` reads it and accrues placement debt — the
                // C++/Rust frontends drop fields for exactly this reason). Only `x: T = v` binds.
                if let ast::Expr::Name(n) = a.target.as_ref() {
                    if let Some(v) = &a.value {
                        acts.push(Self::bind(n.id.as_str(), BindKind::Value, self.line(n), false, collect_loads(v)));
                    }
                } else {
                    acts.extend(self.target_actions(&a.target, false));
                }
                if let Some(v) = &a.value {
                    let leaf = match a.target.as_ref() {
                        ast::Expr::Name(n) => Some(n.id.as_str()),
                        _ => None,
                    };
                    acts.extend(Self::rhs_attributed(leaf, Some(v.as_ref())));
                }
                acts
            }
            ast::Stmt::AugAssign(a) => {
                let mut acts = Vec::new();
                if let ast::Expr::Name(n) = a.target.as_ref() {
                    acts.push(Self::use_(n.id.as_str(), self.line(n)));
                } else {
                    acts.push(Self::recurse_exprs([a.target.as_ref()]));
                }
                acts.push(Self::recurse_exprs([a.value.as_ref()]));
                acts
            }
            ast::Stmt::Return(r) => r.value.iter().map(|v| Self::recurse_exprs([v.as_ref()])).collect(),
            ast::Stmt::Expr(e) => vec![Self::recurse_exprs([e.value.as_ref()])],
            ast::Stmt::Delete(d) => vec![Self::recurse_exprs(d.targets.iter())],
            ast::Stmt::Assert(a) => vec![Self::recurse_exprs([a.test.as_ref()])],
            ast::Stmt::Raise(r) => {
                let mut acts = Vec::new();
                if let Some(e) = &r.exc {
                    acts.push(Self::recurse_exprs([e.as_ref()]));
                }
                if let Some(e) = &r.cause {
                    acts.push(Self::recurse_exprs([e.as_ref()]));
                }
                acts
            }
            ast::Stmt::If(s) => {
                let mut acts = vec![
                    Self::recurse_exprs([s.test.as_ref()]),
                    Action::OpenBlock { is_loop: false },
                    Self::recurse_stmts(&s.body),
                    Action::Close,
                ];
                for clause in &s.elif_else_clauses {
                    if let Some(t) = &clause.test {
                        acts.push(Self::recurse_exprs([t]));
                    }
                    acts.push(Action::OpenBlock { is_loop: false });
                    acts.push(Self::recurse_stmts(&clause.body));
                    acts.push(Action::Close);
                }
                acts
            }
            ast::Stmt::For(s) => {
                // The iterable is evaluated once in the enclosing scope; the loop TARGET belongs to
                // the loop (bound INSIDE its block), so it never wedges a binding that sits right
                // before the loop and is used inside it. (Scope resolution is unaffected — the target
                // is still in the same function scope, just a deeper block.)
                let mut acts = vec![Self::recurse_exprs([s.iter.as_ref()]), Action::OpenBlock { is_loop: true }];
                acts.extend(self.target_actions(&s.target, true));
                acts.push(Self::recurse_stmts(&s.body));
                acts.push(Action::Close);
                if !s.orelse.is_empty() {
                    acts.push(Action::OpenBlock { is_loop: false });
                    acts.push(Self::recurse_stmts(&s.orelse));
                    acts.push(Action::Close);
                }
                acts
            }
            ast::Stmt::While(s) => {
                let mut acts = vec![
                    Self::recurse_exprs([s.test.as_ref()]),
                    Action::OpenBlock { is_loop: true },
                    Self::recurse_stmts(&s.body),
                    Action::Close,
                ];
                if !s.orelse.is_empty() {
                    acts.push(Action::OpenBlock { is_loop: false });
                    acts.push(Self::recurse_stmts(&s.orelse));
                    acts.push(Action::Close);
                }
                acts
            }
            ast::Stmt::With(s) => {
                let mut acts = Vec::new();
                for it in &s.items {
                    acts.push(Self::recurse_exprs([&it.context_expr]));
                    if let Some(v) = &it.optional_vars {
                        acts.extend(self.target_actions(v, true));
                    }
                }
                acts.push(Action::OpenBlock { is_loop: false });
                acts.push(Self::recurse_stmts(&s.body));
                acts.push(Action::Close);
                acts
            }
            ast::Stmt::Try(s) => {
                let mut acts = vec![
                    Action::OpenBlock { is_loop: false },
                    Self::recurse_stmts(&s.body),
                    Action::Close,
                ];
                for h in &s.handlers {
                    let ast::ExceptHandler::ExceptHandler(h) = h;
                    acts.push(Action::OpenBlock { is_loop: false });
                    if let Some(name) = &h.name {
                        acts.push(Self::bind(name.as_str(), BindKind::Value, self.line(name), true, vec![]));
                    }
                    acts.push(Self::recurse_stmts(&h.body));
                    acts.push(Action::Close);
                }
                if !s.orelse.is_empty() {
                    acts.push(Action::OpenBlock { is_loop: false });
                    acts.push(Self::recurse_stmts(&s.orelse));
                    acts.push(Action::Close);
                }
                if !s.finalbody.is_empty() {
                    acts.push(Action::OpenBlock { is_loop: false });
                    acts.push(Self::recurse_stmts(&s.finalbody));
                    acts.push(Action::Close);
                }
                acts
            }
            ast::Stmt::Match(m) => {
                let mut acts = vec![Self::recurse_exprs([m.subject.as_ref()])];
                for case in &m.cases {
                    acts.push(Action::OpenBlock { is_loop: false });
                    acts.extend(self.pattern_actions(&case.pattern));
                    if let Some(guard) = &case.guard {
                        acts.push(Self::recurse_exprs([guard.as_ref()]));
                    }
                    acts.push(Self::recurse_stmts(&case.body));
                    acts.push(Action::Close);
                }
                acts
            }
            _ => vec![], // global/nonlocal (collected separately), pass/break/continue: no references
        }
    }

    /// `lambda params: body` — its own scope (like a nested function / closure): params bind, the
    /// body recurses inside. References in the body are attributed to the lambda scope, so captures
    /// narrow; the body's call edges aren't in the graph (a known limit, same as Rust closures).
    fn lambda_actions(&self, parameters: Option<&'a ast::Parameters>, body: &'a ast::Expr) -> Vec<Action<Py<'a>>> {
        let params = parameters.map(|p| self.params(p)).unwrap_or_default();
        vec![
            Action::OpenScope { leaf: "<lambda>".to_string(), params, is_class: false, is_namespace: false, nonlocals: vec![] },
            Self::recurse_exprs([body]),
            Action::Close,
        ]
    }

    /// A comprehension / generator — a Py3 scope of its own. The FIRST iterable is evaluated in the
    /// ENCLOSING scope; the targets, guards, remaining iterables and the result expression(s) live
    /// in the comprehension scope (so the loop targets narrow there, not in the enclosing function).
    fn comprehension_actions(&self, result: &[&'a ast::Expr], generators: &'a [ast::Comprehension], leaf: &str) -> Vec<Action<Py<'a>>> {
        let mut acts = Vec::new();
        if let Some(first) = generators.first() {
            acts.push(Self::recurse_exprs([&first.iter]));
        }
        acts.push(Action::OpenScope { leaf: leaf.to_string(), params: vec![], is_class: false, is_namespace: false, nonlocals: vec![] });
        for (i, c) in generators.iter().enumerate() {
            acts.extend(self.target_actions(&c.target, true));
            if i > 0 {
                acts.push(Self::recurse_exprs([&c.iter]));
            }
            acts.push(Self::recurse_exprs(c.ifs.iter()));
        }
        acts.push(Self::recurse_exprs(result.iter().copied()));
        acts.push(Action::Close);
        acts
    }

    fn expr_actions(&self, e: &'a ast::Expr) -> Vec<Action<Py<'a>>> {
        match e {
            ast::Expr::Name(n) => match n.ctx {
                ast::ExprContext::Store => vec![Self::bind(n.id.as_str(), BindKind::Value, self.line(n), false, vec![])],
                ast::ExprContext::Load => vec![Self::use_(n.id.as_str(), self.line(n))],
                _ => vec![],
            },
            ast::Expr::Named(x) => {
                let mut acts = self.target_actions(&x.target, true);
                acts.push(Self::recurse_exprs([x.value.as_ref()]));
                acts
            }
            ast::Expr::BoolOp(x) => vec![Self::recurse_exprs(x.values.iter())],
            ast::Expr::BinOp(x) => vec![Self::recurse_exprs([x.left.as_ref(), x.right.as_ref()])],
            ast::Expr::UnaryOp(x) => vec![Self::recurse_exprs([x.operand.as_ref()])],
            ast::Expr::Compare(x) => {
                let mut v = vec![x.left.as_ref()];
                v.extend(x.comparators.iter());
                vec![Self::recurse_exprs(v)]
            }
            ast::Expr::If(x) => vec![Self::recurse_exprs([x.test.as_ref(), x.body.as_ref(), x.orelse.as_ref()])],
            ast::Expr::Call(x) => {
                // A call is just a reference in func position + uses in the args (the references
                // model treats calls and other references alike).
                let mut v = vec![x.func.as_ref()];
                v.extend(x.arguments.args.iter());
                v.extend(x.arguments.keywords.iter().map(|k| &k.value));
                vec![Self::recurse_exprs(v)]
            }
            ast::Expr::Attribute(x) => {
                // `self.m` / `cls.m` (read OR call) is a class-scoped member reference; any other
                // `obj.attr` drops the attribute (unknown receiver) and just uses the receiver.
                if let ast::Expr::Name(b) = x.value.as_ref()
                    && (b.id.as_str() == "self" || b.id.as_str() == "cls")
                {
                    vec![Self::member_use(x.attr.as_str(), self.line(x.value.as_ref())), Self::recurse_exprs([x.value.as_ref()])]
                } else {
                    vec![Self::recurse_exprs([x.value.as_ref()])]
                }
            }
            ast::Expr::Subscript(x) => vec![Self::recurse_exprs([x.value.as_ref(), x.slice.as_ref()])],
            ast::Expr::Starred(x) => vec![Self::recurse_exprs([x.value.as_ref()])],
            ast::Expr::Await(x) => vec![Self::recurse_exprs([x.value.as_ref()])],
            ast::Expr::List(x) => vec![Self::recurse_exprs(x.elts.iter())],
            ast::Expr::Tuple(x) => vec![Self::recurse_exprs(x.elts.iter())],
            ast::Expr::Set(x) => vec![Self::recurse_exprs(x.elts.iter())],
            ast::Expr::Dict(x) => {
                let mut v = Vec::new();
                for it in &x.items {
                    if let Some(k) = &it.key {
                        v.push(k);
                    }
                    v.push(&it.value);
                }
                vec![Self::recurse_exprs(v)]
            }
            ast::Expr::Lambda(x) => self.lambda_actions(x.parameters.as_deref(), &x.body),
            ast::Expr::ListComp(x) => self.comprehension_actions(&[x.elt.as_ref()], &x.generators, "<listcomp>"),
            ast::Expr::SetComp(x) => self.comprehension_actions(&[x.elt.as_ref()], &x.generators, "<setcomp>"),
            ast::Expr::Generator(x) => self.comprehension_actions(&[x.elt.as_ref()], &x.generators, "<genexpr>"),
            ast::Expr::DictComp(x) => {
                self.comprehension_actions(&[x.key.as_ref(), x.value.as_ref()], &x.generators, "<dictcomp>")
            }
            ast::Expr::Yield(x) => x.value.iter().map(|v| Self::recurse_exprs([v.as_ref()])).collect(),
            ast::Expr::YieldFrom(x) => vec![Self::recurse_exprs([x.value.as_ref()])],
            ast::Expr::Slice(x) => {
                let v: Vec<&ast::Expr> = [&x.lower, &x.upper, &x.step].into_iter().flatten().map(|e| e.as_ref()).collect();
                vec![Self::recurse_exprs(v)]
            }
            // f-string: the `{expr}` interpolations are references in the current scope (the literal
            // parts have none). `format_spec`s can hold expressions too, but those are rare.
            ast::Expr::FString(x) => {
                let v: Vec<&ast::Expr> = x
                    .value
                    .elements()
                    .filter_map(|el| match el {
                        ast::InterpolatedStringElement::Interpolation(e) => Some(e.expression.as_ref()),
                        ast::InterpolatedStringElement::Literal(_) => None,
                    })
                    .collect();
                vec![Self::recurse_exprs(v)]
            }
            _ => vec![], // constants / literals / ellipsis: no references
        }
    }
}

impl<'a> ScopeLang for PyLang<'a> {
    type Node = Py<'a>;
    fn actions(&self, node: &Py<'a>) -> Vec<Action<Py<'a>>> {
        match *node {
            Py::Stmt(s) => self.stmt_actions(s),
            Py::Expr(e) => self.expr_actions(e),
        }
    }
}
