//! The TypeScript/JavaScript lowering RULES: how each oxc node maps to `core::scopelang::Action`s,
//! plus the `ScopeLang` impl dispatching statements / expressions / JSX. The vocabulary (line index,
//! node type, `Action` constructors) lives in `super::prim`.
//!
//! TS/JS is block-scoped: `let`/`const` are local to their block, so the `levels` term narrows the
//! REAL runtime scope. A `class` maps onto the core's class notion (methods → `C.method`, `this.x`
//! is a member reference); a class field is a DATA definition; a module-level `const` is data too.
//! Arrow / function expressions open anonymous scopes — references inside them are attributed to the
//! enclosing named definition (the core's `attribution_scope`), exactly like Python lambdas.

use oxc_ast::ast::{
    ArrowFunctionExpression, BindingPattern, BindingRestElement, Class, ClassElement, Declaration, Expression,
    ForStatementLeft, FormalParameters, Function, JSXAttributeItem, JSXAttributeValue, JSXChild, JSXElement,
    JSXElementName, JSXExpression, ObjectPropertyKind, PropertyKey, Statement, VariableDeclaration,
};
use oxc_span::{GetSpan, Span};

use crate::core::scopelang::{Action, ScopeLang};

use super::prim::{Ts, TsLang, bind_decl, bind_import, bind_value, member_use, use_};

type Acts<'a> = Vec<Action<Ts<'a>>>;

impl<'a> TsLang<'a> {
    fn ln(&self, span: Span) -> u32 {
        self.lines.line(span.start)
    }

    fn rec_exprs(&self, exprs: impl IntoIterator<Item = &'a Expression<'a>>) -> Action<Ts<'a>> {
        Action::Recurse(exprs.into_iter().map(Ts::Expr).collect())
    }

    fn rec_stmts(&self, stmts: &'a [Statement<'a>]) -> Action<Ts<'a>> {
        Action::Recurse(stmts.iter().map(Ts::Stmt).collect())
    }

    // --- patterns / params / dependencies ----------------------------------------------

    fn pat_binds(&self, pat: &'a BindingPattern<'a>, out: &mut Vec<(String, u32)>) {
        match pat {
            BindingPattern::BindingIdentifier(b) => out.push((b.name.as_str().to_string(), self.ln(b.span))),
            BindingPattern::ObjectPattern(o) => {
                for p in &o.properties {
                    self.pat_binds(&p.value, out);
                }
                self.rest_binds(o.rest.as_deref(), out);
            }
            BindingPattern::ArrayPattern(a) => {
                for e in a.elements.iter().flatten() {
                    self.pat_binds(e, out);
                }
                self.rest_binds(a.rest.as_deref(), out);
            }
            BindingPattern::AssignmentPattern(ap) => self.pat_binds(&ap.left, out),
        }
    }

    fn rest_binds(&self, rest: Option<&'a BindingRestElement<'a>>, out: &mut Vec<(String, u32)>) {
        if let Some(r) = rest {
            self.pat_binds(&r.argument, out);
        }
    }

    fn params(&self, fp: &'a FormalParameters<'a>) -> Vec<(String, u32)> {
        let mut out = Vec::new();
        for p in &fp.items {
            self.pat_binds(&p.pattern, &mut out);
        }
        self.rest_binds(fp.rest.as_deref().map(|fpr| &fpr.rest), &mut out);
        out
    }

    /// Single bound name of a pattern, if it is exactly one bare identifier (so a declarator's RHS
    /// can be attributed to it). Destructuring → `None`.
    fn single_name(pat: &BindingPattern<'a>) -> Option<String> {
        match pat {
            BindingPattern::BindingIdentifier(b) => Some(b.name.as_str().to_string()),
            BindingPattern::AssignmentPattern(ap) => Self::single_name(&ap.left),
            _ => None,
        }
    }

    /// Single-name identifiers read in an expression — a binding's RHS dependencies (shallow; does
    /// not descend into nested function / arrow scopes).
    fn loads(expr: &Expression<'a>, out: &mut Vec<String>) {
        match expr {
            Expression::Identifier(i) => out.push(i.name.as_str().to_string()),
            Expression::StaticMemberExpression(m) => Self::loads(&m.object, out),
            Expression::ComputedMemberExpression(m) => {
                Self::loads(&m.object, out);
                Self::loads(&m.expression, out);
            }
            Expression::CallExpression(c) => {
                Self::loads(&c.callee, out);
                c.arguments.iter().filter_map(|a| a.as_expression()).for_each(|e| Self::loads(e, out));
            }
            Expression::NewExpression(n) => {
                Self::loads(&n.callee, out);
                n.arguments.iter().filter_map(|a| a.as_expression()).for_each(|e| Self::loads(e, out));
            }
            Expression::BinaryExpression(b) => {
                Self::loads(&b.left, out);
                Self::loads(&b.right, out);
            }
            Expression::LogicalExpression(b) => {
                Self::loads(&b.left, out);
                Self::loads(&b.right, out);
            }
            Expression::UnaryExpression(u) => Self::loads(&u.argument, out),
            Expression::AwaitExpression(a) => Self::loads(&a.argument, out),
            Expression::ParenthesizedExpression(p) => Self::loads(&p.expression, out),
            Expression::ConditionalExpression(c) => {
                Self::loads(&c.test, out);
                Self::loads(&c.consequent, out);
                Self::loads(&c.alternate, out);
            }
            Expression::SequenceExpression(s) => s.expressions.iter().for_each(|e| Self::loads(e, out)),
            Expression::TemplateLiteral(t) => t.expressions.iter().for_each(|e| Self::loads(e, out)),
            Expression::ArrayExpression(a) => {
                a.elements.iter().filter_map(|e| e.as_expression()).for_each(|e| Self::loads(e, out));
            }
            Expression::ObjectExpression(o) => {
                for p in &o.properties {
                    if let ObjectPropertyKind::ObjectProperty(op) = p {
                        Self::loads(&op.value, out);
                    }
                }
            }
            _ => {} // arrow / function / literal: a nested scope or no reference
        }
    }

    fn collect_loads(expr: &Expression<'a>) -> Vec<String> {
        let mut out = Vec::new();
        Self::loads(expr, &mut out);
        out
    }

    /// Recurse a declarator's RHS, attributing its references to `leaf` when single-named (the driver
    /// routes the attribution to a module/class DATA definition, or back to the function).
    fn rhs(&self, leaf: Option<String>, value: &'a Expression<'a>) -> Acts<'a> {
        match leaf {
            Some(name) => vec![
                Action::OpenAttrib { leaf: name, fallback_to_scope: true },
                self.rec_exprs([value]),
                Action::CloseAttrib,
            ],
            None => vec![self.rec_exprs([value])],
        }
    }

    // --- declarations ------------------------------------------------------------------

    fn var_decl(&self, v: &'a VariableDeclaration<'a>) -> Acts<'a> {
        let mut acts = Vec::new();
        for d in &v.declarations {
            // `const Foo = () => …` / `= function …` / `= class …` is really a NAMED definition (the
            // dominant JS/React shape: components, hooks, handlers). Name the scope after the binding
            // so its body's debt is attributed to `Foo`, not an anonymous `<arrow>`.
            if let (Some(name), Some(init)) = (Self::single_name(&d.id), &d.init) {
                let line = self.ln(d.id.span());
                match init {
                    Expression::ArrowFunctionExpression(a) => {
                        acts.extend(self.named_arrow(&name, line, a));
                        continue;
                    }
                    Expression::FunctionExpression(f) => {
                        acts.extend(self.function(&name, line, f));
                        continue;
                    }
                    Expression::ClassExpression(c) => {
                        acts.extend(self.class(&name, line, c));
                        continue;
                    }
                    _ => {}
                }
            }
            let deps = d.init.as_ref().map(Self::collect_loads).unwrap_or_default();
            // A React hook result (`const [x] = useState()`, `useSelector(…)`) is positionally PINNED
            // by the rules of hooks — it must sit unconditionally at the top and cannot move down, so
            // (like a loop-carried seed) it is an introducer: no levels, no wedges, no reorder noise.
            let intro = d.init.as_ref().map(Self::is_hook_call).unwrap_or(false);
            let mut names = Vec::new();
            self.pat_binds(&d.id, &mut names);
            for (n, l) in &names {
                acts.push(bind_value(n, *l, intro, deps.clone()));
            }
            if let Some(init) = &d.init {
                acts.extend(self.rhs(Self::single_name(&d.id), init));
            }
        }
        acts
    }

    /// A `use*(…)` call (React hook) — its callee is a `useFoo` identifier or `X.useFoo` member. Such
    /// a result is positionally pinned by the rules of hooks (top-level, unconditional).
    fn is_hook_call(init: &Expression<'a>) -> bool {
        fn is_hook_name(n: &str) -> bool {
            n.len() > 3 && n.starts_with("use") && n.as_bytes()[3].is_ascii_uppercase()
        }
        if let Expression::CallExpression(c) = init {
            return match &c.callee {
                Expression::Identifier(id) => is_hook_name(id.name.as_str()),
                Expression::StaticMemberExpression(m) => is_hook_name(m.property.name.as_str()),
                _ => false,
            };
        }
        false
    }

    /// An arrow assigned to a name (`const f = (x) => …`): a `Decl` + its own named scope.
    fn named_arrow(&self, name: &str, line: u32, a: &'a ArrowFunctionExpression<'a>) -> Acts<'a> {
        vec![
            bind_decl(name, line, false),
            Action::OpenScope {
                leaf: name.to_string(),
                params: self.params(&a.params),
                is_class: false,
                is_namespace: false,
                nonlocals: vec![],
            },
            self.rec_stmts(&a.body.statements),
            Action::Close,
        ]
    }

    /// A function declaration / method: a `Decl` + its own scope (params + body).
    fn function(&self, name: &str, line: u32, f: &'a Function<'a>) -> Acts<'a> {
        let mut acts = vec![
            bind_decl(name, line, false),
            Action::OpenScope {
                leaf: name.to_string(),
                params: self.params(&f.params),
                is_class: false,
                is_namespace: false,
                nonlocals: vec![],
            },
        ];
        if let Some(body) = &f.body {
            acts.push(self.rec_stmts(&body.statements));
        }
        acts.push(Action::Close);
        acts
    }

    fn class(&self, name: &str, line: u32, c: &'a Class<'a>) -> Acts<'a> {
        let mut acts = vec![bind_decl(name, line, true)];
        if let Some(sup) = &c.super_class {
            acts.push(self.rec_exprs([sup]));
        }
        acts.push(Action::OpenScope {
            leaf: name.to_string(),
            params: vec![],
            is_class: true,
            is_namespace: false,
            nonlocals: vec![],
        });
        for el in &c.body.body {
            acts.extend(self.class_element(el));
        }
        acts.push(Action::Close);
        acts
    }

    fn key_name(key: &PropertyKey<'a>) -> Option<(String, bool)> {
        match key {
            PropertyKey::StaticIdentifier(i) => Some((i.name.as_str().to_string(), false)),
            PropertyKey::PrivateIdentifier(i) => Some((i.name.as_str().to_string(), false)),
            _ => None, // computed / string / numeric key — no stable entity name
        }
    }

    fn class_element(&self, el: &'a ClassElement<'a>) -> Acts<'a> {
        match el {
            ClassElement::MethodDefinition(m) => match Self::key_name(&m.key) {
                Some((name, _)) => self.function(&name, self.ln(m.key.span()), &m.value),
                None => self.function("<computed>", self.ln(m.span), &m.value),
            },
            ClassElement::PropertyDefinition(p) => {
                let Some((name, _)) = Self::key_name(&p.key) else { return vec![] };
                let line = self.ln(p.key.span());
                // a class-field arrow (`onClick = () => …`) is really a METHOD — name the scope after
                // the field so its body's debt is attributed to it, not an anonymous `<arrow>`.
                match &p.value {
                    Some(Expression::ArrowFunctionExpression(a)) => self.named_arrow(&name, line, a),
                    Some(Expression::FunctionExpression(f)) => self.function(&name, line, f),
                    // otherwise a DATA definition (attribute); its RHS is attributed to it.
                    other => {
                        let deps = other.as_ref().map(Self::collect_loads).unwrap_or_default();
                        let mut acts = vec![bind_value(&name, line, false, deps)];
                        if let Some(val) = other {
                            acts.extend(self.rhs(Some(name), val));
                        }
                        acts
                    }
                }
            }
            ClassElement::StaticBlock(b) => vec![self.rec_stmts(&b.body)],
            ClassElement::AccessorProperty(a) => match &a.value {
                Some(v) => vec![self.rec_exprs([v])],
                None => vec![],
            },
            ClassElement::TSIndexSignature(_) => vec![],
        }
    }

    fn declaration(&self, d: &'a Declaration<'a>) -> Acts<'a> {
        match d {
            Declaration::VariableDeclaration(v) => self.var_decl(v),
            Declaration::FunctionDeclaration(f) => match &f.id {
                Some(id) => self.function(id.name.as_str(), self.ln(id.span), f),
                None => vec![],
            },
            Declaration::ClassDeclaration(c) => match &c.id {
                Some(id) => self.class(id.name.as_str(), self.ln(id.span), c),
                None => vec![],
            },
            _ => vec![], // TS type alias / interface / enum / module: no runtime locality (skipped)
        }
    }

    // --- statements --------------------------------------------------------------------

    fn block(&self, stmts: &'a [Statement<'a>], is_loop: bool) -> Acts<'a> {
        vec![Action::OpenBlock { is_loop }, self.rec_stmts(stmts), Action::Close]
    }

    /// A loop whose `left` may declare the iteration variable (`for (const x of …)`): bind it as an
    /// introducer inside the loop body, then recurse the iterated expression and body.
    fn for_each(&self, left: &'a ForStatementLeft<'a>, right: &'a Expression<'a>, body: &'a Statement<'a>) -> Acts<'a> {
        let mut acts = vec![self.rec_exprs([right]), Action::OpenBlock { is_loop: true }];
        if let ForStatementLeft::VariableDeclaration(v) = left {
            let mut names = Vec::new();
            for d in &v.declarations {
                self.pat_binds(&d.id, &mut names);
            }
            acts.extend(names.into_iter().map(|(n, l)| bind_value(&n, l, true, vec![])));
        }
        acts.push(Action::Recurse(vec![Ts::Stmt(body)]));
        acts.push(Action::Close);
        acts
    }

    fn stmt(&self, s: &'a Statement<'a>) -> Acts<'a> {
        match s {
            Statement::VariableDeclaration(v) => self.var_decl(v),
            Statement::FunctionDeclaration(f) => match &f.id {
                Some(id) => self.function(id.name.as_str(), self.ln(id.span), f),
                None => vec![],
            },
            Statement::ClassDeclaration(c) => match &c.id {
                Some(id) => self.class(id.name.as_str(), self.ln(id.span), c),
                None => vec![],
            },
            Statement::ImportDeclaration(i) => match &i.specifiers {
                Some(specs) => specs
                    .iter()
                    .map(|sp| {
                        let local = sp.local();
                        bind_import(local.name.as_str(), self.ln(local.span))
                    })
                    .collect(),
                None => vec![],
            },
            Statement::ExportNamedDeclaration(e) => e.declaration.as_ref().map(|d| self.declaration(d)).unwrap_or_default(),
            Statement::ExportDefaultDeclaration(e) => {
                use oxc_ast::ast::ExportDefaultDeclarationKind as K;
                match &e.declaration {
                    K::FunctionDeclaration(f) => match &f.id {
                        Some(id) => self.function(id.name.as_str(), self.ln(id.span), f),
                        None => self.function("<default>", self.ln(e.span), f),
                    },
                    K::ClassDeclaration(c) => match &c.id {
                        Some(id) => self.class(id.name.as_str(), self.ln(id.span), c),
                        None => self.class("<default>", self.ln(e.span), c),
                    },
                    other => other.as_expression().map(|x| vec![self.rec_exprs([x])]).unwrap_or_default(),
                }
            }
            Statement::ExpressionStatement(e) => vec![self.rec_exprs([&e.expression])],
            Statement::BlockStatement(b) => self.block(&b.body, false),
            Statement::IfStatement(i) => {
                let mut acts = vec![self.rec_exprs([&i.test])];
                acts.push(Action::Recurse(vec![Ts::Stmt(&i.consequent)]));
                if let Some(alt) = &i.alternate {
                    acts.push(Action::Recurse(vec![Ts::Stmt(alt)]));
                }
                acts
            }
            Statement::ForStatement(f) => {
                let mut acts = vec![Action::OpenBlock { is_loop: true }];
                if let Some(init) = &f.init {
                    if let oxc_ast::ast::ForStatementInit::VariableDeclaration(v) = init {
                        acts.extend(self.var_decl(v));
                    } else if let Some(e) = init.as_expression() {
                        acts.push(self.rec_exprs([e]));
                    }
                }
                if let Some(t) = &f.test { acts.push(self.rec_exprs([t])); }
                if let Some(u) = &f.update { acts.push(self.rec_exprs([u])); }
                acts.push(Action::Recurse(vec![Ts::Stmt(&f.body)]));
                acts.push(Action::Close);
                acts
            }
            Statement::ForOfStatement(f) => self.for_each(&f.left, &f.right, &f.body),
            Statement::ForInStatement(f) => self.for_each(&f.left, &f.right, &f.body),
            Statement::WhileStatement(w) => {
                let mut acts = vec![self.rec_exprs([&w.test])];
                acts.push(Action::Recurse(vec![Ts::Stmt(&w.body)]));
                acts
            }
            Statement::DoWhileStatement(d) => {
                let mut acts = vec![Action::Recurse(vec![Ts::Stmt(&d.body)])];
                acts.push(self.rec_exprs([&d.test]));
                acts
            }
            Statement::ReturnStatement(r) => r.argument.iter().map(|e| self.rec_exprs([e])).collect(),
            Statement::ThrowStatement(t) => vec![self.rec_exprs([&t.argument])],
            Statement::TryStatement(t) => {
                let mut acts = self.block(&t.block.body, false);
                if let Some(h) = &t.handler {
                    acts.push(Action::OpenBlock { is_loop: false });
                    if let Some(p) = &h.param {
                        let mut names = Vec::new();
                        self.pat_binds(&p.pattern, &mut names);
                        acts.extend(names.into_iter().map(|(n, l)| bind_value(&n, l, true, vec![])));
                    }
                    acts.push(self.rec_stmts(&h.body.body));
                    acts.push(Action::Close);
                }
                if let Some(f) = &t.finalizer {
                    acts.extend(self.block(&f.body, false));
                }
                acts
            }
            Statement::SwitchStatement(s) => {
                let mut acts = vec![self.rec_exprs([&s.discriminant])];
                for case in &s.cases {
                    acts.push(Action::OpenBlock { is_loop: false });
                    if let Some(t) = &case.test { acts.push(self.rec_exprs([t])); }
                    acts.push(self.rec_stmts(&case.consequent));
                    acts.push(Action::Close);
                }
                acts
            }
            Statement::LabeledStatement(l) => vec![Action::Recurse(vec![Ts::Stmt(&l.body)])],
            _ => vec![], // break / continue / empty / debugger / TS-only declarations: no references
        }
    }

    // --- expressions -------------------------------------------------------------------

    fn arrow(&self, a: &'a ArrowFunctionExpression<'a>) -> Acts<'a> {
        vec![
            Action::OpenScope {
                leaf: "<arrow>".to_string(),
                params: self.params(&a.params),
                is_class: false,
                is_namespace: false,
                nonlocals: vec![],
            },
            self.rec_stmts(&a.body.statements),
            Action::Close,
        ]
    }

    fn func_expr(&self, f: &'a Function<'a>) -> Acts<'a> {
        let name = f.id.as_ref().map(|id| id.name.as_str().to_string()).unwrap_or_else(|| "<function>".to_string());
        let mut acts = vec![Action::OpenScope {
            leaf: name,
            params: self.params(&f.params),
            is_class: false,
            is_namespace: false,
            nonlocals: vec![],
        }];
        if let Some(body) = &f.body {
            acts.push(self.rec_stmts(&body.statements));
        }
        acts.push(Action::Close);
        acts
    }

    fn expr(&self, e: &'a Expression<'a>) -> Acts<'a> {
        match e {
            Expression::Identifier(i) => vec![use_(i.name.as_str(), self.ln(i.span))],
            Expression::CallExpression(c) => {
                let mut acts = vec![self.rec_exprs([&c.callee])];
                acts.push(self.rec_exprs(c.arguments.iter().filter_map(|a| a.as_expression())));
                acts
            }
            Expression::NewExpression(n) => {
                let mut acts = vec![self.rec_exprs([&n.callee])];
                acts.push(self.rec_exprs(n.arguments.iter().filter_map(|a| a.as_expression())));
                acts
            }
            // `this.x` is a member reference (resolves in the class scope); `a.b` is a field access
            // on another object — only recurse the base (a method is `this.x()`, handled here too).
            Expression::StaticMemberExpression(m) => {
                if matches!(&m.object, Expression::ThisExpression(_)) {
                    vec![member_use(m.property.name.as_str(), self.ln(m.property.span))]
                } else {
                    vec![self.rec_exprs([&m.object])]
                }
            }
            Expression::ComputedMemberExpression(m) => vec![self.rec_exprs([&m.object, &m.expression])],
            Expression::ChainExpression(c) => c.expression.as_member_expression().map_or_else(
                Vec::new,
                |me| match me {
                    oxc_ast::ast::MemberExpression::StaticMemberExpression(m) => {
                        if matches!(&m.object, Expression::ThisExpression(_)) {
                            vec![member_use(m.property.name.as_str(), self.ln(m.property.span))]
                        } else {
                            vec![self.rec_exprs([&m.object])]
                        }
                    }
                    oxc_ast::ast::MemberExpression::ComputedMemberExpression(m) => vec![self.rec_exprs([&m.object, &m.expression])],
                    oxc_ast::ast::MemberExpression::PrivateFieldExpression(m) => vec![self.rec_exprs([&m.object])],
                },
            ),
            Expression::BinaryExpression(b) => vec![self.rec_exprs([&b.left, &b.right])],
            Expression::LogicalExpression(b) => vec![self.rec_exprs([&b.left, &b.right])],
            Expression::UnaryExpression(u) => vec![self.rec_exprs([&u.argument])],
            Expression::UpdateExpression(_) => vec![], // `x++` on a target — no definition reference
            Expression::AwaitExpression(a) => vec![self.rec_exprs([&a.argument])],
            Expression::YieldExpression(y) => y.argument.iter().map(|e| self.rec_exprs([e])).collect(),
            Expression::ParenthesizedExpression(p) => vec![self.rec_exprs([&p.expression])],
            Expression::ConditionalExpression(c) => vec![self.rec_exprs([&c.test, &c.consequent, &c.alternate])],
            Expression::AssignmentExpression(a) => {
                let mut acts = vec![self.rec_exprs([&a.right])];
                if let Some(t) = a.left.get_expression() {
                    acts.push(self.rec_exprs([t]));
                }
                acts
            }
            Expression::SequenceExpression(s) => vec![self.rec_exprs(s.expressions.iter())],
            Expression::ArrayExpression(a) => {
                vec![self.rec_exprs(a.elements.iter().filter_map(|e| e.as_expression()))]
            }
            Expression::ObjectExpression(o) => {
                let mut acts = Vec::new();
                for p in &o.properties {
                    match p {
                        ObjectPropertyKind::ObjectProperty(op) => acts.push(self.rec_exprs([&op.value])),
                        ObjectPropertyKind::SpreadProperty(s) => acts.push(self.rec_exprs([&s.argument])),
                    }
                }
                acts
            }
            Expression::TemplateLiteral(t) => vec![self.rec_exprs(t.expressions.iter())],
            Expression::TaggedTemplateExpression(t) => {
                let mut acts = vec![self.rec_exprs([&t.tag])];
                acts.push(self.rec_exprs(t.quasi.expressions.iter()));
                acts
            }
            Expression::ArrowFunctionExpression(a) => self.arrow(a),
            Expression::FunctionExpression(f) => self.func_expr(f),
            Expression::ClassExpression(c) => match &c.id {
                Some(id) => self.class(id.name.as_str(), self.ln(id.span), c),
                None => self.class("<class>", self.ln(c.span), c),
            },
            Expression::JSXElement(j) => self.jsx_element(j),
            Expression::JSXFragment(f) => vec![Action::Recurse(f.children.iter().map(Ts::JsxChild).collect())],
            _ => vec![], // literals / this / super / import.meta / TS casts handled via children
        }
    }

    // --- JSX (React) -------------------------------------------------------------------

    fn jsx_element(&self, j: &'a JSXElement<'a>) -> Acts<'a> {
        let mut acts = Vec::new();
        // The opening tag: a capitalized / member name is a component REFERENCE (an edge).
        if let JSXElementName::IdentifierReference(r) = &j.opening_element.name {
            acts.push(use_(r.name.as_str(), self.ln(r.span)));
        }
        for attr in &j.opening_element.attributes {
            if let JSXAttributeItem::Attribute(a) = attr {
                if let Some(JSXAttributeValue::ExpressionContainer(c)) = &a.value {
                    acts.push(Action::Recurse(vec![Ts::JsxExpr(&c.expression)]));
                }
            } else if let JSXAttributeItem::SpreadAttribute(s) = attr {
                acts.push(self.rec_exprs([&s.argument]));
            }
        }
        acts.push(Action::Recurse(j.children.iter().map(Ts::JsxChild).collect()));
        acts
    }

    fn jsx_child(&self, c: &'a JSXChild<'a>) -> Acts<'a> {
        match c {
            JSXChild::Element(e) => self.jsx_element(e),
            JSXChild::Fragment(f) => vec![Action::Recurse(f.children.iter().map(Ts::JsxChild).collect())],
            JSXChild::ExpressionContainer(ec) => vec![Action::Recurse(vec![Ts::JsxExpr(&ec.expression)])],
            JSXChild::Spread(s) => vec![self.rec_exprs([&s.expression])],
            JSXChild::Text(_) => vec![],
        }
    }

    fn jsx_expr(&self, je: &'a JSXExpression<'a>) -> Acts<'a> {
        match je.as_expression() {
            Some(e) => vec![self.rec_exprs([e])],
            None => vec![], // JSXEmptyExpression (a comment-only `{}`)
        }
    }
}

impl<'a> ScopeLang for TsLang<'a> {
    type Node = Ts<'a>;
    fn actions(&self, node: &Ts<'a>) -> Vec<Action<Ts<'a>>> {
        match *node {
            Ts::Stmt(s) => self.stmt(s),
            Ts::Expr(e) => self.expr(e),
            Ts::JsxChild(c) => self.jsx_child(c),
            Ts::JsxExpr(e) => self.jsx_expr(e),
        }
    }
}
