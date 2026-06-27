//! Language frontends. Each frontend parses source into the language-agnostic raw IR
//! ([`crate::core::raw`]); `core::lower` + `core::analyze` then resolve the call graph and score
//! locality (P5).
//!
//! Ships the Python (`python`), Rust (`rust`) and C++ (`cpp`) frontends. The raw IR + the `Frontend`
//! trait live in `core::raw` (re-exported here for convenience); this module only holds the
//! per-language frontends. All lower to the same `core::scopelang::Action`s and share the entire core.

pub mod cpp;
pub mod python;
pub mod rust;

// The raw IR is defined in the core (it is the language-agnostic contract); re-exported so
// `crate::lang::{RawModule, ...}` keeps resolving.
pub use crate::core::raw::{Frontend, RawEntity, RawKind, RawModule, RawScope};
