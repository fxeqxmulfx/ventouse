//! The shared wedge primitive (`core::wedge`) tested directly — the one gap-to-deps implementation
//! used by both value scope-debt and definition placement.

use std::collections::HashSet;
use ventouse::core::wedge::{self, Sib};

fn sib(key: &'static str, line: u32, deps: &[&'static str]) -> Sib<&'static str> {
    Sib { key, line, deps: deps.iter().copied().collect() }
}

fn deps(ds: &[&'static str]) -> HashSet<&'static str> {
    ds.iter().copied().collect()
}

#[test]
fn dep_side_counts_unrelated_wedge() {
    // target at line 5 depends on `a` (line 1); `junk` (line 3) is wedged between them.
    let sibs = [sib("a", 1, &[]), sib("junk", 3, &[])];
    assert_eq!(wedge::dep_side(5, &deps(&["a"]), &sibs), 1);
}

#[test]
fn dep_side_no_dependency_above_is_free() {
    // the only dependency is BELOW the target -> no span -> 0.
    let sibs = [sib("a", 9, &[])];
    assert_eq!(wedge::dep_side(5, &deps(&["a"]), &sibs), 0);
}

#[test]
fn dep_side_dependency_is_not_a_wedge() {
    // a sibling that IS a dependency does not count.
    let sibs = [sib("a", 1, &[]), sib("b", 3, &[])];
    assert_eq!(wedge::dep_side(5, &deps(&["a", "b"]), &sibs), 0);
}

#[test]
fn dep_side_shared_dependency_is_not_a_wedge() {
    // `g` shares dependency `a` with the target -> same cluster -> not a wedge.
    let sibs = [sib("a", 1, &[]), sib("g", 3, &["a"])];
    assert_eq!(wedge::dep_side(5, &deps(&["a"]), &sibs), 0);
}

#[test]
fn between_respects_bounds_and_exemption() {
    let sibs = [sib("x", 2, &[]), sib("y", 4, &[]), sib("z", 6, &[])];
    // window (1, 5) covers x and y; exempt y -> only x counts.
    assert_eq!(wedge::between(1, 5, &deps(&[]), &sibs, |k| *k == "y"), 1);
}

#[test]
fn first_dep_above_is_the_nearest_dependency() {
    // the CLOSEST dependency above the target (line 3, not the topmost at 1) — so a far-away shared
    // dependency does not stretch the wedge span across the whole file.
    let sibs = [sib("a", 1, &[]), sib("b", 3, &[])];
    assert_eq!(wedge::first_dep_above(9, &deps(&["a", "b"]), &sibs), Some(3));
    assert_eq!(wedge::first_dep_above(9, &deps(&["missing"]), &sibs), None);
}
