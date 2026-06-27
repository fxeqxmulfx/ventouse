"""Catalog 2/3 — definition-ordering rules (functions / classes / module data).

Definitions are FREE of nesting; they are scored on ORDER instead: a callee
should sit ABOVE its caller, and a definition should sit NEXT TO the ones it
references. These findings span functions, so each rule uses its own small
cluster of definitions (distinct names, no calls across clusters).
Run:  ventouse tests/fixtures/python/catalog/02_definitions.py --all
"""


# === UseBeforeDecl — a callee defined below its caller (forward reference) ===

def caller_bad():
    return leaf_bad()      # BAD: `leaf_bad` is defined LATER -> forward reference (warning)


def leaf_bad():
    return 1


def leaf_good():
    return 1               # GOOD: callee defined above...


def caller_good():
    return leaf_good()     # ...its caller


# === Misplaced (placement) — keep a definition next to what it references ===

def gravity():
    return 9.81


def noise():               # BAD: unrelated definition wedged between `gravity` and its user
    return 0


def weight(mass):
    return mass * gravity()  # -> weight: "1 unrelated definition between it and gravity"


# === ExtractShared (suggestion) — shared infrastructure used all over ===
# `shared` is referenced by four definitions below; the file already carries
# placement debt (above), so extracting `shared` into its own module would make
# those references cross-file (un-penalized) and localize the rest.

def shared():
    return 1


def task_a():
    return shared()


def task_b():
    return shared()


def task_c():
    return shared()


def task_d():
    return shared()
