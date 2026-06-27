# Core rules — "if … then …"

The complete decision logic of the language-agnostic core, extracted from the source:
[scopegraph.rs](src/core/scopegraph.rs), [wedge.rs](src/core/wedge.rs),
[placement.rs](src/core/placement.rs), [declorder.rs](src/core/declorder.rs),
[analyze.rs](src/core/analyze.rs), [score.rs](src/core/score.rs).

Every rule below carries its own minimal example. Examples are Python (namespace, which Python lacks,
is shown in Rust), but the rules are language-agnostic. Runnable versions of the Python rules live in
[tests/fixtures/python/catalog/](tests/fixtures/python/catalog/).

## Binding classification (what counts as what)

- **If** a name is bound at module / class / namespace (`mod`) scope → **then** it is a *data
  definition* (constant/attribute): placed and ordered, but **not** narrowed (free of nesting).

```python
LIMIT = 100              # module scope → data definition; placed/ordered, never narrowed
```

- **If** the binding is a `def`/`class` → **then** it is a *Decl* (code): free of nesting, never
  penalized for levels, and becomes an entity that `placement`/`declorder` place and order.

```python
def f():                 # Decl → an entity; free of nesting, placed and ordered
    ...
```

- **If** the binding is an `import` → **then** it is a distinct *Import* kind: also free of nesting,
  but **no** entity is created — it is neither placed nor ordered.

```python
import os                # Import → free of nesting, but not an entity (not placed/ordered)
```

- **If** the binding is a local variable inside a function → **then** it is a *Value*: scored as
  `levels_excess + wedges`.

```python
def f():
    x = compute()        # local Value → scored for levels + wedges
```

- **If** a name is declared `global`/`nonlocal` → **then** a write to it is a *use* of the outer
  binding (not a local one), and it is **pinned** — never narrowed.

```python
COUNTER = 0


def bump():
    global COUNTER       # pinned shared state — `COUNTER += 1` is a use, never narrowed
    COUNTER += 1
```

- **If** the binding is a parameter → **then** it is excluded from analysis entirely.

```python
def add(a, b):           # `a`, `b` are parameters → excluded (not placements the function chose)
    return a + b
```

- **If** one name is bound several times → **then** the earliest declaration is kept (first-binding rule).

```python
def f(cond):
    x = 1                # this earliest line is "where x is declared"
    if cond:
        x = 2            # a re-binding, not a new declaration
```

### Container scopes — module / namespace / class are NOT fully interchangeable

The "data definition" rule above lumps the three definition-containers together — but only for *that*
axis. Don't read "module / class / namespace" as "the same in every rule":

- **If** a value is bound in module **or** namespace **or** class scope → **then** it is a data
  definition (free of nesting, placed/ordered, never narrowed) — identical across all three.

```python
LIMIT = 100              # module-level data def (a class attribute is the same) — never narrowed
```

- **If** a `def`/`fn` is in a **class** scope → **then** it is a **Method**: it can be pinned as an
  accessor, and it is **not** ExtractShared-eligible (methods are excluded).

```python
class C:
    def m(self):         # a Method — can pin as an accessor (when many siblings call it); never ExtractShared
        ...
```

- **If** a `def`/`fn` is in a **module or namespace** scope → **then** it is a **Function**: never
  pinned, and ExtractShared-eligible.

```python
def f():                 # a Function — never pinned; can be suggested for ExtractShared
    ...
```

- **If** the scope is a **class** → **then** its names are **not** lexically visible to nested
  functions; members reach them only via `self.`/`cls.`, resolved to the class scope.

```python
class C:
    SIZE = 8

    def meth(self):
        def inner():
            return SIZE  # does NOT resolve to C.SIZE — class body invisible to a nested fn
        return self.SIZE  # a class member is reached only via self.
```

- **If** the scope is a **module or namespace** → **then** its names are lexically visible to nested
  scopes (normal fall-through).

```python
LIMIT = 100


def m():
    def inner():
        return LIMIT     # resolves to module LIMIT — normal lexical fall-through
```

- **If** the scope is a **namespace or class** (not the root module) → **then** the qualname is
  prefixed (`a.f`, `C.f`). Namespace has no Python form — here it is in Rust (`mod`; C++ `namespace`
  behaves the same):

```rust
mod a {
    fn gravity() -> f64 { 9.81 }
    fn noise() -> i32 { 0 }                       // wedged → `a.weight` is Misplaced(1):
    fn weight(m: f64) -> f64 { m * gravity() }    // a namespace fn is placed/ordered like module code
}
mod b {
    fn gravity() -> f64 { 1.62 }                  // `b.gravity` is DISTINCT from `a.gravity` —
    fn weight(m: f64) -> f64 { m * gravity() }    // the prefix keeps `a.weight` / `b.weight` separate
}
```

- **If** the scope is the **root module** → **then** the qualname has no prefix (`f`).

```python
def f():                 # top-level → qualname `f`, no prefix
    ...
```

Net: namespace differs from module **only** by the qualname prefix; class differs by all three —
method kind, lexical invisibility, and prefix.

## Locality metric (per Value binding) — levels_excess

Declare a value in the narrowest block that covers its uses.

- **If** the variable is declared higher than the narrowest block (LCA) covering all its uses →
  **then** `levels_excess = depth(narrow_target) − depth(decl_block)`.

```python
def bad(flag):
    x = compute()        # decl at depth 0, only use at depth 1 → levels_excess = 1
    if flag:
        return use(x)
```

- **If** the narrow target coincides with the declaration block (nowhere to narrow to) → **then**
  `levels = 0`.

```python
def good():
    x = compute()        # used at the same level it is declared → nowhere to narrow → 0
    return use(x)
```

- **If** there is a loop boundary on the path down toward the use → **then** the narrow target is
  capped **above** the loop (loop-carried state is not pushed inside).

```python
def good(rows):
    acc = 0              # used inside the loop, but capped ABOVE it → levels 0 (not pushed in)
    for r in rows:
        acc += r
    return acc
```

- **If** the binding is an *introducer* (loop target / `with` / `except` / walrus) **or** pinned →
  **then** `levels = 0` (positionally fixed by the construct).

```python
def good(items):
    for x in items:      # `x` is a loop introducer — positionally fixed → levels 0
        use(x)
```

## Locality metric (per Value binding) — wedges

Junk between a thing and what it connects to.

- **If** an unrelated sibling sits between the variable and its *first use below* → **then** +1
  wedge (use-side).

```python
def bad():
    value = load()       # used only at the end, with two unrelated defs wedged between → 2 wedges
    a = compute_a()
    b = compute_b()
    log(a, b)
    return use(value)
```

- **If** an unrelated sibling sits between the variable and its *nearest dependency above* → **then**
  +1 wedge (dep-side). (Dep-side and use-side are two ends of the *same* gap: the junk between a
  dependency and its dependent counts as dep-side on the dependent **and** use-side on the
  dependency — so both bindings below are flagged.)

```python
def bad():
    cfg = load()         # +1 use-side: `a` sits between cfg and its first use (host = cfg.host)
    a = unrelated()      # the one piece of junk wedges BOTH ends of the gap
    host = cfg.host      # +1 dep-side: `a` sits between host and its dependency cfg
    return use(host)
```

- **If** the variable has no dependency above → **then** dep-side = 0 (free).

```python
def good():
    a = unrelated()
    v = load()           # no dependency above v → dep-side 0 no matter what precedes it
    return use(v)
```

- **If** a sibling is itself a dependency, **or** shares a dependency with the target, **or** is
  co-used at the target's first-use site → **then** it is *not* a wedge (one cluster).

```python
def good():
    cfg = load()         # the shared dependency
    a = cfg.host         # depends on cfg
    b = cfg.port         # depends on cfg too → a & b are one cluster, neither wedges the other
    return connect(a, b)
```

- **If** a sibling is in a "cousin" block (a different, mutually-exclusive `match`/`if` branch) →
  **then** it is not on any execution path → not a wedge.

```python
def good(flag):
    v = load()
    if flag:
        a = other()      # in a cousin branch — not on the path to v's use → not a wedge
    use(v)               # use v at the function level, so v itself carries no levels debt either
```

- **If** the binding is an introducer → **then** wedges = 0 (otherwise junk inside a loop would
  "wedge" the loop variable, which cannot be moved).

```python
def good(items):
    for x in items:      # `x` is an introducer → accrues no wedges even if junk sits in the loop
        junk = noise()
        use(x)
```

## Emitted findings

- **If** `levels_excess > 0` → **then** a `ScopeDebt` finding with `score = scope_level * levels_excess`.

```python
def f(flag):
    x = compute()        # levels_excess 1 → ScopeDebt score = scope_level (10)
    if flag:
        return use(x)
```

- **If** `wedges > 0` → **then** a `ScopeDebt` finding with `score = scope_level * wedges`. (One
  binding can produce both.)

```python
def f():
    value = load()       # 2 use-side wedges → ScopeDebt score = 2 * scope_level (20)
    a = compute_a()
    b = compute_b()
    return use(value)
```

- **If** `levels == 0 && wedges == 0` → **then** no finding.

```python
def f():
    x = load()
    return use(x)        # narrowest scope, nothing wedged → no finding
```

- **If** a name is read before its first binding in the same scope → **then** a `DeclBeforeUse`
  warning (`UseBeforeDecl`, score 0).

```python
def bad():
    print(total)         # read before it is bound → use-before-decl warning
    total = 1
    return total
```

## Definition placement (placement / gap-to-deps)

- **If** unrelated siblings sit between a definition (function/class/data) and its nearest *real*
  dependency above → **then** a `ScopeDebt` finding (Misplaced) with the wedge count.

```python
def gravity():
    return 9.81


def noise():             # unrelated, wedged between gravity and its user → weight is Misplaced(1)
    return 0


def weight(m):
    return m * gravity()
```

- **If** there is no real dependency above the definition (only ubiquitous accessors) → **then**
  count = 0.

```python
def helper():            # no dependency above it → placement count 0 wherever it sits
    return 1
```

- **If** a definition is referenced inside a lambda / comprehension / closure body → **then** the
  reference is attributed to the nearest enclosing *named* definition (the function holding the
  closure), so it still counts for that function's placement and declare-order — a callee used only
  inside a closure does **not** escape the graph. (Value captures are unaffected: they narrow via uses.)

```python
def helper():
    return 1


def n1():
    ...


def n2():
    ...


def n3():
    ...


def make():
    # helper used only in the closure → attributed to `make`, gap-to-deps from
    # helper 3 defs above → Misplaced(3)
    return lambda: helper()
```

- **If** a class-member method has fan-in ≥ `accessor_fanin` (3) and depends only on other accessors
  (fixpoint seeded by the leaves) → **then** it is **pinned**: it does not anchor a span, is not
  counted as a wedge, and is never reordered (a "field dressed as a method").

```python
class Buffer:
    def size(self):
        return self._n      # one-line accessor used by ≥3 methods below →

    def a(self):
        return self.size()  # pinned (de-facto data): won't inflate the placement debt of

    def b(self):
        return self.size()  # methods that read it, nor be reordered

    def c(self):
        return self.size()
```

## Declare-before-use (the one conventional axis)

- **If** a caller references a callee declared in the same scope, on the convention's out-of-order
  side (`bottom-up`: callee **below**; `top-down`: callee **above**) → **then** a `ForwardRef` warning.

```python
def caller():
    return leaf()        # BAD (bottom-up): `leaf` is defined below → forward reference


def leaf():
    return 1
```

- **If** the reference is cross-module (another file / import) → **then** it is not checked.

```python
import other


def f():
    return other.thing()  # cross-module reference → no forward-reference warning
```

- **If** the callee can reach the caller (a cycle / mutual recursion / self-reference) → **then** it
  is exempt (no warning).

```python
def even(n):
    return n == 0 or odd(n - 1)   # `odd` is below, but they form a cycle → exempt


def odd(n):
    return n != 0 and even(n - 1)
```

- **If** one caller references one callee multiple times → **then** one warning per pair.

```python
def caller():
    leaf()
    leaf()               # two references to the same below-defined `leaf` → ONE warning


def leaf():
    return 1
```

## Suggestions (actionable advice)

- **`ExtractShared`** — **if** the file carries placement debt **and** a definition (not a method,
  not a module) is referenced by ≥ `shared_callers` (4) distinct others, **or** ≥ `shared_uses` (8)
  times total by ≥2 others → **then** advise "extract into its own module".

```python
# this misplaced cluster gives the FILE placement debt that is ExtractShared's
# precondition (without it, nothing fires):
def gravity():
    return 9.81


def noise():
    return 0


def weight(m):
    return m * gravity()


# `shared` has 4 distinct callers → "extract into its own module" (those refs
# become cross-file and stop wedging; the rest localizes):
def shared():
    return 1


def a():
    return shared()


def b():
    return shared()


def c():
    return shared()


def d():
    return shared()
```

- **If** the file carries no placement debt → **then** ExtractShared is not emitted (a tightly
  clustered helper needs no extraction).

```python
def shared():            # referenced a lot, but if NOTHING in the file is misplaced,
    return 1             # there is nothing to localize → no ExtractShared


def a():
    return shared()


def b():
    return shared()
```

- **`CrowdedScope`** — **if** a function's *independent* locals (empty deps) together carry ≥
  `crowded_scope` (100) debt → **then** advise "bundle them into a struct/dataclass".

```python
def bad(rows):           # six independent accumulators wedging each other →
    total = 0            # "bundle them into a dataclass"
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
```

- **If** the debt comes from interdependent, sequential locals → **then** the function is **not**
  flagged (nothing to bundle).

```python
def good():
    a = step1()          # each local feeds the next (interdependent) → not an independent
    b = step2(a)         # cluster → CrowdedScope does NOT fire
    c = step3(b)
    return c
```

- **`ReorderBinding`** (local) — **if** `levels_excess == 0`, use-side wedges ≥ `reorder_wedges` (3),
  and the first use is below the declaration → **then** advise "push the declaration down to its use".

```python
def bad():
    q = setup()          # bound early, first used after 3 unrelated defs → "move it down"
    a = step_one()
    b = step_two()
    c = step_three()
    log(a, b, c)
    return run(q)
```

- **`ReorderBinding`** (definition) — **if** a function/class is declared above its first use with ≥
  `reorder_wedges` unrelated siblings in between → **then** the same advice at the definition level.

```python
def helper():            # declared far above its first use, 3 unrelated defs between → "move it down"
    return 1


def x():
    ...


def y():
    ...


def z():
    ...


def main():
    return helper()
```

- **If** the reference is *above* the definition → **then** it is a forward reference (a `declorder`
  matter), not a reorder → skipped.

```python
def main():
    return helper()      # reference ABOVE the definition → forward-ref (declorder), NOT a reorder


def helper():
    return 1
```

- **`NarrowToBlock`** — **if** `levels_excess >= narrow_levels` (1) and the first use is below the
  declaration → **then** advise "declare it at its first use, inside the block".

```python
def bad(flag):
    x = compute()        # declared 1 level too high, used only in the block →
    if flag:             # "declare it at its first use, inside the block"
        return use(x)
```

- **If** a local's first use is *inside a loop* (the move would cross a loop boundary) → **then**
  use_wedges is zeroed (the seed cannot be pushed down — it would reset every iteration); the wedge
  debt still stands, fixed via CrowdedScope.

```python
def ok(rows):
    acc = 0              # first use is inside the loop → reorder signal dropped
    junk = noise()       # (moving the seed in would reset it); the wedge debt still shows
    for r in rows:
        acc += r
    return acc
```

## Design stance — precision over coverage (intentional)

The suggestion layer fires **only on a provably-safe mechanical move**. Where safety isn't proven,
the metric still prints a number but **no suggestion is emitted** — a chosen point on the
precision/recall curve favoring precision (minimal false alarms) over full coverage. These silent
gaps are deliberate; do not widen suggestions to close them if it raises the false-alarm rate:

- a **sub-threshold dep-side wedge on a local** — there is no "move up to the dependency" suggestion;
  a gap ≥ `reorder_wedges` is instead actioned from the *dependency's* side (its use-side
  ReorderBinding moves it down), but a smaller one only shows as a number.

```python
def f():
    cfg = load()         # cfg also shows Misplaced(1) (use-side) — same gap, both ends below threshold
    a = unrelated()      # gap of 1 (< reorder_wedges) → no suggestion at either end
    host = cfg.host      # host shows Misplaced(1) (dep-side), with no "move up" suggestion
    return use(host)
```

- a **high-fan-in non-accessor method** — a private method can't be mechanically extracted, so it
  shows placement debt with no suggestion (not an accessor, and methods are not ExtractShared).

```python
class C:
    def step(self):
        return raw()        # low fan-in → not an accessor

    def junk(self):
        return 0

    def core(self):
        return self.step()  # depends on a non-accessor → not pinned; junk wedges it →

    def a(self):
        return self.core()  # `C.core` Misplaced(1), but it's a method → no suggestion

    def b(self):
        return self.core()

    def c(self):
        return self.core()
```

- a **local first-used inside a loop** — `use_wedges` is explicitly zeroed (pushing a seed in resets it).

```python
def ok(rows):
    acc = 0
    junk = noise()       # `acc` shows Misplaced(1), but no ReorderBinding (first use is in the loop)
    for r in rows:
        acc += r
    return acc
```

- **mutually-recursive accessors** aren't pinned — better than falsely hiding a real one, so they can
  inflate their callers' placement debt.

```python
class C:
    def p(self):
        return self.q()              # p and q call each other → no pure-leaf seed → not pinned

    def q(self):
        return self.p()

    def noise(self):
        return 0

    def a(self):
        return self.p() + self.q()   # noise wedges → `C.a` Misplaced(1), driven by the un-pinned p/q

    def b(self):
        return self.p() + self.q()

    def c(self):
        return self.p() + self.q()
```

### What the locality metric deliberately does NOT measure

The wedge metric looks at exactly two windows: `[nearest dependency above → declaration]` (dep-side)
and `[declaration → FIRST use]` (use-side). Everything outside those windows is invisible **by
design** — these are boundaries of the metric, not bugs to "fix":

- **Junk across a binding's live range** (between its *first* and *later* uses) is not counted. A
  value used early and late with unrelated work between has **no single safe move** (it is pinned by
  two live uses), so flagging it only produces false alarms. Measured experimentally: extending the
  use-side window to the last use raised debt on ventouse's own (clean) source by ~43%, almost all of
  it foundational locals declared at the top and correctly used throughout. Reverted — the
  `→ first use` window is intentional.
- **`self.`/member uses** never drive narrowing — a class attribute can't be moved into the method
  that reads it.
- **Cross-file distance** — locality is per-module; a definition far from its only cross-file user is
  not measured (that is the `ExtractShared` premise).
- **A value used across two scopes** can't narrow below their LCA, so it is not flagged toward either.

Contrast with the closure hole (now fixed): there the reference was genuinely *lost* from the graph,
so the definition's real distance went unmeasured — fixable with zero added noise. These boundaries
are the opposite: the connection is present, but penalizing it would be a false alarm.

```python
def f():
    x = setup()
    log(x)               # first use — tight
    a = step_one()       # unrelated work in x's live range...
    b = step_two()       # ...but x is pinned by two uses, so this is NOT counted (no safe move)
    return run(x)        # later use of x
```

## Default thresholds (all tunable in `[tool.ventouse.weights]`)

| Constant | Default | Role |
|---|---|---|
| `scope_level` | 10 | points per debt unit |
| `order` | bottom-up | declare-order convention |
| `shared_callers` | 4 | ExtractShared: broad fan-in |
| `shared_uses` | 8 | ExtractShared: heavy use from a few |
| `crowded_scope` | 100 | CrowdedScope: independent-local debt threshold |
| `reorder_wedges` | 3 | ReorderBinding: "scattered" |
| `narrow_levels` | 1 | NarrowToBlock: N levels deeper |
| `accessor_fanin` | 3 | accessor pinning |
