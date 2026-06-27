//! Declare-before-use: callees-before-callers (P5 readability). Operates on the [`DefGraph`] +
//! textual position (`line`) of definitions.
//!
//! A reference `caller → callee` is a forward reference when the callee is defined LATER in the
//! SAME scope (same enclosing qualname) than the caller. Order is free for the compiler but matters
//! for humans reading top-down, so a forward reference is a warning — UNLESS it is part of an
//! unavoidable definition cycle (mutual recursion / self-reference), detected by reachability (the
//! callee can reach the caller). Per-module: not applied across files (those are imports).

use std::collections::{HashSet, VecDeque};

use crate::core::defgraph::DefGraph;
use crate::core::finding::{Category, Finding, Severity};
use crate::core::model::Reason;
use crate::core::score::DeclOrder;

/// The enclosing scope of a qualname: everything before the last `.` (or "" for top level).
fn parent_scope(qualname: &str) -> &str {
    qualname.rsplit_once('.').map(|(p, _)| p).unwrap_or("")
}

/// The leaf (own) name of a qualname: everything after the last `.`.
fn leaf(qualname: &str) -> &str {
    qualname.rsplit_once('.').map(|(_, l)| l).unwrap_or(qualname)
}

/// Can `from` reach `to` via reference edges? (Used to spot definition cycles.)
fn reaches(g: &DefGraph, from: usize, to: usize) -> bool {
    let mut seen = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(from);
    while let Some(n) = queue.pop_front() {
        if n == to {
            return true;
        }
        if !seen.insert(n) {
            continue;
        }
        for &c in &g.defs[n].calls {
            queue.push_back(c);
        }
    }
    false
}

/// Emit a `DeclBeforeUse` warning per (deduped) out-of-order reference between same-scope siblings.
/// The direction is the one conventional axis (`order`): bottom-up wants the callee ABOVE the
/// caller (a callee below is a forward reference); top-down wants it BELOW (a callee above is the
/// out-of-order one). Cycles are exempt either way.
pub fn warnings(g: &DefGraph, order: DeclOrder) -> Vec<Finding> {
    let mut out = Vec::new();
    for (ci, caller) in g.defs.iter().enumerate() {
        let cparent = parent_scope(&caller.qualname);
        let mut seen = HashSet::new();
        for &callee_idx in &caller.calls {
            let callee = &g.defs[callee_idx];
            // same scope = same enclosing qualname
            if parent_scope(&callee.qualname) != cparent {
                continue;
            }
            // a callee on the convention's "in order" side needs no warning
            let in_order = match order {
                DeclOrder::BottomUp => callee.line <= caller.line, // callee above caller
                DeclOrder::TopDown => callee.line >= caller.line,  // callee below caller
            };
            if in_order {
                continue;
            }
            if !seen.insert(callee_idx) {
                continue; // one warning per (caller, callee)
            }
            // an unavoidable cycle (mutual recursion / self) is exempt
            if reaches(g, callee_idx, ci) {
                continue;
            }
            // Attributed to the CALLER (the referrer is in place; the misplaced one is the callee) —
            // the reason names the callee, and `below` records which side it is on for the chosen
            // convention, so the wording stays true in either direction.
            out.push(Finding {
                file: g.file.clone(),
                line: caller.line,
                entity: caller.qualname.clone(),
                entity_kind: caller.kind,
                category: Category::DeclBeforeUse,
                severity: Severity::Warning,
                score: 0,
                reason: Reason::ForwardRef {
                    callee: leaf(&callee.qualname).to_string(),
                    below: order == DeclOrder::BottomUp, // bottom-up out-of-order = callee below the caller
                },
            });
        }
    }
    out
}
