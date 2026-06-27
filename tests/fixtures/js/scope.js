// ventouse JS fixtures — scope-debt (P5 locality). Model = levels (narrowest block) + wedges
// (unrelated definitions between a binding and its deps/first use); no per-line liveness.
// Surface: `let`/`const` are block-scoped, so `levels` narrows the REAL runtime scope (bites
// harder than Python's function scope). `levels` is loop-aware.

// JS-S-LEVELS — `x` declared in the function body but used only inside the `if` -> it could live
// in that block (real block scope) -> levels_excess 1 -> 10.
function deepOnly(flag, a) {
  let x = a + 1;
  if (flag) {
    return x;
  }
  return 0;
}

// JS-S-LOOP — loop-carried state declared before the loop is NOT penalized (loop-aware levels):
// `total` must live outside the loop.
function sum(items) {
  let total = 0;
  for (const it of items) {
    total += it;
  }
  return total;
}
