//! Definition placement debt (P5 locality, "gap-to-deps"): a definition should sit right after the
//! definitions it depends on, with nothing unrelated wedged in between. Operates on the [`DefGraph`]
//! and textual position, within one scope (siblings sharing a qualname parent). The wedge counting
//! itself is shared with value scope-debt (`core::wedge`) — one implementation for both.
//!
//! Declaring a function is otherwise free (no nesting penalty) — this only rewards keeping a
//! definition next to what it needs.

use std::collections::{HashMap, HashSet};

use crate::core::defgraph::DefGraph;
use crate::core::model::EntityKind;
use crate::core::score::Weights;
use crate::core::wedge;

/// A placed definition: code (function/method/class) or module/class data (a `Binding` in the
/// graph). Each gets a gap-to-deps finding and is an eligible sibling / dependency / wedge.
fn is_def(k: EntityKind) -> bool {
    matches!(k, EntityKind::Function | EntityKind::Method | EntityKind::Class | EntityKind::Binding)
}

fn parent(qualname: &str) -> &str {
    qualname.rsplit_once('.').map(|(p, _)| p).unwrap_or("")
}

/// Class-member ACCESSORS: high-fan-in methods that depend only on other accessors (transitively —
/// seeded by the leaves that call nothing). A one-line `readableBytes()`/`begin()` is a field read
/// dressed as a method: de-facto DATA, used all over the class. Like data, such a method is PINNED —
/// its position is irrelevant, so it must not figure in any gap-to-deps span: it neither pulls a
/// caller toward it (excluded from `target_deps`) nor wedges anyone (excluded from the siblings).
/// Without this, a handful of shared accessors inflate the placement debt of every method that uses
/// them (dogfooding `muduo::Buffer`: 48 methods, all reaching a few top-of-class accessors).
///
/// Restricted to class members (`Method`): a high-fan-in leaf at module scope is genuinely shared
/// infrastructure the caller can EXTRACT (handled by `analyze::extract_suggestions`), but a private
/// accessor cannot be lifted out of its class — so it is pinned in place instead.
pub fn accessors(g: &DefGraph, w: &Weights) -> HashSet<usize> {
    let mut fanin = vec![0usize; g.defs.len()];
    let mut callers: Vec<HashSet<usize>> = vec![HashSet::new(); g.defs.len()];
    for (i, d) in g.defs.iter().enumerate() {
        for &c in &d.calls {
            if c != i {
                callers[c].insert(i);
            }
        }
    }
    for (i, c) in callers.iter().enumerate() {
        fanin[i] = c.len();
    }
    let dense = |i: usize| g.defs[i].kind == EntityKind::Method && fanin[i] >= w.accessor_fanin;

    let mut pinned: HashSet<usize> = HashSet::new();
    loop {
        let mut grew = false;
        for i in 0..g.defs.len() {
            if pinned.contains(&i) || !dense(i) {
                continue;
            }
            // an accessor depends only on (other) accessors — pure leaves seed the fixpoint
            if g.defs[i].calls.iter().all(|&c| c == i || pinned.contains(&c)) {
                pinned.insert(i);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    pinned
}

/// `(def index, wedge count)` for each definition with unrelated siblings between it and its deps.
pub fn wedges(g: &DefGraph, w: &Weights) -> Vec<(usize, u32)> {
    // siblings = definitions sharing an enclosing scope (qualname parent). A definition is code
    // (function/method/class) OR module/class DATA (a constant / attribute): both depend on what
    // they reference and both are placed near it (a function near the data it reads; a constant near
    // the constants it is computed from).
    let mut groups: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, d) in g.defs.iter().enumerate() {
        if is_def(d.kind) {
            groups.entry(parent(&d.qualname)).or_default().push(i);
        }
    }

    // Pinned accessors are de-facto data (a field read dressed as a method). They must not ANCHOR a
    // gap-to-deps span (a top-of-class accessor would drag every caller's span to the top) and are
    // not themselves wedges — but they stay in `target_deps` for the "shares a dependency" exemption,
    // so methods that co-use an accessor still count as related, not as junk wedging each other.
    let pinned = accessors(g, w);

    let mut out = Vec::new();
    for (fi, f) in g.defs.iter().enumerate() {
        if !is_def(f.kind) || pinned.contains(&fi) {
            continue;
        }
        // `f` is a definition, so it is in its own sibling group (the lookup is always present).
        let target_deps: HashSet<usize> = f.calls.iter().copied().collect();
        // anchor candidates: real dependencies only — an accessor never sets the top of the span.
        let anchor_deps: HashSet<usize> = target_deps.iter().copied().filter(|c| !pinned.contains(c)).collect();
        let sibs: Vec<wedge::Sib<usize>> = groups[parent(&f.qualname)]
            .iter()
            .filter(|&&gi| gi != fi && !pinned.contains(&gi))
            .map(|&gi| wedge::Sib { key: gi, line: g.defs[gi].line, deps: g.defs[gi].calls.iter().copied().collect() })
            .collect();
        let count = match wedge::first_dep_above(f.line, &anchor_deps, &sibs) {
            Some(lo) => wedge::between(lo, f.line, &target_deps, &sibs, |_| false),
            None => 0, // no REAL dependency above it — its only deps above are ubiquitous accessors
        };
        if count > 0 {
            out.push((fi, count));
        }
    }
    out
}

/// `(def index, first-use line, use-side wedge count)` for each definition declared ABOVE its first
/// use with unrelated siblings in between — the definition-level mirror of the local ReorderBinding.
/// The first use is the FIRST reference, so nothing in the gap uses the definition (anything that did
/// would be the first use); the wedges are therefore provably independent, and pushing the definition
/// DOWN to its first use is a safe, mechanical move that closes exactly that gap. A reference ABOVE
/// the definition is a forward reference (a declare-order matter, `declorder`), not a reorder — those
/// definitions are skipped. Pinned accessors are ubiquitous, never reordered.
pub fn use_reorder(g: &DefGraph, w: &Weights) -> Vec<(usize, u32, u32)> {
    let mut groups: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, d) in g.defs.iter().enumerate() {
        if is_def(d.kind) {
            groups.entry(parent(&d.qualname)).or_default().push(i);
        }
    }
    let pinned = accessors(g, w);
    // every reference TO a definition (its callers), so we can find its first use below.
    let mut callers: Vec<Vec<usize>> = vec![Vec::new(); g.defs.len()];
    for (i, d) in g.defs.iter().enumerate() {
        for &c in &d.calls {
            if c != i {
                callers[c].push(i);
            }
        }
    }

    let mut out = Vec::new();
    for (di, d) in g.defs.iter().enumerate() {
        if !is_def(d.kind) || pinned.contains(&di) {
            continue;
        }
        // first use = the lowest-line reference; only a move-DOWN target when it sits BELOW the def.
        let Some(&fc) = callers[di].iter().min_by_key(|&&c| g.defs[c].line) else {
            continue;
        };
        let hi = g.defs[fc].line;
        if hi <= d.line {
            continue; // referenced at/above its own line → forward reference, not a reorder
        }
        let target_deps: HashSet<usize> = d.calls.iter().copied().filter(|c| !pinned.contains(c)).collect();
        // siblings the first user ALSO calls are co-used with `d` at that site — one cluster, not junk.
        let co_used: HashSet<usize> = g.defs[fc].calls.iter().copied().collect();
        let sibs: Vec<wedge::Sib<usize>> = groups[parent(&d.qualname)]
            .iter()
            .filter(|&&gi| gi != di && !pinned.contains(&gi))
            .map(|&gi| wedge::Sib { key: gi, line: g.defs[gi].line, deps: g.defs[gi].calls.iter().copied().collect() })
            .collect();
        let count = wedge::between(d.line, hi, &target_deps, &sibs, |k| co_used.contains(k));
        if count > 0 {
            out.push((di, hi, count));
        }
    }
    out
}
