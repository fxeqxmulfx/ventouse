//! The language-agnostic raw IR a frontend produces (the handoff to the core). A `Frontend`
//! parses source into a [`RawModule`]; `core::lower` + `core::analyze` then build the call graph
//! and score locality (P5) — none of that touches a parser or an AST.
//!
//! There is a single reference graph: the frontend's scope analysis (`core::scopegraph`) emits the
//! definition-reference edges (`RawModule::def_edges`); the frontend does NOT walk calls separately.

/// What a def is, at the raw (pre-resolution) stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawKind {
    /// The synthetic `<module>` entity (top-level code).
    Module,
    Function,
    Method,
    Class,
    /// A module-/class-level value binding (a constant / attribute) — a DATA definition. Referenced
    /// like a definition (placement + declare-order), not narrowed like a function-local variable.
    Data,
}

/// One entity (function / method / class / `<module>`), derived from the scope graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawEntity {
    pub qualname: String,
    pub kind: RawKind,
    pub line: u32,
}

/// A scope-debt entry for one local binding (P5). Weights are applied later (at analysis time).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawScope {
    /// Qualname of the owning function/method.
    pub entity: String,
    pub name: String,
    pub line: u32,
    pub levels_excess: u32,
    /// Unrelated sibling definitions wedged between this binding and what it connects to —
    /// its dependencies (sources) and its first use (P5 locality).
    pub wedges: u32,
    /// The USE-SIDE subset of `wedges`: unrelated definitions between this binding's declaration
    /// and its first use. These are provably independent of the binding (anything that used it
    /// would push `first_use` up), so the binding can safely move down past them — the basis of the
    /// `ReorderBinding` suggestion.
    pub use_wedges: u32,
    /// The line of this binding's first lexical use in its own scope (0 if it has none below its
    /// declaration). Where `ReorderBinding` advises moving the declaration to.
    pub first_use: u32,
    /// This binding reads nothing (no RHS dependencies) — a candidate member of a "bag of
    /// independent mutable state" that the `CrowdedScope` suggestion advises bundling into a struct.
    pub independent: bool,
}

/// A parsed module: its entities + the scope-graph outputs (scope-debt, edges, warnings).
#[derive(Clone, Debug)]
pub struct RawModule {
    pub file: String,
    /// Dotted module path derived from the file (e.g. `pkg/a.py` → `pkg.a`).
    pub module: String,
    /// Entities (the synthetic `<module>` is `entities[?].kind == Module`).
    pub entities: Vec<RawEntity>,
    /// Scope-debt entries for local bindings (P5).
    pub scope: Vec<RawScope>,
    /// Definition-reference edges `(caller_qual, callee_qual)` (module-local qualnames), derived
    /// from the scope graph — the single source of the call graph.
    pub def_edges: Vec<(String, String)>,
    /// Use-before-binding warnings (P5): `(entity, line)` where a variable name is read before
    /// its first assignment in the same scope.
    pub decl_warnings: Vec<(String, u32)>,
}

/// A language frontend: source → [`RawModule`].
pub trait Frontend {
    fn parse_module(&self, src: &str, file: &str) -> Result<RawModule, String>;
}
