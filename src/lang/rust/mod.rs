//! The Rust frontend: parse with `syn`, lower each node into language-agnostic
//! `core::scopelang::Action`s, let the core build + score one `ScopeGraph` per file — exactly the
//! same core path as Python. The ONLY per-language code is the lowering.
//!
//! Rust is block-scoped: a `let` binding is local to its enclosing block, so the `levels` term
//! narrows the REAL runtime scope (it bites harder than in function-scoped Python). `impl T` maps
//! onto the core's "class" notion: methods become `T::method` entities and `self.`/`Self::` are
//! member references resolved in the impl scope. `const`/`static` are DATA definitions (like Python
//! module constants). Order-independence of items is irrelevant: declare-order is a *human* reading
//! signal (deps above users), applied to every language the same way.
//!
//! Macro invocations whose body parses as an expression list (`vec!`, `format!`, `assert_eq!`, …)
//! have their references recovered (`lower::macro_uses`); a body that isn't an expression list
//! (`matches!(x, Pat)`, a custom DSL) is the residual hole. Type references (signatures, generics)
//! are excluded, matching the references model's "annotations are excluded".
//!
//! Layout: `prim` is the lowering vocabulary (node type + `Action` constructors + pattern
//! primitives), `lower` the per-node rules + the `ScopeLang` dispatch; this module is parse + the
//! analysis pipeline. The vocabulary lives in its own file (on ventouse's own `ExtractShared`
//! suggestion) so the rules' references to it are cross-file and don't wedge.

mod lower;
mod prim;

use crate::core::finding::{Category, Finding, Severity};
use crate::core::model::{EntityKind, Reason};
use crate::core::scopelang::build;
use crate::core::score::Weights;
use crate::lang::{Frontend, RawEntity, RawKind, RawModule};

use prim::{Ru, RuLang, line};

/// Dotted module path from a file path: `src/core/a.rs` → `core.a`, `lib.rs` → `lib`.
fn module_name(file: &str) -> String {
    let f = file.strip_suffix(".rs").unwrap_or(file);
    let f = f.trim_start_matches("./").trim_start_matches("src/");
    f.replace(['/', '\\'], ".")
}

/// A `ParseError` finding for a file the frontend could not parse.
fn parse_error_finding(file: &str, msg: String) -> Finding {
    Finding {
        file: file.to_string(),
        line: 1,
        entity: "<module>".to_string(),
        entity_kind: EntityKind::Module,
        category: Category::ParseError,
        severity: Severity::Error,
        score: 0,
        reason: Reason::ParseError(msg),
    }
}

pub fn extract(src: &str, file: &str) -> Result<RawModule, String> {
    let parsed = syn::parse_file(src).map_err(|e| e.to_string())?;

    let roots: Vec<Ru> = parsed.items.iter().map(Ru::Item).collect();
    let mut out = build(&RuLang(std::marker::PhantomData), &roots).score();

    let module_line = parsed.items.first().map(line).unwrap_or(1);
    out.entities.push(RawEntity { qualname: "<module>".to_string(), kind: RawKind::Module, line: module_line });

    Ok(RawModule {
        file: file.to_string(),
        module: module_name(file),
        entities: out.entities,
        scope: out.debt,
        def_edges: out.edges,
        decl_warnings: out.decl_warnings,
    })
}

pub struct Rust;

impl Frontend for Rust {
    fn parse_module(&self, src: &str, file: &str) -> Result<RawModule, String> {
        extract(src, file)
    }
}

/// High-level: source → findings (parse + lower here; everything else is `core::analyze`).
pub fn analyze_source(src: &str, file: &str, weights: &Weights) -> Vec<Finding> {
    match extract(src, file) {
        Ok(m) => crate::core::analyze::analyze(&[m], weights),
        Err(msg) => vec![parse_error_finding(file, msg)],
    }
}

/// Whole-project analysis: parse + lower every `(file, src)` (a file that fails to parse yields a
/// `ParseError` finding and is skipped); the rest is `core::analyze`.
pub fn analyze_project(files: &[(&str, &str)], weights: &Weights) -> Vec<Finding> {
    let mut modules = Vec::new();
    let mut errors = Vec::new();
    for (file, src) in files {
        match extract(src, file) {
            Ok(m) => modules.push(m),
            Err(msg) => errors.push(parse_error_finding(file, msg)),
        }
    }
    let mut findings = crate::core::analyze::analyze(&modules, weights);
    findings.extend(errors);
    crate::core::analyze::sort_findings(&mut findings);
    findings
}

/// Convenience for tests: total scope-debt of a source string.
pub fn scope_of(src: &str) -> u32 {
    analyze_source(src, "test.rs", &Weights::default())
        .iter()
        .filter(|f| f.category == Category::ScopeDebt)
        .map(|f| f.score)
        .sum()
}

/// Convenience for tests: number of declare-before-use / declare-order warnings in a source string.
pub fn warnings_of(src: &str) -> usize {
    analyze_source(src, "test.rs", &Weights::default())
        .iter()
        .filter(|f| f.category == Category::DeclBeforeUse)
        .count()
}
