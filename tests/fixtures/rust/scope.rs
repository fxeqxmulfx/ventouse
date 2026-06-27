// ventouse Rust fixtures — scope-debt (P5 locality). Model = levels + wedges; no per-line liveness.
// Rust is block-scoped + shadowing idiomatic, so `levels` narrows the real runtime scope.
// A local `let` use-before-decl is a COMPILER error (skipped); item order is the warning (warn.rs).

// RS-S-LEVELS — `x` used only inside the `if` -> levels_excess 1 -> 10.
fn deep_only(flag: bool, a: i32) -> i32 {
    let x = a + 1;
    if flag {
        return x;
    }
    0
}

// RS-S-LOOP — accumulator declared before the loop is NOT penalized (loop-aware levels).
fn total(items: &[i32]) -> i32 {
    let mut acc = 0;
    for it in items {
        acc += it;
    }
    acc
}
