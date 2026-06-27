//! ventouse — a minimizable code-quality metric for LLM coding agents.
//!
//! The library exposes the language-agnostic analysis `core`. The binary (`main.rs`) and
//! integration tests build on top of it.

pub mod config;
pub mod core;
pub mod discover;
pub mod lang;
pub mod render;
