// ventouse C++ fixtures — scope-debt (P5 locality). Model = levels + wedges; no per-line liveness.
// C++ is block-scoped, so `levels` narrows the real runtime scope. `levels` is loop-aware.

// CPP-S-LEVELS — `x` used only inside the `if` -> levels_excess 1 -> 10.
int deep_only(bool flag, int a) {
    int x = a + 1;
    if (flag) {
        return x;
    }
    return 0;
}

// CPP-S-LOOP — accumulator declared before the loop is NOT penalized (loop-aware levels).
int total(const int* items, int n) {
    int acc = 0;
    for (int i = 0; i < n; i++) {
        acc += items[i];
    }
    return acc;
}
