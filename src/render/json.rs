//! JSON renderer (hand-rolled — no serde dep yet). Stable, byte-deterministic for a fixed input.

use std::fmt::Write;

use crate::core::{Category, EntityKind, Finding, Severity};
use crate::render::{By, View, rank, summarize};

fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Render a ranking as a JSON array.
pub fn ranking(pairs: &[(String, u32)], n: usize) -> String {
    let items: Vec<&(String, u32)> = pairs.iter().take(n).collect();
    let mut out = String::from("[");
    for (i, (name, score)) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(out, "\n  {{\"name\": {}, \"score\": {}}}", quote(name), score);
    }
    out.push_str(if items.is_empty() { "]\n" } else { "\n]\n" });
    out
}

fn kind_str(k: EntityKind) -> &'static str {
    match k {
        EntityKind::Function => "Function",
        EntityKind::Method => "Method",
        EntityKind::Class => "Class",
        EntityKind::Module => "Module",
        EntityKind::Binding => "Binding",
    }
}

fn category_str(c: Category) -> &'static str {
    match c {
        Category::ScopeDebt => "ScopeDebt",
        Category::DeclBeforeUse => "DeclBeforeUse",
        Category::Suggestion => "Suggestion",
        Category::ParseError => "ParseError",
    }
}

fn severity_str(s: Severity) -> &'static str {
    match s {
        Severity::Error => "Error",
        Severity::Warning => "Warning",
        Severity::Info => "Info",
    }
}

fn summary(findings: &[Finding]) -> String {
    let s = summarize(findings);
    let mut out = String::new();
    out.push_str("{\n");
    let _ = writeln!(out, "  \"combined\": {},", s.combined());
    let _ = writeln!(out, "  \"scope_total\": {},", s.scope_total);
    let _ = writeln!(out, "  \"warnings\": {},", s.warnings);
    let _ = writeln!(out, "  \"suggestions\": {},", s.suggestions);
    let _ = writeln!(out, "  \"parse_errors\": {},", s.parse_errors);
    out.push_str("  \"per_file\": [");
    for (i, (file, score)) in s.per_file.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(out, "\n    {{\"file\": {}, \"score\": {}}}", quote(file), score);
    }
    out.push_str(if s.per_file.is_empty() { "]\n}\n" } else { "\n  ]\n}\n" });
    out
}

fn all(findings: &[Finding]) -> String {
    let mut out = String::new();
    out.push('[');
    for (i, f) in findings.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("\n  {");
        let _ = write!(out, "\"file\": {}, ", quote(&f.file));
        let _ = write!(out, "\"line\": {}, ", f.line);
        let _ = write!(out, "\"entity\": {}, ", quote(&f.entity));
        let _ = write!(out, "\"kind\": {}, ", quote(kind_str(f.entity_kind)));
        let _ = write!(out, "\"category\": {}, ", quote(category_str(f.category)));
        let _ = write!(out, "\"severity\": {}, ", quote(severity_str(f.severity)));
        let _ = write!(out, "\"score\": {}, ", f.score);
        let _ = write!(out, "\"reason\": {}", quote(&super::text::reason_text(&f.reason)));
        out.push('}');
    }
    out.push_str(if findings.is_empty() { "]\n" } else { "\n]\n" });
    out
}

fn top(findings: &[Finding], n: usize, by: By) -> String {
    ranking(&rank(findings, by), n)
}

pub fn render(findings: &[Finding], view: View) -> String {
    match view {
        View::Summary => summary(findings),
        View::All => all(findings),
        View::Top { n, by } => top(findings, n, by),
    }
}
