//! Rendering: turn the core's `&[Finding]` (+ derived totals) into output. Display-only — the
//! core never formats strings. Adding a format is a new renderer here; core/tests are untouched.

pub mod json;
pub mod text;

use crate::core::{Category, Finding};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Text,
    Json,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum By {
    Function,
    Class,
    File,
}

impl By {
    pub fn label(self) -> &'static str {
        match self {
            By::Function => "function",
            By::Class => "class",
            By::File => "file",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// Project totals + per-file rollup.
    Summary,
    /// Every finding, sorted by (file, line).
    All,
    /// The top `n` worst offenders, ranked by combined score.
    Top { n: usize, by: By },
}

/// Rank entities/classes/files by scope-debt score, descending.
fn parent(qualname: &str) -> Option<String> {
    qualname.rsplit_once('.').map(|(p, _)| p.to_string())
}

pub fn rank(findings: &[Finding], by: By) -> Vec<(String, u32)> {
    use crate::core::EntityKind;
    let mut map: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    for f in findings {
        if f.category != Category::ScopeDebt {
            continue;
        }
        let key = match by {
            By::File => Some(f.file.clone()),
            By::Function => match f.entity_kind {
                EntityKind::Function | EntityKind::Method | EntityKind::Module => {
                    Some(f.entity.clone())
                }
                // a binding's scope-debt rolls up into its owning function
                EntityKind::Binding => parent(&f.entity),
                EntityKind::Class => None,
            },
            By::Class => match f.entity_kind {
                EntityKind::Class => Some(f.entity.clone()),
                EntityKind::Method => parent(&f.entity),
                _ => None,
            },
        };
        if let Some(k) = key {
            *map.entry(k).or_default() += f.score;
        }
    }
    let mut v: Vec<(String, u32)> = map.into_iter().filter(|(_, s)| *s > 0).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

/// Project-level rollup derived from the findings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub scope_total: u32,
    pub warnings: usize,
    pub suggestions: usize,
    pub parse_errors: usize,
    /// (file, scope-debt score), sorted by score desc then path.
    pub per_file: Vec<(String, u32)>,
}

impl Summary {
    /// The headline number — total scope-debt.
    pub fn combined(&self) -> u32 {
        self.scope_total
    }
}

/// Compute the project rollup from a flat finding list.
pub fn summarize(findings: &[Finding]) -> Summary {
    let mut s = Summary::default();
    let mut per_file: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    for f in findings {
        match f.category {
            Category::ScopeDebt => {
                s.scope_total += f.score;
                *per_file.entry(f.file.clone()).or_default() += f.score;
            }
            Category::DeclBeforeUse => s.warnings += 1,
            Category::Suggestion => s.suggestions += 1,
            Category::ParseError => s.parse_errors += 1,
        }
    }
    s.per_file = per_file.into_iter().collect();
    s.per_file.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    s
}

/// Render findings for the given view + format.
pub fn render(findings: &[Finding], view: View, format: Format) -> String {
    match format {
        Format::Text => text::render(findings, view),
        Format::Json => json::render(findings, view),
    }
}

/// Render a pre-computed ranking (e.g. blast-radius `--by=source`, computed from the call graph).
pub fn format_ranking(pairs: &[(String, u32)], n: usize, label: &str, format: Format) -> String {
    match format {
        Format::Text => text::ranking(pairs, n, label),
        Format::Json => json::ranking(pairs, n),
    }
}
