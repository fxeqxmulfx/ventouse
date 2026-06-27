"""Catalog 1/3 — per-function (local variable) rules.

Each rule is a BAD function (triggers the finding) paired with a GOOD one (scores
0). Helper names like `compute`/`use`/`load` are undefined on purpose, so the
functions don't reference each other — every finding below is purely local.
Run:  ventouse tests/fixtures/python/catalog/01_local_scope.py --all
"""


# === ExcessLevels — declare a value in the narrowest block that covers its uses ===

def levels_bad(flag):
    x = compute()          # BAD: bound at the function top...
    if flag:
        return use(x)      # ...but used only inside the `if` -> "declared 1 level too high"
    return None


def levels_good(flag):
    if flag:
        x = compute()      # GOOD: declared right where it is used
        return use(x)
    return None


# === Misplaced (wedge) — keep a value next to what it connects to ===

def wedge_bad():
    value = load()         # BAD: bound here, but used only at the very end...
    a = compute_a()        # ...with two unrelated definitions (used elsewhere) wedged between
    b = compute_b()
    log(a, b)
    return use(value)      # -> value: "2 unrelated definition(s) between it and its use"


def wedge_good():
    a = compute_a()
    b = compute_b()
    log(a, b)
    value = load()         # GOOD: declared right before its use — nothing wedged
    return use(value)


# === UseBeforeDecl — bind a name before reading it (top-down readability) ===

def use_before_bad():
    print(total)           # BAD: read before it is bound (warning, 0 points)
    total = 1
    return total


def use_before_good():
    total = 1              # GOOD: bound before use
    print(total)
    return total


# === ReorderBinding (suggestion) — a value declared far above its first use ===

def reorder_bad():
    q = setup()            # BAD: bound early, first used only after 3 other definitions
    a = step_one()
    b = step_two()
    c = step_three()
    log(a, b, c)
    return run(q)          # -> Misplaced(3) + suggestion: "move the declaration down to it"


def reorder_good():
    a = step_one()
    b = step_two()
    c = step_three()
    log(a, b, c)
    q = setup()            # GOOD: declared next to its use
    return run(q)
