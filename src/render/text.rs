//! Text renderer (the default). Human wording for reason codes lives here, not in the core.

use std::fmt::Write;

use crate::core::{Category, Finding, Reason};
use crate::render::{By, View, rank, summarize};

/// Render a ranking (top `n` of `pairs`).
pub fn ranking(pairs: &[(String, u32)], n: usize, label: &str) -> String {
    use std::fmt::Write;
    let mut out = format!("top {n} by {label}:\n");
    if pairs.is_empty() {
        out.push_str("  (nothing scored)\n");
    }
    for (name, score) in pairs.iter().take(n) {
        let _ = writeln!(out, "  {score:>6}  {name}");
    }
    out
}

fn category_text(c: Category) -> &'static str {
    match c {
        Category::ScopeDebt => "scope",
        Category::DeclBeforeUse => "decl-before-use",
        Category::Suggestion => "suggest",
        Category::ParseError => "parse-error",
    }
}

/// The human wording for a stable reason code.
pub fn reason_text(r: &Reason) -> String {
    match r {
        Reason::ExcessLevels(n) => format!("declared {n} level(s) too high"),
        Reason::Misplaced(n) => {
            format!("{n} unrelated definition(s) between it and what it connects to")
        }
        Reason::UseBeforeDecl => "used before its declaration".to_string(),
        Reason::ForwardRef { callee, below: true } => format!("references `{callee}`, declared below it"),
        Reason::ForwardRef { callee, below: false } => {
            format!("references `{callee}`, declared above it (stepdown order wants callees below)")
        }
        Reason::ExtractShared(n) => {
            format!("referenced {n} times here — extract into its own module (cross-file refs aren't penalized)")
        }
        Reason::CrowdedScope(n) => {
            format!("a bag of independent locals carries {n} scope-debt — group them into a struct")
        }
        Reason::ReorderBinding { first_use, wedged } => {
            format!("{wedged} unrelated definition(s) before its first use (line {first_use}) — move the declaration down to it")
        }
        Reason::NarrowToBlock { first_use, levels } => {
            format!("used only inside a block {levels} level(s) deeper — declare it at its first use (line {first_use})")
        }
        Reason::ParseError(msg) => format!("parse error: {msg}"),
    }
}

fn summary(findings: &[Finding]) -> String {
    let s = summarize(findings);
    let mut out = String::new();
    let _ = writeln!(out, "scope-debt {}", s.scope_total);
    let _ = writeln!(
        out,
        "warnings: {}   suggestions: {}   parse errors: {}",
        s.warnings, s.suggestions, s.parse_errors
    );
    if !s.per_file.is_empty() {
        out.push_str("\nby file:\n");
        for (file, score) in &s.per_file {
            let _ = writeln!(out, "  {score:>6}  {file}");
        }
    }
    out
}

fn all(findings: &[Finding]) -> String {
    if findings.is_empty() {
        return "clean — no findings\n".to_string();
    }
    let mut out = String::new();
    for f in findings {
        let score = if f.category == Category::ScopeDebt {
            format!("{:>4}  ", f.score)
        } else {
            "      ".to_string()
        };
        let _ = writeln!(
            out,
            "{}:{}  {score}{}  [{}]  {}",
            f.file,
            f.line,
            f.entity,
            category_text(f.category),
            reason_text(&f.reason),
        );
    }
    let s = summarize(findings);
    let _ = writeln!(out, "\ncombined {}", s.combined());
    out
}

fn top(findings: &[Finding], n: usize, by: By) -> String {
    ranking(&rank(findings, by), n, by.label())
}

pub fn render(findings: &[Finding], view: View) -> String {
    match view {
        View::Summary => summary(findings),
        View::All => all(findings),
        View::Top { n, by } => top(findings, n, by),
    }
}
