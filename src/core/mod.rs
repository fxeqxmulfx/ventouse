//! The language-agnostic analysis core: build a call graph (`lower`) then score locality (P5) —
//! scope-debt (`placement` + the binding levels/wedges) and declare-before-use (`declorder`).
//! Produces a render-agnostic `Vec<Finding>`; the `render` module handles display.

pub mod analyze;
pub mod declorder;
pub mod defgraph;
pub mod finding;
pub mod model;
pub mod placement;
pub mod raw;
pub mod scopegraph;
pub mod scopelang;
pub mod score;
pub mod wedge;

pub use finding::{Category, Finding, Severity};
pub use model::{EntityKind, Reason};
pub use score::{DeclOrder, Weights};
