//! The Rust lowering RULES: how each `syn` node maps to `core::scopelang::Action`s, plus the
//! `ScopeLang` impl that dispatches Item/Stmt/Expr. The vocabulary it speaks (node type, `Action`
//! constructors, pattern primitives) lives in `super::prim`.
//!
//! Ordered dependencies-first — each rule sits right under the in-file helpers it builds on (its
//! cross-file uses of `prim` are free), so the file reads top-down and scores ~0.

use crate::core::scopelang::{Action, ScopeLang};

use super::prim::{
    Ru, RuLang, bind_decl, bind_import, bind_value, block_actions, line, member_use, pat_binds, recurse_exprs,
    recurse_stmts, single_ident, span_line, use_,
};

/// Recurse a value's RHS, attributing its references to `leaf` when it is a single name — the driver
/// routes the attribution to the data definition (module/impl const) or back to the function.
fn rhs_attributed<'a>(leaf: Option<String>, value: &'a syn::Expr) -> Vec<Action<Ru<'a>>> {
    match leaf {
        Some(name) => vec![
            Action::OpenAttrib { leaf: name, fallback_to_scope: true },
            recurse_exprs([value]),
            Action::CloseAttrib,
        ],
        None => vec![recurse_exprs([value])],
    }
}

// --- paths ------------------------------------------------------------------------------

const EXTERNAL_ROOTS: &[&str] = &["crate", "super", "std", "core", "alloc"];

/// References from a path expression. A single segment `foo` is a plain reference; `Self::x` is a
/// member; `Type::x` / `module::x` references the leading segment (a local type/module); a path
/// rooted at `crate`/`std`/… is cross-module and dropped.
fn path_use<'a>(path: &syn::Path, line: u32) -> Vec<Action<Ru<'a>>> {
    let segs: Vec<_> = path.segments.iter().collect();
    let Some(first) = segs.first() else { return vec![] };
    let head = first.ident.to_string();
    if segs.len() == 1 {
        if head == "self" {
            return vec![];
        }
        return vec![use_(&head, line)];
    }
    if head == "Self" {
        return vec![member_use(&segs[1].ident.to_string(), line)];
    }
    if EXTERNAL_ROOTS.contains(&head.as_str()) {
        return vec![];
    }
    vec![use_(&head, line)]
}

/// Bare `self`.
fn is_self(e: &syn::Expr) -> bool {
    matches!(e, syn::Expr::Path(p) if p.path.is_ident("self"))
}

/// The leaf names a `use` tree brings into scope (respecting `as`, skipping globs).
fn use_names(tree: &syn::UseTree) -> Vec<(String, u32)> {
    use syn::UseTree;
    match tree {
        UseTree::Path(p) => use_names(&p.tree),
        UseTree::Name(n) => vec![(n.ident.to_string(), span_line(n.ident.span()))],
        UseTree::Rename(r) => vec![(r.rename.to_string(), span_line(r.rename.span()))],
        UseTree::Group(g) => g.items.iter().flat_map(use_names).collect(),
        UseTree::Glob(_) => vec![],
    }
}

// --- collect_loads (a binding's RHS dependencies, shallow) ------------------------------

fn go_loads(e: &syn::Expr, out: &mut Vec<String>) {
    use syn::Expr;
    match e {
        Expr::Path(p) => {
            if let Some(id) = p.path.get_ident() {
                out.push(id.to_string());
            }
        }
        Expr::Call(c) => {
            go_loads(&c.func, out);
            c.args.iter().for_each(|a| go_loads(a, out));
        }
        Expr::MethodCall(m) => {
            go_loads(&m.receiver, out);
            m.args.iter().for_each(|a| go_loads(a, out));
        }
        Expr::Binary(b) => {
            go_loads(&b.left, out);
            go_loads(&b.right, out);
        }
        Expr::Unary(u) => go_loads(&u.expr, out),
        Expr::Reference(r) => go_loads(&r.expr, out),
        Expr::Paren(p) => go_loads(&p.expr, out),
        Expr::Group(g) => go_loads(&g.expr, out),
        Expr::Field(f) => go_loads(&f.base, out),
        Expr::Index(i) => {
            go_loads(&i.expr, out);
            go_loads(&i.index, out);
        }
        Expr::Cast(c) => go_loads(&c.expr, out),
        Expr::Try(t) => go_loads(&t.expr, out),
        Expr::Await(a) => go_loads(&a.base, out),
        Expr::Tuple(t) => t.elems.iter().for_each(|e| go_loads(e, out)),
        Expr::Array(a) => a.elems.iter().for_each(|e| go_loads(e, out)),
        Expr::Struct(s) => s.fields.iter().for_each(|f| go_loads(&f.expr, out)),
        Expr::Range(r) => {
            if let Some(s) = &r.start {
                go_loads(s, out);
            }
            if let Some(en) = &r.end {
                go_loads(en, out);
            }
        }
        _ => {}
    }
}

/// Single-segment path identifiers read in an expression — a binding's RHS dependencies (shallow;
/// does not descend into nested closure/block scopes).
fn collect_loads(expr: &syn::Expr) -> Vec<String> {
    let mut out = Vec::new();
    go_loads(expr, &mut out);
    out
}

/// References inside a macro invocation. Macro bodies are TOKENS, not AST, so the lowering can't
/// recurse them — but a well-behaved macro (`vec!`, `format!`, `assert_eq!`, …) has a body that
/// parses as comma-separated expressions; recover their references so the references model isn't
/// blind to them. A body that isn't an expression list (`matches!(x, Pat)`, a custom DSL) fails to
/// parse and is skipped (the residual hole). All references take the macro's line (good enough — a
/// macro call sits on one line for placement / order).
fn macro_uses<'a>(mac: &syn::Macro) -> Vec<Action<Ru<'a>>> {
    let ln = line(mac);
    match mac.parse_body_with(syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated) {
        Ok(args) => args.iter().flat_map(collect_loads).map(|n| use_(&n, ln)).collect(),
        Err(_) => vec![],
    }
}

// --- statements (depend on collect_loads/rhs_attributed, so they sit right here) ---------

/// `let PAT = EXPR else { … };` — bind the pattern's names (block-local, so they narrow), recurse
/// the initializer (attributed to a single-name target), then the `else` diverging block.
fn local_actions(local: &syn::Local) -> Vec<Action<Ru<'_>>> {
    let deps = local.init.as_ref().map(|i| collect_loads(&i.expr)).unwrap_or_default();
    let mut acts: Vec<Action<Ru>> =
        pat_binds(&local.pat).into_iter().map(|(n, l)| bind_value(&n, l, false, deps.clone())).collect();
    if let Some(init) = &local.init {
        acts.extend(rhs_attributed(single_ident(&local.pat), &init.expr));
        if let Some((_, diverge)) = &init.diverge {
            acts.push(recurse_exprs([diverge.as_ref()]));
        }
    }
    acts
}

// --- functions / impls / data definitions / items ---------------------------------------

/// A `const`/`static` (module level) or associated const: a DATA definition whose RHS is attributed
/// to it (so it is placed near / ordered after the data it is computed from).
fn data_def<'a>(name: &str, ln: u32, expr: &'a syn::Expr) -> Vec<Action<Ru<'a>>> {
    let mut acts = vec![bind_value(name, ln, false, collect_loads(expr))];
    acts.extend(rhs_attributed(Some(name.to_string()), expr));
    acts
}

/// Typed parameters (the `self` receiver is handled by member resolution, not a binding).
fn fn_params(sig: &syn::Signature) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    for arg in &sig.inputs {
        if let syn::FnArg::Typed(pt) = arg {
            out.extend(pat_binds(&pt.pat));
        }
    }
    out
}

/// A free function or a method: a `Decl` + its own scope (params + body).
fn fn_actions<'a>(sig: &'a syn::Signature, block: &'a syn::Block) -> Vec<Action<Ru<'a>>> {
    vec![
        bind_decl(&sig.ident.to_string(), line(&sig.ident), false),
        Action::OpenScope {
            leaf: sig.ident.to_string(),
            params: fn_params(sig),
            is_class: false,
            is_namespace: false,
            nonlocals: vec![],
        },
        recurse_stmts(&block.stmts),
        Action::Close,
    ]
}

/// The leaf name of an impl's self type (`impl Foo`, `impl Trait for Foo` → `Foo`).
fn type_leaf(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        syn::Type::Reference(r) => type_leaf(&r.elem),
        _ => None,
    }
}

/// `impl T { … }` maps onto a class scope `T`: methods become `T::method` (member-resolved), and
/// associated consts become `T::CONST` data definitions.
fn impl_actions(im: &syn::ItemImpl) -> Vec<Action<Ru<'_>>> {
    let Some(leaf) = type_leaf(&im.self_ty) else { return vec![] };
    let mut acts = vec![Action::OpenScope { leaf, params: vec![], is_class: true, is_namespace: false, nonlocals: vec![] }];
    for it in &im.items {
        match it {
            syn::ImplItem::Fn(f) => acts.extend(fn_actions(&f.sig, &f.block)),
            syn::ImplItem::Const(c) => acts.extend(data_def(&c.ident.to_string(), line(&c.ident), &c.expr)),
            _ => {}
        }
    }
    acts.push(Action::Close);
    acts
}

fn item_actions(item: &syn::Item) -> Vec<Action<Ru<'_>>> {
    use syn::Item;
    match item {
        Item::Fn(f) => fn_actions(&f.sig, &f.block),
        Item::Impl(im) => impl_actions(im),
        Item::Struct(s) => vec![bind_decl(&s.ident.to_string(), line(&s.ident), true)],
        Item::Enum(e) => vec![bind_decl(&e.ident.to_string(), line(&e.ident), true)],
        Item::Union(u) => vec![bind_decl(&u.ident.to_string(), line(&u.ident), true)],
        Item::Trait(t) => vec![bind_decl(&t.ident.to_string(), line(&t.ident), true)],
        Item::Const(c) => data_def(&c.ident.to_string(), line(&c.ident), &c.expr),
        Item::Static(s) => data_def(&s.ident.to_string(), line(&s.ident), &s.expr),
        Item::Use(u) => use_names(&u.tree).iter().map(|(n, l)| bind_import(n, *l)).collect(),
        Item::Mod(m) => match &m.content {
            Some((_, items)) => {
                let mut acts = vec![Action::OpenScope {
                    leaf: m.ident.to_string(),
                    params: vec![],
                    is_class: false,
                    is_namespace: true, // a Rust `mod` is a definition container, like a C++ namespace
                    nonlocals: vec![],
                }];
                acts.push(Action::Recurse(items.iter().map(Ru::Item).collect()));
                acts.push(Action::Close);
                acts
            }
            None => vec![],
        },
        _ => vec![],
    }
}

// --- expressions (per-construct lowering, then the dispatcher) ---------------------------

/// Split an `if`/`while` condition into the expressions to evaluate in the CURRENT scope and the
/// patterns bound for the body — covering both `if let PAT = EXPR` and let-chains
/// (`if a && let Some(b) = c`, where the `let`'s pattern is in scope for the body).
fn cond_lets<'a>(cond: &'a syn::Expr, recurse: &mut Vec<&'a syn::Expr>, pats: &mut Vec<&'a syn::Pat>) {
    match cond {
        syn::Expr::Let(l) => {
            recurse.push(&l.expr);
            pats.push(&l.pat);
        }
        syn::Expr::Binary(b) if matches!(b.op, syn::BinOp::And(_)) => {
            cond_lets(&b.left, recurse, pats);
            cond_lets(&b.right, recurse, pats);
        }
        syn::Expr::Paren(p) => cond_lets(&p.expr, recurse, pats),
        syn::Expr::Group(g) => cond_lets(&g.expr, recurse, pats),
        other => recurse.push(other),
    }
}

/// `OpenBlock … bind the condition's `let` patterns … body … Close` (shared by `if`/`while`).
fn cond_block_actions<'a>(cond: &'a syn::Expr, body: &'a syn::Block, is_loop: bool) -> Vec<Action<Ru<'a>>> {
    let (mut recurse, mut pats) = (Vec::new(), Vec::new());
    cond_lets(cond, &mut recurse, &mut pats);
    let mut acts = vec![recurse_exprs(recurse), Action::OpenBlock { is_loop }];
    acts.extend(pats.into_iter().flat_map(pat_binds).map(|(n, ln)| bind_value(&n, ln, true, vec![])));
    acts.push(recurse_stmts(&body.stmts));
    acts.push(Action::Close);
    acts
}

fn if_actions(i: &syn::ExprIf) -> Vec<Action<Ru<'_>>> {
    let mut acts = cond_block_actions(&i.cond, &i.then_branch, false);
    if let Some((_, els)) = &i.else_branch {
        acts.push(recurse_exprs([els.as_ref()]));
    }
    acts
}

fn while_actions(w: &syn::ExprWhile) -> Vec<Action<Ru<'_>>> {
    cond_block_actions(&w.cond, &w.body, true)
}

fn match_actions(m: &syn::ExprMatch) -> Vec<Action<Ru<'_>>> {
    let mut acts = vec![recurse_exprs([m.expr.as_ref()])];
    for arm in &m.arms {
        acts.push(Action::OpenBlock { is_loop: false });
        acts.extend(pat_binds(&arm.pat).into_iter().map(|(n, ln)| bind_value(&n, ln, true, vec![])));
        if let Some((_, guard)) = &arm.guard {
            acts.push(recurse_exprs([guard.as_ref()]));
        }
        acts.push(recurse_exprs([arm.body.as_ref()]));
        acts.push(Action::Close);
    }
    acts
}

fn for_actions(f: &syn::ExprForLoop) -> Vec<Action<Ru<'_>>> {
    let mut acts = vec![recurse_exprs([f.expr.as_ref()])];
    acts.push(Action::OpenBlock { is_loop: true });
    acts.extend(pat_binds(&f.pat).into_iter().map(|(n, ln)| bind_value(&n, ln, true, vec![])));
    acts.push(recurse_stmts(&f.body.stmts));
    acts.push(Action::Close);
    acts
}

fn closure_actions(c: &syn::ExprClosure) -> Vec<Action<Ru<'_>>> {
    let params: Vec<(String, u32)> = c.inputs.iter().flat_map(pat_binds).collect();
    vec![
        Action::OpenScope { leaf: "{closure}".to_string(), params, is_class: false, is_namespace: false, nonlocals: vec![] },
        recurse_exprs([c.body.as_ref()]),
        Action::Close,
    ]
}

fn expr_actions(e: &syn::Expr) -> Vec<Action<Ru<'_>>> {
    use syn::Expr;
    match e {
        Expr::Path(p) => path_use(&p.path, line(p)),
        Expr::Call(c) => {
            let mut v = vec![c.func.as_ref()];
            v.extend(c.args.iter());
            vec![recurse_exprs(v)]
        }
        Expr::MethodCall(m) => {
            let mut acts = Vec::new();
            if is_self(&m.receiver) {
                acts.push(member_use(&m.method.to_string(), line(&m.method)));
            }
            acts.push(recurse_exprs([m.receiver.as_ref()]));
            acts.push(recurse_exprs(m.args.iter()));
            acts
        }
        // A field access `self.x` (or `a.b`) is NOT a definition reference: in Rust a field is
        // always a field (a method is `self.x()`, handled above), and fields aren't entities. Emit
        // no member use — only recurse the base — so `self.coeffs` never resolves to a same-named
        // `coeffs()` method (that conflation produced phantom declare-order warnings).
        Expr::Field(f) => vec![recurse_exprs([f.base.as_ref()])],
        Expr::Struct(s) => {
            let mut acts = path_use(&s.path, line(s));
            acts.push(recurse_exprs(s.fields.iter().map(|f| &f.expr)));
            if let Some(rest) = &s.rest {
                acts.push(recurse_exprs([rest.as_ref()]));
            }
            acts
        }
        Expr::Binary(b) => vec![recurse_exprs([b.left.as_ref(), b.right.as_ref()])],
        Expr::Unary(u) => vec![recurse_exprs([u.expr.as_ref()])],
        Expr::Reference(r) => vec![recurse_exprs([r.expr.as_ref()])],
        Expr::Paren(p) => vec![recurse_exprs([p.expr.as_ref()])],
        Expr::Group(g) => vec![recurse_exprs([g.expr.as_ref()])],
        Expr::Cast(c) => vec![recurse_exprs([c.expr.as_ref()])],
        Expr::Try(t) => vec![recurse_exprs([t.expr.as_ref()])],
        Expr::Await(a) => vec![recurse_exprs([a.base.as_ref()])],
        Expr::Index(i) => vec![recurse_exprs([i.expr.as_ref(), i.index.as_ref()])],
        Expr::Assign(a) => vec![recurse_exprs([a.left.as_ref(), a.right.as_ref()])],
        Expr::Array(a) => vec![recurse_exprs(a.elems.iter())],
        Expr::Tuple(t) => vec![recurse_exprs(t.elems.iter())],
        Expr::Repeat(r) => vec![recurse_exprs([r.expr.as_ref(), r.len.as_ref()])],
        Expr::Range(r) => {
            let mut v = Vec::new();
            if let Some(s) = &r.start {
                v.push(s.as_ref());
            }
            if let Some(en) = &r.end {
                v.push(en.as_ref());
            }
            vec![recurse_exprs(v)]
        }
        Expr::Return(r) => r.expr.iter().map(|e| recurse_exprs([e.as_ref()])).collect(),
        Expr::Break(b) => b.expr.iter().map(|e| recurse_exprs([e.as_ref()])).collect(),
        Expr::If(i) => if_actions(i),
        Expr::Match(m) => match_actions(m),
        Expr::ForLoop(f) => for_actions(f),
        Expr::While(w) => while_actions(w),
        Expr::Loop(l) => block_actions(&l.body, true),
        Expr::Block(b) => block_actions(&b.block, false),
        Expr::Unsafe(u) => block_actions(&u.block, false),
        Expr::Async(a) => block_actions(&a.block, false),
        Expr::TryBlock(t) => block_actions(&t.block, false),
        Expr::Closure(c) => closure_actions(c),
        Expr::Let(l) => vec![recurse_exprs([l.expr.as_ref()])], // standalone (rare); bindings handled in if/while
        Expr::Macro(m) => macro_uses(&m.mac),
        _ => vec![], // Lit / Yield / Infer / Verbatim / Const block: nothing
    }
}

fn stmt_actions(stmt: &syn::Stmt) -> Vec<Action<Ru<'_>>> {
    match stmt {
        syn::Stmt::Local(local) => local_actions(local),
        syn::Stmt::Item(item) => item_actions(item),
        syn::Stmt::Expr(expr, _) => expr_actions(expr),
        syn::Stmt::Macro(m) => macro_uses(&m.mac),
    }
}

impl<'a> ScopeLang for RuLang<'a> {
    type Node = Ru<'a>;
    fn actions(&self, node: &Ru<'a>) -> Vec<Action<Ru<'a>>> {
        match *node {
            Ru::Item(i) => item_actions(i),
            Ru::Stmt(s) => stmt_actions(s),
            Ru::Expr(e) => expr_actions(e),
        }
    }
}
