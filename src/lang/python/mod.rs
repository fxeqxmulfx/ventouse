//! The Python frontend: parse with the **ruff** parser (`ruff_python_parser`), extract a
//! [`RawModule`] (entity tree, owned-lines, same-module call references, scope-debt bindings, and
//! declare-before-use warnings), then `core::analyze` resolves the call graph and scores locality
//! (P5). The frontend extracts only structure — there is no effect/purity analysis.
//!
//! Layout: `prim` is the lowering vocabulary (node type + `Action` constructors + line index + AST
//! helpers), `lower` the per-node rules + the `ScopeLang` dispatch; this module is parse + the
//! analysis pipeline. The vocabulary lives in its own file (on ventouse's own `ExtractShared`
//! suggestion) so the rules' references to it are cross-file and don't wedge — like `lang/rust`.

mod lower;
mod prim;

use ruff_python_ast as ast;
use ruff_python_parser::parse_module;

use crate::core::finding::{Category, Finding, Severity};
use crate::core::model::{EntityKind, Reason};
use crate::core::scopegraph::ScopeOutput;
use crate::core::scopelang::build;
use crate::core::score::Weights;
use crate::lang::{Frontend, RawEntity, RawKind, RawModule};

use prim::{LineIndex, Py, PyLang, start_row};

pub struct Python;

impl Frontend for Python {
    fn parse_module(&self, src: &str, file: &str) -> Result<RawModule, String> {
        extract(src, file)
    }
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

/// Dotted module path from a file path: `pkg/a.py` → `pkg.a`, `test.py` → `test`.
fn module_name(file: &str) -> String {
    let f = file.strip_suffix(".py").unwrap_or(file);
    let f = f.trim_start_matches("./");
    f.replace(['/', '\\'], ".")
}

/// Whole-module scope analysis: lower the AST via the Python profile, let the core build + score.
/// Returns the value scope-debt, the definition-reference edges, AND the entity list — everything
/// the locality analysis needs, from one walk.
fn scope_analysis(idx: &LineIndex, suite: &[ast::Stmt]) -> ScopeOutput {
    let roots: Vec<Py> = suite.iter().map(Py::Stmt).collect();
    build(&PyLang { idx }, &roots).score()
}

pub fn extract(src: &str, file: &str) -> Result<RawModule, String> {
    let parsed = parse_module(src).map_err(|e| e.to_string())?;
    let suite = &parsed.syntax().body;
    let idx = LineIndex::new(src);

    // ONE walk: value scope-debt + definition-reference edges + entities + declare-before-use.
    let mut out = scope_analysis(&idx, suite);
    // The synthetic `<module>` entity (top-level code), at the first statement's line.
    let module_line = suite.first().map(|s| start_row(&idx, s)).unwrap_or(1);
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

/// High-level: source → findings (parse + extract here; everything else is `core::analyze`).
pub fn analyze_source(src: &str, file: &str, weights: &Weights) -> Vec<Finding> {
    match extract(src, file) {
        Ok(m) => crate::core::analyze::analyze(&[m], weights),
        Err(msg) => vec![parse_error_finding(file, msg)],
    }
}

/// Whole-project analysis: parse + extract every `(file, src)` (a file that fails to parse yields
/// a `ParseError` finding and is skipped); the rest is `core::analyze`.
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
    analyze_source(src, "test.py", &Weights::default())
        .iter()
        .filter(|f| f.category == Category::ScopeDebt)
        .map(|f| f.score)
        .sum()
}

/// Convenience for tests: number of declare-before-use warnings in a source string.
pub fn warnings_of(src: &str) -> usize {
    analyze_source(src, "test.py", &Weights::default())
        .iter()
        .filter(|f| f.category == Category::DeclBeforeUse)
        .count()
}
