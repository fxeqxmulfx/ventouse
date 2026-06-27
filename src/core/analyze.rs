//! The analysis pipeline: a project of [`RawModule`]s → a sorted `Vec<Finding>`. Language-agnostic
//! — lowering (`core::lower`) builds the call graph; then scope-debt (the binding `wedges`/`levels`
//! carried on `RawModule`) + placement (`core::placement`) + declare-before-use (`core::declorder`).
//! A frontend just produces the `RawModule`s and calls [`analyze`].

use crate::core::defgraph::DefGraph;
use crate::core::finding::{Category, Finding, Severity};
use crate::core::model::{EntityKind, Reason};
use crate::core::raw::{RawKind, RawModule};
use crate::core::score::Weights;

/// Stable order: by (file, line, entity).
pub fn sort_findings(findings: &mut [Finding]) {
    findings.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then(a.line.cmp(&b.line))
            .then(a.entity.cmp(&b.entity))
    });
}

/// Scope-debt findings from a module's raw scope entries (levels + wedges), applying weights.
/// A binding can contribute up to two findings: levels (nesting) and wedges (Misplaced locality).
fn scope_findings(raw: &RawModule, weights: &Weights) -> Vec<Finding> {
    let mut out = Vec::new();
    for s in &raw.scope {
        let mk = |score: u32, reason: Reason| Finding {
            file: raw.file.clone(),
            line: s.line,
            entity: s.entity.clone(),
            entity_kind: EntityKind::Binding,
            category: Category::ScopeDebt,
            severity: Severity::Info,
            score,
            reason,
        };
        if s.levels_excess > 0 {
            out.push(mk(weights.scope_level * s.levels_excess, Reason::ExcessLevels(s.levels_excess)));
        }
        if s.wedges > 0 {
            out.push(mk(weights.scope_level * s.wedges, Reason::Misplaced(s.wedges)));
        }
    }
    out
}

/// Use-before-binding `DeclBeforeUse` findings from a module's raw warnings.
fn decl_warning_findings(raw: &RawModule) -> Vec<Finding> {
    raw.decl_warnings
        .iter()
        .map(|(entity, line)| Finding {
            file: raw.file.clone(),
            line: *line,
            entity: entity.clone(),
            entity_kind: EntityKind::Binding,
            category: Category::DeclBeforeUse,
            severity: Severity::Warning,
            score: 0,
            reason: Reason::UseBeforeDecl,
        })
        .collect()
}

/// Definition-placement (gap-to-deps, P5) findings from the definition graph, applying weights.
fn placement_findings(g: &DefGraph, weights: &Weights) -> Vec<Finding> {
    crate::core::placement::wedges(g, weights)
        .into_iter()
        .map(|(i, count)| {
            let d = &g.defs[i];
            Finding {
                file: g.file.clone(),
                line: d.line,
                entity: d.qualname.clone(),
                entity_kind: d.kind,
                category: Category::ScopeDebt,
                severity: Severity::Info,
                score: weights.scope_level * count,
                reason: Reason::Misplaced(count),
            }
        })
        .collect()
}

/// `ExtractShared` suggestions: high-use definitions (shared infrastructure) in a file that carries
/// placement debt. Moving them into their own module turns those references cross-file (so they stop
/// wedging) and localizes the rest — the actionable fix the placement numbers point at.
fn extract_suggestions(g: &DefGraph, has_placement_debt: bool, w: &Weights) -> Vec<Finding> {
    if !has_placement_debt {
        return Vec::new(); // a tightly-clustered shared helper needs no extraction
    }
    // Count references from OTHER definitions (self-recursion is not sharing): both the DISTINCT
    // referrers and the TOTAL number of references.
    let mut callers: Vec<std::collections::HashSet<usize>> = vec![Default::default(); g.defs.len()];
    let mut uses = vec![0u32; g.defs.len()];
    for (i, d) in g.defs.iter().enumerate() {
        for &c in &d.calls {
            if c != i {
                callers[c].insert(i);
                uses[c] += 1;
            }
        }
    }
    let shared = |i: usize| {
        callers[i].len() >= w.shared_callers || (uses[i] >= w.shared_uses && callers[i].len() >= 2)
    };
    // Methods are excluded: "extract into its own module" is a module-scope remedy — you cannot lift
    // a private member out of its class. A high-fan-in class member is either a pinned accessor
    // (handled in `placement`) or simply a core method; neither is extractable.
    g.defs
        .iter()
        .enumerate()
        .filter(|(i, d)| shared(*i) && d.kind != EntityKind::Module && d.kind != EntityKind::Method)
        .map(|(i, d)| Finding {
            file: g.file.clone(),
            line: d.line,
            entity: d.qualname.clone(),
            entity_kind: d.kind,
            category: Category::Suggestion,
            severity: Severity::Info,
            score: 0,
            reason: Reason::ExtractShared(uses[i]),
        })
        .collect()
}

/// `CrowdedScope` suggestions: a function holding a "bag" of INDEPENDENT mutable accumulators (each
/// reads nothing, yet they wedge each other). Bundling them into a struct collapses N siblings into
/// one and removes that mutual wedging — a reliable metric win. Only independent-local debt counts:
/// a function whose debt comes from interdependent, sequential locals can't be bundled (splitting it
/// is a maintainability call that doesn't lower locality debt), so it is NOT flagged here.
fn crowded_suggestions(m: &RawModule, weights: &Weights) -> Vec<Finding> {
    let mut debt: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    for s in &m.scope {
        if !s.independent {
            continue; // only a bundleable bag of independent state reduces the metric
        }
        let owner = s.entity.rsplit_once('.').map(|(o, _)| o).unwrap_or(&s.entity);
        *debt.entry(owner).or_default() += weights.scope_level * (s.levels_excess + s.wedges);
    }
    debt.into_iter()
        .filter(|(_, d)| *d >= weights.crowded_scope)
        .filter_map(|(owner, d)| {
            let e = m.entities.iter().find(|e| e.qualname == owner)?;
            // the owner of a local binding is always a function or a method.
            let entity_kind = if e.kind == RawKind::Method { EntityKind::Method } else { EntityKind::Function };
            Some(Finding {
                file: m.file.clone(),
                line: e.line,
                entity: owner.to_string(),
                entity_kind,
                category: Category::Suggestion,
                severity: Severity::Info,
                score: 0,
                reason: Reason::CrowdedScope(d),
            })
        })
        .collect()
}

/// A binding whose declaration sits this many unrelated definitions above its first use is "scattered"
/// enough to flag for reordering (≈30 displacement units at the default weight). Below it the use-side
/// gap is left as a silent score so the suggestion list stays signal, not noise.
/// `ReorderBinding` suggestions: a local binding declared far above its first use, with `use_wedges`
/// unrelated definitions in between. Those wedges are provably independent of the binding (anything
/// using it would pull `first_use` up), so pushing the declaration down to its first use is a safe,
/// mechanical move that erases exactly that use-side gap — the actionable face of a large use-side
/// wedge (the opposite extreme of a `UseBeforeDecl` warning).
fn reorder_suggestions(m: &RawModule, w: &Weights) -> Vec<Finding> {
    m.scope
        .iter()
        // `levels_excess == 0`: a binding that could ALSO move into a deeper block is owned by
        // `narrow_suggestions` (the more specific move) — this handles the same-level scattering.
        .filter(|s| s.levels_excess == 0 && s.use_wedges >= w.reorder_wedges && s.first_use > s.line)
        .map(|s| Finding {
            file: m.file.clone(),
            line: s.line,
            entity: s.entity.clone(),
            entity_kind: EntityKind::Binding,
            category: Category::Suggestion,
            severity: Severity::Info,
            score: 0,
            reason: Reason::ReorderBinding { first_use: s.first_use, wedged: s.use_wedges },
        })
        .collect()
}

/// `NarrowToBlock` suggestions: a local used only inside a block deeper than where it is declared
/// (`levels_excess`), so declaring it at its first use (inside that block) narrows its real scope.
/// The levels-term twin of `reorder_suggestions`: `narrow_target` already capped the move above any
/// loop boundary, so a flagged binding is safe to push in (loop-carried state has `levels_excess` 0).
fn narrow_suggestions(m: &RawModule, w: &Weights) -> Vec<Finding> {
    m.scope
        .iter()
        .filter(|s| s.levels_excess >= w.narrow_levels && s.first_use > s.line)
        .map(|s| Finding {
            file: m.file.clone(),
            line: s.line,
            entity: s.entity.clone(),
            entity_kind: EntityKind::Binding,
            category: Category::Suggestion,
            severity: Severity::Info,
            score: 0,
            reason: Reason::NarrowToBlock { first_use: s.first_use, levels: s.levels_excess },
        })
        .collect()
}

/// `ReorderBinding` suggestions for DEFINITIONS: a function/method/class declared far above its first
/// use, with `wedged` unrelated definitions in between (the definition-level mirror of the local
/// `reorder_suggestions`). Pushing the definition down to its first use is the safe, mechanical move
/// that closes the gap — the actionable face of a scattered, declared-too-early definition.
fn def_reorder_suggestions(g: &DefGraph, w: &Weights) -> Vec<Finding> {
    crate::core::placement::use_reorder(g, w)
        .into_iter()
        .filter(|(_, _, wedged)| *wedged >= w.reorder_wedges)
        .map(|(di, first_use, wedged)| {
            let d = &g.defs[di];
            Finding {
                file: g.file.clone(),
                line: d.line,
                entity: d.qualname.clone(),
                entity_kind: d.kind,
                category: Category::Suggestion,
                severity: Severity::Info,
                score: 0,
                reason: Reason::ReorderBinding { first_use, wedged },
            }
        })
        .collect()
}

/// Run the whole pipeline over a project and return findings sorted by (file, line, entity).
/// Everything is per-module (locality is same-file): the scope graph's outputs (`scope`,
/// `decl_warnings`) plus the definition graph it derived (`entities`, `def_edges`) drive
/// `placement` + `declorder`.
pub fn analyze(modules: &[RawModule], weights: &Weights) -> Vec<Finding> {
    let mut findings = Vec::new();
    for m in modules {
        findings.extend(scope_findings(m, weights));
        findings.extend(decl_warning_findings(m));
        let dg = DefGraph::build(&m.file, &m.entities, &m.def_edges);
        let placement = placement_findings(&dg, weights);
        let has_placement_debt = !placement.is_empty();
        findings.extend(placement);
        findings.extend(crate::core::declorder::warnings(&dg, weights.order));
        findings.extend(extract_suggestions(&dg, has_placement_debt, weights));
        findings.extend(crowded_suggestions(m, weights));
        findings.extend(reorder_suggestions(m, weights));
        findings.extend(narrow_suggestions(m, weights));
        findings.extend(def_reorder_suggestions(&dg, weights));
    }
    sort_findings(&mut findings);
    findings
}
