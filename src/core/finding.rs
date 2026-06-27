//! The core's render-agnostic output: a flat, sorted `Vec<Finding>`.
//!
//! The core NEVER formats display strings — it emits structured findings carrying a stable
//! reason code; the `render` module turns these into text/json.

use crate::core::model::{EntityKind, Reason};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    /// Locality penalty (excess nesting or a wedged definition).
    ScopeDebt,
    /// A name used before its textual definition (callee below caller).
    DeclBeforeUse,
    /// An actionable suggestion (not a metric penalty) — e.g. extract shared infrastructure.
    Suggestion,
    ParseError,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// One structured finding. Human wording lives in `render`, not here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub file: String,
    pub line: u32,
    pub entity: String,
    pub entity_kind: EntityKind,
    pub category: Category,
    pub severity: Severity,
    /// Metric score (ScopeDebt); 0 for non-metric findings.
    pub score: u32,
    pub reason: Reason,
}
