//! The TypeScript / JavaScript frontend: parse with `oxc`, lower each node into language-agnostic
//! `core::scopelang::Action`s, let the core build + score one `ScopeGraph` per file — the same core
//! path as Python / Rust / C++. The ONLY per-language code is the lowering (`lower`/`prim`).
//!
//! TS/JS is block-scoped (`let`/`const`), so `levels` narrows the real runtime scope. `.tsx`/`.jsx`
//! enable JSX (React): a component reference in a tag and the `{ … }` expression containers feed the
//! reference graph, so a handler/sub-component used only deep in returned JSX still counts. Types
//! (annotations, interfaces, type aliases) are excluded, matching the references model.

mod lower;
mod prim;

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::core::finding::{Category, Finding, Severity};
use crate::core::model::{EntityKind, Reason};
use crate::core::scopelang::build;
use crate::core::score::Weights;
use crate::lang::{Frontend, RawEntity, RawKind, RawModule};

use prim::{LineIndex, Ts, TsLang};

/// Dotted module path from a file path: `src/api/user.ts` → `api.user`, `app.tsx` → `app`.
fn module_name(file: &str) -> String {
    let f = file
        .strip_suffix(".tsx")
        .or_else(|| file.strip_suffix(".ts"))
        .or_else(|| file.strip_suffix(".jsx"))
        .or_else(|| file.strip_suffix(".js"))
        .unwrap_or(file);
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
    let source_type = SourceType::from_path(file).unwrap_or_else(|_| SourceType::tsx());
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, src, source_type).parse();
    if ret.panicked {
        let msg = ret.diagnostics.iter().next().map(ToString::to_string).unwrap_or_else(|| "parse failed".to_string());
        return Err(msg);
    }

    let lines = LineIndex::new(src);
    let lang = TsLang { lines: &lines };
    let roots: Vec<Ts> = ret.program.body.iter().map(Ts::Stmt).collect();
    let mut out = build(&lang, &roots).score();

    let module_line = ret.program.body.first().map(|s| lines.line(oxc_span::GetSpan::span(s).start)).unwrap_or(1);
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

pub struct TypeScript;

impl Frontend for TypeScript {
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
    analyze_source(src, "test.ts", &Weights::default())
        .iter()
        .filter(|f| f.category == Category::ScopeDebt)
        .map(|f| f.score)
        .sum()
}

/// Convenience for tests: number of declare-before-use / declare-order warnings in a source string.
pub fn warnings_of(src: &str) -> usize {
    analyze_source(src, "test.ts", &Weights::default())
        .iter()
        .filter(|f| f.category == Category::DeclBeforeUse)
        .count()
}
