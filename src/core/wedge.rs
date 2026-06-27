//! Shared wedge counting (P5 locality, "gap-to-deps") — ONE implementation for both value bindings
//! (`scopegraph`) and definitions (`placement`). A definition/value should sit right after the
//! things it depends on; an UNRELATED sibling wedged between it and what it connects to is one unit
//! of debt. "Unrelated" = not a dependency, and not sharing a dependency with the target.

use std::collections::HashSet;
use std::hash::Hash;

/// One candidate sibling: its key, its textual line, and the keys it depends on.
pub struct Sib<K> {
    pub key: K,
    pub line: u32,
    pub deps: HashSet<K>,
}

/// The line of the target's NEAREST dependency declared above it, if any — the closest one, so a
/// far-away SHARED dependency (a utility used by many definitions, which cannot sit adjacent to all
/// of them) does not drag the span to the top of the file. `siblings` must exclude the target itself
/// (and anything ineligible). Junk is then counted only in the tight gap below the nearest dep.
pub fn first_dep_above<K: Eq + Hash>(target_line: u32, target_deps: &HashSet<K>, siblings: &[Sib<K>]) -> Option<u32> {
    siblings
        .iter()
        .filter(|s| s.line < target_line && target_deps.contains(&s.key))
        .map(|s| s.line)
        .max()
}

/// Count unrelated siblings strictly between lines `lo` and `hi`. A sibling is a wedge unless it is
/// a dependency of the target, shares a dependency with it, or is exempted by `exempt` (the use side
/// exempts siblings co-used at the target's first-use site).
pub fn between<K: Eq + Hash>(
    lo: u32,
    hi: u32,
    target_deps: &HashSet<K>,
    siblings: &[Sib<K>],
    exempt: impl Fn(&K) -> bool,
) -> u32 {
    siblings
        .iter()
        .filter(|s| {
            lo < s.line
                && s.line < hi
                && !target_deps.contains(&s.key)
                && target_deps.is_disjoint(&s.deps)
                && !exempt(&s.key)
        })
        .count() as u32
}

/// Dep-side wedges: unrelated siblings between the target and its nearest dependency above. Free
/// (0) when the target has no dependency above it. Used for both values and definitions.
pub fn dep_side<K: Eq + Hash>(target_line: u32, target_deps: &HashSet<K>, siblings: &[Sib<K>]) -> u32 {
    match first_dep_above(target_line, target_deps, siblings) {
        Some(lo) => between(lo, target_line, target_deps, siblings, |_| false),
        None => 0,
    }
}
