//! Tunable constants (P4). Every threshold the analysis uses lives here, with a default; all are
//! overridable via `[tool.ventouse.weights]` config (`crate::config`). One unit (`scope_level`) per
//! scope-debt unit.

/// Which reading convention the declare-order warning enforces — the one genuinely conventional
/// axis (compiler-free function ordering). Locality/wedges and value use-before-binding do NOT
/// depend on it (data flow is invariant); only callee-vs-caller direction does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclOrder {
    /// Callees ABOVE callers — define before use. A reference to a definition declared later is a
    /// forward reference (warned). The default.
    BottomUp,
    /// Callers ABOVE callees — stepdown / overview-first. A definition declared above the code that
    /// uses it is the out-of-order one (warned).
    TopDown,
}

/// All tunable analysis constants. Defaults are arbitrary placeholders (P4); override per-project in
/// `[tool.ventouse.weights]`. The thresholds are NOT derived from a principle — they were tuned by
/// dogfooding and live here precisely so a project can retune them without code changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Weights {
    /// Per scope-debt unit (an excess nesting level, or an unrelated wedged definition).
    pub scope_level: u32,
    /// Reading convention for the declare-order warning (default bottom-up).
    pub order: DeclOrder,
    /// `ExtractShared`: a definition referenced by ≥ this many DISTINCT others is shared (broad fan-in).
    pub shared_callers: usize,
    /// `ExtractShared`: …OR referenced this many TIMES in total by ≥2 others (heavy use from a few).
    pub shared_uses: u32,
    /// `CrowdedScope`: a function whose INDEPENDENT locals carry at least this much scope-debt.
    pub crowded_scope: u32,
    /// `ReorderBinding`: a binding/definition with at least this many use-side wedges is "scattered".
    pub reorder_wedges: u32,
    /// `NarrowToBlock`: a local used at least this many levels deeper than declared is flagged.
    pub narrow_levels: u32,
    /// Accessor pinning: a class-member leaf used by ≥ this many distinct siblings is de-facto data.
    pub accessor_fanin: usize,
}

impl Default for Weights {
    fn default() -> Weights {
        Weights {
            scope_level: 10,
            order: DeclOrder::BottomUp,
            shared_callers: 4,
            shared_uses: 8,
            crowded_scope: 100,
            reorder_wedges: 3,
            narrow_levels: 1,
            accessor_fanin: 3,
        }
    }
}
