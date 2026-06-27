//! The language-agnostic **scope graph** — the L2 substrate: bindings + references over a
//! block/scope tree with textual order. A frontend BUILDS one (mapping its syntax to
//! bindings/uses/scopes via its language profile); [`ScopeGraph::score`] then computes locality
//! (P5) language-agnostically. The computation lives HERE, not in any frontend, so every language
//! shares one metric — new frontends only emit the graph, they don't re-derive the rules.
//!
//! Locality score (L1) per value binding = `levels_excess + wedges`:
//! - **levels_excess** — how far the declaration could move DOWN toward the narrowest block (LCA)
//!   covering its uses, never crossing a loop boundary (C1: loop-carried state stays outside).
//! - **wedges** — unrelated sibling definitions wedged between the binding and its dependencies
//!   (above) or its first use (below). A sibling sharing a dependency, or co-used at the first-use
//!   site, is the same cluster → free.
//!
//! Declarations of code (def/class/import) are FREE of nesting (C2) — not scored here.

use std::collections::{BTreeMap, HashSet};

use crate::core::raw::{RawEntity, RawKind, RawScope};
use crate::core::wedge;

/// The kind of a binding (a language-profile concept; the frontend classifies each binding site).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BindKind {
    /// A value binding (variable) — narrowable; scored for levels + wedges.
    Value,
    /// A declaration of reusable code (def/class) — FREE of nesting (C2); not scored here.
    Decl,
    /// An import — a declaration; free.
    Import,
    /// A parameter — excluded.
    Param,
}

struct Binding {
    kind: BindKind,
    decl_block: usize,
    decl_line: u32,
    /// An un-narrowable introducer (loop/`with`/`except`/walrus target) — no levels penalty.
    intro: bool,
    /// Referenced via `global`/`nonlocal` somewhere — intentional shared state, never narrowed.
    pinned: bool,
    /// For a `Decl`: is it a class (vs a function/method)? Refined to Method by a class scope.
    is_class_def: bool,
    /// (block, line, use_scope) — populated from `uses` at scoring time.
    uses: Vec<(usize, u32, usize)>,
    /// Names this binding's defining RHS reads — its dependencies (for wedges).
    deps: Vec<String>,
}

struct Scope {
    parent: Option<usize>,
    qualname: String,
    is_module: bool,
    /// A class body — `self`/`cls` member references resolve to its bindings (no fall-through).
    is_class: bool,
    /// A namespace / module-block (C++ `namespace`, Rust `mod`) — a definition CONTAINER, not a
    /// runtime frame: its value bindings are top-level DATA (constants/globals), not narrowable
    /// locals, exactly like the module scope. It keeps its own qualname prefix, though (unlike the
    /// root module), so `ns1::f` and `ns2::f` stay distinct.
    is_namespace: bool,
    /// Names declared `global`/`nonlocal` here — NOT local bindings; a write to one is a use of the
    /// outer binding (so global state read/written across functions is not narrowed into one).
    nonlocals: HashSet<String>,
    bindings: BTreeMap<String, Binding>,
}

/// A reference recorded during the walk, resolved to a binding at scoring time. EVERY reference to
/// a same-module definition becomes a graph edge (the references model, L5) — not only calls.
struct Use {
    name: String,
    scope: usize,
    block: usize,
    line: u32,
    /// A `self.`/`cls.` member reference — resolves to the enclosing class scope only.
    member: bool,
    /// Attribution override: a header reference (decorator / base / default) is resolved in the
    /// enclosing scope but ATTRIBUTED to the entity being defined. `None` → the use's own scope.
    referrer: Option<String>,
}

/// Everything the locality analysis needs, produced from the single scope graph in one pass.
#[derive(Default)]
pub struct ScopeOutput {
    /// Value-binding locality debt (levels + wedges).
    pub debt: Vec<RawScope>,
    /// Definition-reference edges `(referrer_qual, referent_qual)` — the one call graph.
    pub edges: Vec<(String, String)>,
    /// The entity list (every `Decl` binding → function / method / class).
    pub entities: Vec<RawEntity>,
    /// Declare-before-use warnings for VALUES: `(entity_qual, use_line)` where a variable is read
    /// before its first binding in the same scope.
    pub decl_warnings: Vec<(String, u32)>,
}

/// One connected block/scope tree spanning a whole module, plus the reference (use) list.
pub struct ScopeGraph {
    /// (depth, parent block, is_loop) — `is_loop` marks for/while bodies (no narrowing into them).
    blocks: Vec<(u32, Option<usize>, bool)>,
    scopes: Vec<Scope>,
    uses: Vec<Use>,
}

impl Default for ScopeGraph {
    fn default() -> Self {
        ScopeGraph { blocks: vec![(0, None, false)], scopes: Vec::new(), uses: Vec::new() }
    }
}

impl ScopeGraph {
    pub fn new() -> ScopeGraph {
        ScopeGraph::default()
    }

    /// The root (module) block — depth 0.
    pub fn module_block(&self) -> usize {
        0
    }

    #[allow(clippy::too_many_arguments)] // mirrors the fields of a `Scope` — a builder method
    pub fn new_scope(
        &mut self,
        parent: Option<usize>,
        qualname: String,
        is_module: bool,
        is_class: bool,
        is_namespace: bool,
        nonlocals: Vec<String>,
    ) -> usize {
        self.scopes.push(Scope {
            parent,
            qualname,
            is_module,
            is_class,
            is_namespace,
            nonlocals: nonlocals.into_iter().collect(),
            bindings: BTreeMap::new(),
        });
        self.scopes.len() - 1
    }

    /// A definition CONTAINER (module root, class body, or namespace/`mod`): a value bound here is a
    /// top-level DATA definition (constant / attribute), free of nesting — NOT a narrowable local.
    fn holds_defs(&self, sid: usize) -> bool {
        let s = &self.scopes[sid];
        s.is_module || s.is_class || s.is_namespace
    }

    pub fn new_block(&mut self, parent: usize, is_loop: bool) -> usize {
        let depth = self.blocks[parent].0 + 1;
        self.blocks.push((depth, Some(parent), is_loop));
        self.blocks.len() - 1
    }

    pub fn add_use(&mut self, name: &str, scope: usize, block: usize, line: u32, member: bool, referrer: Option<String>) {
        self.uses.push(Use { name: name.to_string(), scope, block, line, member, referrer });
    }

    #[allow(clippy::too_many_arguments)] // mirrors the fields of `Action::Bind` — a builder method
    pub fn bind(&mut self, scope: usize, name: &str, kind: BindKind, block: usize, line: u32, intro: bool, is_class_def: bool) {
        // A `global`/`nonlocal` name is not a local binding — the write is a use of the outer one.
        if kind == BindKind::Value && self.scopes[scope].nonlocals.contains(name) {
            self.add_use(name, scope, block, line, false, None);
            return;
        }
        let e = self.scopes[scope].bindings.entry(name.to_string()).or_insert(Binding {
            kind,
            decl_block: block,
            decl_line: line,
            intro,
            pinned: false,
            is_class_def,
            uses: Vec::new(),
            deps: Vec::new(),
        });
        if line < e.decl_line {
            // first-binding rule: keep the earliest declaration
            e.decl_block = block;
            e.decl_line = line;
            e.intro = intro;
        }
    }

    pub fn set_deps(&mut self, scope: usize, name: &str, deps: &[String]) {
        if let Some(b) = self.scopes[scope].bindings.get_mut(name)
            && b.deps.is_empty()
        {
            b.deps = deps.to_vec();
        }
    }

    /// The qualname for a child scope `name` declared in `parent`.
    pub fn child_qual(&self, parent: usize, name: &str) -> String {
        let p = &self.scopes[parent];
        if p.is_module {
            name.to_string()
        } else {
            format!("{}.{}", p.qualname, name)
        }
    }

    /// Where to attribute a value's RHS references. A definition-container value (module/class/
    /// namespace level) is a DATA definition, so its RHS belongs to it (the value's qualname). A
    /// function-local value is not a definition — its RHS references belong to the enclosing
    /// function (the scope itself).
    pub fn data_attrib(&self, scope: usize, name: &str) -> String {
        if self.holds_defs(scope) {
            self.child_qual(scope, name)
        } else {
            self.scopes[scope].qualname.clone()
        }
    }

    // --- scoring (language-agnostic) ----------------------------------------------------

    /// Is `name` in scope `sid` a DEFINITION — a function/class (`Decl`), or a container-level
    /// value (a constant / attribute = data definition)? A function-local variable is not.
    fn is_definition(&self, sid: usize, name: &str) -> bool {
        match self.scopes[sid].bindings.get(name).map(|b| b.kind) {
            Some(BindKind::Decl) => true,
            Some(BindKind::Value) => self.holds_defs(sid),
            _ => false,
        }
    }

    /// The module-local qualname of binding `name` declared in scope `bscope`.
    fn qual_of(&self, bscope: usize, name: &str) -> String {
        if self.scopes[bscope].is_module {
            name.to_string()
        } else {
            format!("{}.{}", self.scopes[bscope].qualname, name)
        }
    }

    /// Resolve a `self.`/`cls.` member reference: the NEAREST enclosing class scope, looked up
    /// there only (no fall-through to module/globals — an instance attribute is not a binding).
    fn resolve_member(&self, name: &str, from_scope: usize) -> Option<usize> {
        let mut s = Some(from_scope);
        while let Some(id) = s {
            if self.scopes[id].is_class {
                return self.scopes[id].bindings.contains_key(name).then_some(id);
            }
            s = self.scopes[id].parent;
        }
        None
    }

    /// The deepest block a binding declared in `decl` could move down to, given uses bottoming at
    /// `min_block`, WITHOUT crossing a loop boundary. The highest loop on the path wins.
    fn narrow_target(&self, min_block: usize, decl: usize) -> usize {
        let decl_depth = self.blocks[decl].0;
        let mut target = min_block;
        let mut node = min_block;
        while self.blocks[node].0 > decl_depth {
            if self.blocks[node].2 {
                target = self.blocks[node].1.unwrap(); // can't enter this loop; cap above it
            }
            node = self.blocks[node].1.unwrap();
        }
        target
    }

    /// Is block `anc` the same as, or an ancestor of, block `node`?
    fn block_encloses(&self, anc: usize, mut node: usize) -> bool {
        loop {
            if node == anc {
                return true;
            }
            match self.blocks[node].1 {
                Some(p) => node = p,
                None => return false,
            }
        }
    }

    fn lca(&self, mut a: usize, mut b: usize) -> usize {
        while self.blocks[a].0 > self.blocks[b].0 {
            a = self.blocks[a].1.unwrap();
        }
        while self.blocks[b].0 > self.blocks[a].0 {
            b = self.blocks[b].1.unwrap();
        }
        while a != b {
            a = self.blocks[a].1.unwrap();
            b = self.blocks[b].1.unwrap();
        }
        a
    }

    fn resolve_scope(&self, name: &str, scope: usize) -> Option<usize> {
        let mut s = Some(scope);
        let mut start = true;
        while let Some(id) = s {
            // A nested function does NOT see the enclosing CLASS body's names lexically (Python
            // scoping) — class-body names are reached only via `self.`/`cls.` (resolve_member).
            // The class body itself (the starting scope) does see its own names.
            let visible = start || !self.scopes[id].is_class;
            if visible && self.scopes[id].bindings.contains_key(name) {
                return Some(id);
            }
            start = false;
            s = self.scopes[id].parent;
        }
        None
    }

    /// Is `sid` a NAMED definition scope (module / class / namespace, or a `def`/`fn`/`class` body —
    /// i.e. its leaf is a `Decl` binding in its parent), as opposed to an ANONYMOUS body (lambda /
    /// comprehension / closure, whose leaf — `<lambda>`, `<listcomp>`, `{closure}` — binds nothing)?
    fn is_named_scope(&self, sid: usize) -> bool {
        let s = &self.scopes[sid];
        if s.is_module || s.is_class || s.is_namespace {
            return true;
        }
        let leaf = s.qualname.rsplit('.').next().unwrap_or(&s.qualname);
        match s.parent {
            Some(p) => matches!(self.scopes[p].bindings.get(leaf).map(|b| b.kind), Some(BindKind::Decl)),
            None => true,
        }
    }

    /// Where a reference made in scope `sid` is ATTRIBUTED for the definition-reference graph: the
    /// nearest enclosing NAMED definition. A reference buried in a lambda/comprehension/closure body
    /// is read as a use by the function that contains it (you meet it reading that function), so the
    /// referenced definition is placed/ordered against it — closing the locality blind spot where a
    /// callee used only inside a closure escaped the graph. Value-capture narrowing is unaffected: it
    /// runs off `uses` (block/line), not this attribution.
    fn attribution_scope(&self, mut sid: usize) -> usize {
        while !self.is_named_scope(sid) {
            match self.scopes[sid].parent {
                Some(p) => sid = p,
                None => break,
            }
        }
        sid
    }

    /// gap-to-deps + gap-to-use for a value (P5 locality): unrelated sibling definitions wedged
    /// between the variable and its dependencies (above) or its first use (below). "Unrelated" =
    /// not a dependency and not sharing a dependency; on the use side, also not co-used at the
    /// variable's first-use site. Returns `(total, use_side)` — the use-side subset is the part a
    /// `ReorderBinding` reorder (push the declaration down to its use) would remove.
    fn var_wedges(
        &self,
        scope: &Scope,
        name: &str,
        b: &Binding,
        first_use: &BTreeMap<&str, u32>,
    ) -> (u32, u32) {
        let target_deps: HashSet<&str> = b.deps.iter().map(String::as_str).collect();
        // candidate siblings: same scope, not a parameter, not the target itself, AND in a block
        // that ENCLOSES the target (same block or an ancestor). A binding in a cousin block — a
        // different, mutually-exclusive branch (e.g. another `match` arm) — is not on any execution
        // path from the target to its dependency/use, so it cannot be a wedge.
        let sibs: Vec<wedge::Sib<&str>> = scope
            .bindings
            .iter()
            .filter(|(gn, g)| {
                g.kind != BindKind::Param && gn.as_str() != name && self.block_encloses(g.decl_block, b.decl_block)
            })
            .map(|(gn, g)| wedge::Sib {
                key: gn.as_str(),
                line: g.decl_line,
                deps: g.deps.iter().map(String::as_str).collect(),
            })
            .collect();

        // dep-side: between the nearest dependency above and the declaration.
        let dep = wedge::dep_side(b.decl_line, &target_deps, &sibs);
        // use-side: between the declaration and the first use, exempting siblings co-used there.
        let use_ = match first_use.get(name).copied().filter(|&h| h > b.decl_line) {
            Some(hi) => wedge::between(b.decl_line, hi, &target_deps, &sibs, |k| first_use.get(*k) == Some(&hi)),
            None => 0,
        };
        (dep + use_, use_)
    }

    /// Score the graph — the single source of everything the locality analysis needs (last: every
    /// resolver / helper it calls is defined above):
    /// - the value-binding locality debt (`RawScope`s);
    /// - the definition-reference edges (`(referrer_qual, referent_qual)` per CALL to a `Decl`) —
    ///   the one call graph, so resolution (incl. shadowing) is consistent;
    /// - the entity list (every `Decl` binding → function/method/class), so no second AST walk is
    ///   needed to enumerate definitions.
    pub fn score(mut self) -> ScopeOutput {
        let uses = std::mem::take(&mut self.uses);
        let mut edges = Vec::new();
        for u in uses {
            let resolved = if u.member {
                self.resolve_member(&u.name, u.scope)
            } else {
                self.resolve_scope(&u.name, u.scope)
            };
            let Some(bscope) = resolved else { continue };
            // ANY reference to a DEFINITION is a graph edge (references model). A definition is a
            // function/class (`Decl`) OR a module-/class-level value (a constant / attribute = DATA
            // definition). A function-local variable is not a definition — its references aren't edges.
            if self.is_definition(bscope, &u.name) {
                let referrer = u.referrer.clone().unwrap_or_else(|| {
                    self.scopes[self.attribution_scope(u.scope)].qualname.clone()
                });
                edges.push((referrer, self.qual_of(bscope, &u.name)));
            }
            // A `self.`/`cls.` member access is a reference (an edge), but NOT a lexical use — you
            // can't move a class attribute into the method that reads `self.x`. So member uses do
            // not feed value scope-debt; only plain (lexical) uses do.
            let pin = self.scopes[u.scope].nonlocals.contains(&u.name);
            if !u.member
                && let Some(b) = self.scopes[bscope].bindings.get_mut(&u.name)
            {
                b.uses.push((u.block, u.line, u.scope));
                if pin {
                    b.pinned = true; // a global/nonlocal reference → intentional shared state
                }
            }
        }

        let mut out = Vec::new();
        let mut entities = Vec::new();
        let mut decl_warnings = Vec::new();
        for (sid, scope) in self.scopes.iter().enumerate() {
            // first use line of each binding IN THIS scope (for the use-side co-use exemption)
            let first_use: BTreeMap<&str, u32> = scope
                .bindings
                .iter()
                .filter_map(|(n, b)| {
                    b.uses.iter().filter(|(_, _, us)| *us == sid).map(|(_, l, _)| *l).min().map(|l| (n.as_str(), l))
                })
                .collect();

            // A definition container (module / class / namespace) → bindings here are top-level
            // DEFINITIONS (code or data); inside a function they are local variables (which narrow).
            let module_or_class = scope.is_module || scope.is_class || scope.is_namespace;
            for (name, b) in &scope.bindings {
                // Each definition → an entity. `Decl` is code (function/method/class); a container-
                // level value is DATA (a constant / attribute). Both are placed + ordered by
                // `core::placement` / `core::declorder`; neither narrows.
                match b.kind {
                    BindKind::Decl => {
                        let kind = if b.is_class_def {
                            RawKind::Class
                        } else if scope.is_class {
                            RawKind::Method
                        } else {
                            RawKind::Function
                        };
                        entities.push(RawEntity { qualname: self.qual_of(sid, name), kind, line: b.decl_line });
                    }
                    BindKind::Value if module_or_class => {
                        entities.push(RawEntity { qualname: self.qual_of(sid, name), kind: RawKind::Data, line: b.decl_line });
                    }
                    _ => {}
                }
                // Scope-debt (levels + wedges + use-before-binding) is for FUNCTION-LOCAL variables
                // only. Definitions (code + module/class data) are free of nesting (C2).
                if b.kind != BindKind::Value || module_or_class || b.uses.is_empty() {
                    continue;
                }
                // Declare-before-use (P5): a same-scope use earlier than the first binding.
                if let Some(&fu) = first_use.get(name.as_str())
                    && fu < b.decl_line
                {
                    decl_warnings.push((format!("{}.{}", scope.qualname, name), fu));
                }
                // levels: LCA over ALL uses — a module variable used in a single function pins
                // down into it; one used in two functions can't narrow (LCA is the module) → 0.
                let mut min_block = b.uses[0].0;
                for (blk, _, _) in &b.uses[1..] {
                    min_block = self.lca(min_block, *blk);
                }
                let levels = if b.intro || b.pinned {
                    0
                } else {
                    let target = self.narrow_target(min_block, b.decl_block);
                    self.blocks[target].0.saturating_sub(self.blocks[b.decl_block].0)
                };
                // An `intro` binding (loop/`with`/`except`/walrus target) is positionally fixed by
                // the construct that introduces it — it can't be reordered, so it accrues no wedges
                // (just as it accrues no levels). Otherwise unrelated siblings inside the loop would
                // "wedge" the loop variable, which one cannot move.
                let (wedges, mut use_wedges) =
                    if b.intro { (0, 0) } else { self.var_wedges(scope, name, b, &first_use) };
                // ReorderBinding (use_wedges) advises pushing the declaration DOWN to its first use.
                // That is only safe if the first use can be reached without crossing a loop boundary:
                // a loop-carried accumulator's first use is INSIDE the loop, and moving its `= 0` seed
                // in there would reset it every iteration. When the move is unsafe, drop the reorder
                // signal (the wedge debt still stands — bundling, i.e. CrowdedScope, is the fix).
                let fu_block = b.uses.iter().filter(|(_, _, us)| *us == sid).min_by_key(|(_, l, _)| *l).map(|(blk, _, _)| *blk);
                if let Some(fb) = fu_block
                    && self.narrow_target(fb, b.decl_block) != fb
                {
                    use_wedges = 0;
                }
                if levels == 0 && wedges == 0 {
                    continue;
                }
                out.push(RawScope {
                    entity: format!("{}.{}", scope.qualname, name),
                    name: name.clone(),
                    line: b.decl_line,
                    levels_excess: levels,
                    wedges,
                    use_wedges,
                    first_use: first_use.get(name.as_str()).copied().unwrap_or(0),
                    independent: b.deps.is_empty(),
                });
            }
        }
        ScopeOutput { debt: out, edges, entities, decl_warnings }
    }
}
