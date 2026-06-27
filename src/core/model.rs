//! Shared finding vocabulary: the kind of entity a finding is about, and the stable reason codes.
//! (The reference graph itself lives in `core::defgraph`, built from the scope graph's outputs.)

/// What kind of named thing an entity is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum EntityKind {
    Function,
    Method,
    Class,
    /// The synthetic `<module>` entity for top-level code.
    Module,
    /// A name binding (used by the scope analysis).
    Binding,
}

/// A stable reason code for a finding (human wording lives in `render`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    /// Scope-debt: the binding could be pushed `n` nesting levels deeper (P5).
    ExcessLevels(u32),
    /// Scope-debt: `n` unrelated definitions wedged between it and what it connects to (P5).
    Misplaced(u32),
    /// A name used before its textual definition in the SAME scope (P5) — the finding's entity IS
    /// that name (a local read before its first binding, bottom-up data flow). Contrast
    /// [`ForwardRef`], where the entity is the *referrer* and the out-of-order callee is named.
    UseBeforeDecl,
    /// Declare-order (P5): this definition (the finding's entity) references `callee`, a sibling
    /// declared on the wrong side of it for the chosen reading convention. `below` = the callee is
    /// declared BELOW the referrer (a forward reference — bottom-up wants callees above); `!below` =
    /// the callee is ABOVE (top-down/stepdown wants callees below). The referrer is in place; the
    /// callee is the misplaced one, so it is named here, not the entity.
    ForwardRef { callee: String, below: bool },
    /// Suggestion: this definition is shared infrastructure — referenced `n` times by others in the
    /// file (broad fan-in, or heavy use from a few big callers). Extracting it into its own module
    /// turns those references cross-file (un-penalized) and localizes the rest.
    ExtractShared(u32),
    /// Suggestion: this function holds a bag of INDEPENDENT mutable accumulators carrying `n`
    /// scope-debt (they read nothing, yet wedge each other). Group them into a struct — that
    /// collapses the siblings into one and removes the mutual wedging.
    CrowdedScope(u32),
    /// Suggestion: this binding is declared far above where it is first used, with `wedged`
    /// unrelated definitions in between (all independent of it — the move is safe). Push the
    /// declaration down to its first use (line `first_use`) to close the gap. The actionable face
    /// of a large use-side wedge (the opposite extreme of [`UseBeforeDecl`]).
    ReorderBinding { first_use: u32, wedged: u32 },
    /// Suggestion: this binding is used only inside a block `levels` deeper than where it is declared
    /// (and never in a loop body, so the move is safe). Declaring it AT its first use (line
    /// `first_use`, inside that block) narrows its real scope — the actionable face of excess
    /// nesting levels, the levels-term twin of [`ReorderBinding`].
    NarrowToBlock { first_use: u32, levels: u32 },
    /// The file could not be parsed (carries the parser's message).
    ParseError(String),
}
