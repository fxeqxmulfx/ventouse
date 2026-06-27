"""Catalog 3/3 — the CrowdedScope suggestion, plus patterns that look risky but
score ZERO on purpose (the metric must not punish good code).
Run:  ventouse tests/fixtures/python/catalog/03_free_and_crowded.py --all
"""


# === CrowdedScope (suggestion) — a bag of INDEPENDENT accumulators ===
# Each local reads nothing, yet they wedge each other. Bundling them into a small
# dataclass collapses the bag into one value and removes the mutual wedging.

def crowded_bad(rows):
    total = 0
    count = 0
    largest = 0
    smallest = 0
    names = []
    seen = set()
    for r in rows:
        total += r.value
        count += 1
        largest = max(largest, r.value)
        smallest = min(smallest, r.value)
        names.append(r.name)
        seen.add(r.id)
    return total, count, largest, smallest, names, seen


# === NOT penalized: a single loop-carried accumulator ===
# One accumulator initialised before the loop is loop-carried state — it CANNOT
# narrow into the loop, and with nothing to wedge it, it scores 0.

def running_sum_ok(rows):
    total = 0
    for r in rows:
        total += r
    return total


# === NOT penalized: module data read by one function ===
# A module-level constant is a DATA definition, placed/ordered like a definition
# (next to its reader), never narrowed into the function that reads it.

LIMIT = 100


def clamp(x):
    return min(x, LIMIT)


# === NOT penalized: global state, pinned ===
# State shared via `global` is intentional — it is pinned and never narrowed,
# even though it is written in just one function.

COUNTER = 0


def bump():
    global COUNTER
    COUNTER += 1
    return COUNTER


# === NOT penalized: parameters ===
# Parameters are not bindings the function chose to place — they are excluded.

def add(a, b):
    return a + b
