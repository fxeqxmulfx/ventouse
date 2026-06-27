//! The C++ frontend: parse with **libclang** (the `clang` crate), lower each cursor into
//! language-agnostic `core::scopelang::Action`s, and let the core build + score one `ScopeGraph`
//! per file — exactly the same core path as Python/Rust. The ONLY per-language code is the lowering.
//!
//! libclang gives a real, semantically-resolved AST: a single childless `MemberRefExpr` is a `this.`
//! member access (resolved in the enclosing class), an access through an object carries that object,
//! and a static member read is a plain `DeclRefExpr` — so member resolution falls out of the cursor
//! shape. C++ is block-scoped, so the `levels` term narrows the REAL runtime scope (a `let`-like
//! local used only inside an `if` could move in). Unresolved `#include`s only cost some cross-module
//! references (which are dropped anyway); the file still parses, so the analysis is unaffected.
//!
//! Layout mirrors `lang/rust`: `prim` is the lowering vocabulary, `lower` the per-cursor rules + the
//! `ScopeLang` dispatch; this module is parse + the analysis pipeline.
//!
//! libclang has a process-global constraint — only one `clang::Clang` may exist at a time — so
//! `extract` serializes on a `Mutex` and builds the whole owned `RawModule` while the translation
//! unit (which the cursors borrow) is still alive, before releasing. If no libclang can be loaded,
//! parsing yields a `ParseError` finding rather than panicking.

mod compdb;
mod lower;
mod prim;

use std::marker::PhantomData;
use std::sync::Mutex;

use clang::{Clang, Index, Unsaved};

use crate::core::finding::{Category, Finding, Severity};
use crate::core::model::{EntityKind, Reason};
use crate::core::scopelang::build;
use crate::core::score::Weights;
use crate::lang::{Frontend, RawEntity, RawKind, RawModule};

use prim::CppLang;

/// Serializes libclang use: only one `clang::Clang` may exist per process at a time. The guard is
/// held for all of `extract` so the translation unit outlives the cursors lowered from it.
static CLANG_LOCK: Mutex<()> = Mutex::new(());

/// Dotted module path from a file path: `src/net/sock.cpp` → `net.sock`, `main.cpp` → `main`.
fn module_name(file: &str) -> String {
    let stem = ["cpp", "cc", "cxx", "hpp", "hh", "hxx", "h"]
        .iter()
        .find_map(|ext| file.strip_suffix(&format!(".{ext}")))
        .unwrap_or(file);
    let stem = stem.trim_start_matches("./").trim_start_matches("src/");
    stem.replace(['/', '\\'], ".")
}

/// A `ParseError` finding for a file the frontend could not parse (or had no libclang to parse with).
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

/// Base compiler arguments. `-x c++` forces C++ SOURCE mode for every input — without it libclang
/// infers the language from the extension and treats a `.h`/`.hpp` as a header-to-precompile, which
/// fails to parse standalone ("AST deserialization failed"), yet headers are exactly where C++
/// class/method declarations live. Project `-I` paths are appended by `parse_all`.
const BASE_ARGS: &[&str] = &["-x", "c++", "-std=c++17"];

/// Parse + lower ONE file against an EXISTING `Clang` (no instance creation, no locking), with the
/// given compiler `args`. The cursors borrow the translation unit, so the whole owned `RawModule` is
/// built before `tu` drops.
fn parse_with(clang: &Clang, src: &str, file: &str, args: &[&str]) -> Result<RawModule, String> {
    let index = Index::new(clang, false, false);
    let tu = index
        .parser(file)
        .arguments(args)
        .unsaved(&[Unsaved::new(file, src)])
        .parse()
        .map_err(|e| e.to_string())?;

    // Top-level cursors declared in this file (skip everything pulled in by `#include`).
    let root = tu.get_entity();
    let roots: Vec<_> = root.get_children().into_iter().filter(clang::Entity::is_in_main_file).collect();

    let mut out = build(&CppLang(PhantomData), &roots).score();
    let module_line = roots.first().map(|c| c.get_location().map(|l| l.get_spelling_location().line).unwrap_or(1)).unwrap_or(1);
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

pub fn extract(src: &str, file: &str) -> Result<RawModule, String> {
    // Hold the lock for the whole call: the cursors lowered in `parse_with` borrow the translation
    // unit, which borrows this `Clang` — so it must outlive the lowering.
    let _guard = CLANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let clang = Clang::new().map_err(|e| format!("libclang unavailable: {e}"))?;
    parse_with(&clang, src, file, BASE_ARGS)
}

/// The deepest directory containing ALL `files` (the project-ish root that project-relative
/// `#include "pkg/foo.h"` resolves from). `None`/empty when the files share no real prefix.
fn common_ancestor(files: &[(&str, &str)]) -> Option<String> {
    let dirs: Vec<Vec<&str>> =
        files.iter().filter_map(|(f, _)| std::path::Path::new(f).parent()?.to_str().map(|d| d.split('/').collect())).collect();
    let (first, rest) = dirs.split_first()?;
    let mut prefix = first.clone();
    for d in rest {
        let common = prefix.iter().zip(d).take_while(|(a, b)| a == b).count();
        prefix.truncate(common);
    }
    let root = prefix.join("/");
    (!root.is_empty()).then_some(root)
}

/// Heuristic `-I` include directories for the run: the files' common-ancestor root (where
/// project-relative includes resolve from) plus conventional `include/` / `src/` subdirs when they
/// exist. Resolving the project's own headers stops libclang from degrading the AST on unknown types
/// (dropped references), which keeps locality (uses) accurate. (A `compile_commands.json` would be
/// the precise source; this covers the common project-rooted layout.)
fn include_dirs(files: &[(&str, &str)]) -> Vec<String> {
    let Some(root) = common_ancestor(files) else { return Vec::new() };
    let mut dirs = vec![root.clone()];
    for sub in ["include", "src"] {
        let p = format!("{root}/{sub}");
        if std::path::Path::new(&p).is_dir() {
            dirs.push(p);
        }
    }
    dirs
}

/// Shares one `Clang` across the parallel-parsing worker threads. libclang supports concurrent
/// parsing as long as each thread uses its OWN `Index` (we create one per file) — the `Clang` only
/// gates the one-instance-per-process rule, so handing out `&Clang` to make indexes from is sound.
/// The `clang` crate marks `Clang` `!Send`/`!Sync` to protect single-threaded users; this wrapper
/// opts into the documented multi-index threading (verified: parallel output == serial output).
struct SharedClang<'c>(&'c Clang);
// SAFETY: only used to create per-thread `Index`es and parse distinct files concurrently — the
// libclang usage pattern its own docs call thread-safe. No cursor/TU ever crosses a thread.
unsafe impl Sync for SharedClang<'_> {}

/// Parse + lower every file, in PARALLEL across worker threads, returning results in input order.
/// libclang parsing dominates the runtime (the rest of the pipeline is ~free), and parsing distinct
/// translation units is embarrassingly parallel. One `Clang` is created for the whole run; each
/// thread parses a strided share of the files with its own `Index`, after pointing its thread-local
/// libclang handle at the shared library (`clang-sys`'s runtime loader is per-thread).
fn parse_all(files: &[(&str, &str)]) -> Vec<Result<RawModule, String>> {
    let _guard = CLANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let clang = match Clang::new() {
        Ok(c) => c,
        Err(e) => return files.iter().map(|_| Err(format!("libclang unavailable: {e}"))).collect(),
    };
    let lib = clang_sys::get_library();
    let shared = SharedClang(&clang);
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(files.len());

    // Compute compiler args ONCE and share them across workers: the precise flags from a
    // `compile_commands.json` if the project has one, else the heuristic `-I` guess.
    let mut owned_args: Vec<String> = BASE_ARGS.iter().map(|s| s.to_string()).collect();
    match compdb::flags(files) {
        Some(flags) => owned_args.extend(flags),
        None => owned_args.extend(include_dirs(files).into_iter().map(|d| format!("-I{d}"))),
    }
    let args: Vec<&str> = owned_args.iter().map(String::as_str).collect();

    let mut out: Vec<Option<Result<RawModule, String>>> = (0..files.len()).map(|_| None).collect();
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|tid| {
                let shared = &shared;
                let args = &args;
                let lib = lib.clone();
                s.spawn(move || {
                    if let Some(l) = lib {
                        clang_sys::set_library(Some(l)); // point this thread's TLS at the shared libclang
                    }
                    files
                        .iter()
                        .enumerate()
                        .filter(move |(i, _)| i % threads == tid)
                        .map(|(i, (file, src))| (i, parse_with(shared.0, src, file, args)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for h in handles {
            for (i, r) in h.join().expect("a parse worker thread panicked") {
                out[i] = Some(r);
            }
        }
    });
    out.into_iter().map(|o| o.expect("every file index was assigned to a worker")).collect()
}

pub struct Cpp;

impl Frontend for Cpp {
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
    if files.is_empty() {
        return Vec::new();
    }
    let mut modules = Vec::new();
    let mut errors = Vec::new();
    for ((file, _), result) in files.iter().zip(parse_all(files)) {
        match result {
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
    analyze_source(src, "test.cpp", &Weights::default())
        .iter()
        .filter(|f| f.category == Category::ScopeDebt)
        .map(|f| f.score)
        .sum()
}

/// Convenience for tests: number of declare-before-use / declare-order warnings in a source string.
pub fn warnings_of(src: &str) -> usize {
    analyze_source(src, "test.cpp", &Weights::default())
        .iter()
        .filter(|f| f.category == Category::DeclBeforeUse)
        .count()
}
