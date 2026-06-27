# ventouse — test design (locality)

Concrete fixtures and exact expected numbers for every case in `todo.md`. ventouse scores ONE thing —
**locality** (scope-debt) — plus declare-before-use warnings and actionable refactor suggestions.
Numbers use the default scoring constant `SCOPE_LEVEL = 10`, configurable via `[tool.ventouse.weights]`.

```
scope_debt(binding) = SCOPE_LEVEL × (levels_excess + wedges)   # 10 × …
```

## Harness

The **core** is language-agnostic and has DIRECT unit tests (synthetic graph, no parser) — 100%
coverage:
- `tests/core_scopegraph.rs` — `ScopeGraph::score` built via the public API: levels (narrow / loop /
  clamp / LCA / intro), wedges, edges, entities, member resolution, class-scope skip, global/nonlocal
  pinning, declare-before-use, first-binding.
- `tests/core_defgraph.rs` — `DefGraph::build` + `placement` + `declorder` on synthetic (entities,
  edges): wedge / shared-dep / no-dep, forward / above / cycle / self / cross-scope / dedup.
- `tests/core_driver.rs` — the `scopelang::build` driver via a TOY `ScopeLang`: block nesting, loops,
  the `OpenAttrib` attribution stack, nested-scope resolution, balanced Open/Close.
- `tests/core_wedge.rs` — the shared gap-to-deps primitive directly.

The **Python frontend** is tested end-to-end:
- `tests/m4_scope.rs` — `python::scope_of(src)` → Σ scope-debt of an inline snippet (+ the
  suggestions and the loop-carried `ReorderBinding` exemption).
- `tests/m4_warn.rs` — `python::warnings_of(src)` → declare-before-use count.
- `tests/m4_references.rs` — the references model + attribution + global/nonlocal (asserts the
  warning ENTITY, not just the count), and the `--order` direction flip.
- `tests/m3_render.rs` — render from a fixed `Vec<Finding>` + an end-to-end pass (headline total,
  discovery, ParseError wiring).

The **Rust frontend** is tested in `tests/rust_lang.rs` (block-scoped `let`, `impl`/`self` member
resolution + ordering, `const` data, `self.field` is not a method).

A **catalog** of tiny good-vs-bad examples, one per rule, lives in `tests/fixtures/python/catalog/`
(`01_local_scope` · `02_definitions` · `03_free_and_crowded` + README); each `*_bad` triggers exactly
one named finding and each `*_good` scores 0, verified against real output.

Everything is computed from ONE graph: `PyLang` lowers the AST into `Action`s, the core driver builds
a `ScopeGraph`, and `ScopeGraph::score()` derives scope-debt, the definition-reference edges, the
entity list, and value declare-before-use — all with one lexical resolution. A per-module `DefGraph`
(entities + edges) then drives `placement` (definitions) and `declorder` (order). Locality is
same-file; cross-file references don't affect it.

## Scope metric (locality; default SCOPE_LEVEL = 10 per level/wedge)

`scope_debt = SCOPE_LEVEL × (levels_excess + wedges)`. `levels_excess` = how far the declaration
could move down toward the narrowest block covering its uses (loop-aware: never narrowed into a
for/while body). `wedges` = unrelated definitions between a binding and what it connects to (its
dependencies above and its first use below); a sibling sharing a dependency — or, on the use side,
co-used at the same first-use site — is the same cluster and not a wedge. Functions/classes get the
dependency-side wedges only (declaring code is free of nesting). No per-line liveness.

### S1 — declared too high (used only in `if`)
```python
def f(flag):
    x = 1
    if flag:
        print(x)
```
`x`: decl func body, only use in if-body → levels_excess = 1 → **10**. (Nothing unrelated wedged —
the `if` header is not a definition.)

### S2 — tight, narrowest block
```python
def f(flag):
    if flag:
        x = 1
        print(x)
```
`x`: decl & use in if-body → levels 0, no wedge → **0**.

### S3 — value declared too early (an unrelated definition between it and its use)
```python
def f(p, q):
    x = 0
    r = p + q
    s = r * 2
    return x + s
```
`x` is used only in `return x + s`; `r` is wedged between `x` and that use and is NOT co-used there
(`r`'s own first use is `s = r * 2`), so `x` is misplaced → **10**. `s` is co-used with `x` at the
return → exempt. `r`/`s` themselves sit right after their dependencies → 0.

### S4 — module helper used in one function → FREE (declaration is free)
```python
def helper(x):
    return x + 1

def main():
    return helper(5)
```
`helper` is a function declaration → declaring it costs nothing; using it from one function is
**not** debt → **0**. (Nesting is never penalized; only locality is — declaring code is free of
nesting, so we never push the agent to destroy a reusable helper.)

### S5 — import used in one function → FREE
```python
import math

def area(r):
    return math.pi * r * r
```
An import is a declaration → free → **0**.

### S-DEPS — gap-to-deps (a definition wedged away from its dependencies)
```python
def a():
    return 1

def junk():       # unrelated, wedged between a and b
    return 2

def b():
    return a()    # b depends on a; junk sits between them → b misplaced → 10
```
`b` should sit right after its dependency `a`; the unrelated `junk` wedged in between is one unit of
debt → **10** (`SCOPE_LEVEL`). A sibling that SHARES a dependency (two functions both calling
`helper`) is the same cluster → order is free → no debt. The symmetric rule holds for variables:
`b = a + 1` with an unrelated assignment between `a` and `b` → `b` misplaced.

### S6 — unused binding is NOT scored (out of scope)
```python
def f():
    x = 1
    return 0
```
`x` is never used → **no scope-debt, no finding**. Unused detection is out of scope (flake8/ruff/
pyflakes handle it).

### S7 — parameter is excluded
```python
def f(a, b):
    return 0
```
`a`, `b` are parameters → NOT scored. **0**.

### S8 — use in two sibling branches (LCA)
```python
def f(flag):
    x = 1
    if flag:
        print(x)
    else:
        print(x)
```
`x` used in both branches → `min_block` = their common ancestor (function body) → levels_excess 0;
no unrelated definition wedged → scope_debt **0**. Asserts lowest-common-block (LCA) logic.

## Scope bindings (binding kinds & depth)

### SB1–SB4 — introduced targets are at their natural block (no false penalty)
```python
def f(xs):
    for i in xs:          # SB1 for-target
        print(i)
    with open("x") as h:  # SB2 with-as
        h.read()
    try:
        pass
    except E as e:        # SB3 except-as
        print(e)
    return [j for j in xs]  # SB4 comprehension target (own scope)
```
`i`/`h`/`e`/`j` have `decl_block` = the block they introduce → levels_excess 0, no scope-debt from
placement. An introducer target is positionally FIXED by its construct, so it accrues neither levels
NOR wedges — it can't be reordered.

### Loop variable does not wedge a binding placed right before the loop
```python
def f(xs):
    v = setup()           # immediately before the loop, used inside it
    for x in xs:
        noise = work()
        use(x, v)
```
The `for` target `x` is bound INSIDE the loop block (not the enclosing block), and an `intro` binding
accrues no wedges, so neither `x` nor in-loop `noise` wedges `v` — `v` is already maximally close →
**0**. (Contrast: two unrelated definitions BETWEEN `v` and the loop → `v` could move down to just
before the loop → 2 wedges. The loop is the floor for narrowing, not a free pass for the gap before
it.)

### SB5 — declaring a function is FREE (no nesting penalty)
```python
def f(flag):
    def helper():
        return 1
    if flag:
        return helper()
    return 0
```
`helper` is a function declaration → declaring code is free of nesting → **0**, regardless of where
in `f` it sits. Contrast a VARIABLE used only inside the `if`, which IS narrowable → `levels_excess`
(S1).

### SB6 — first-binding rule with conditional rebind
```python
def f(c):
    x = 0
    if c:
        x = 1
    return x
```
`decl_block` of `x` = function body (first binding at top), NOT the `if`; uses span both →
min_block = function body, levels_excess 0 → **0**.

### SB7 — depth across scope kinds (pins the model)
Each scope-introducing body is +1: function/class body and `if/for/while/with/try/except` bodies;
module = 0. Used to compute `levels_excess` consistently across boundaries.

### EC12 — levels_excess clamp: declared deeper than used → 0
```python
def f(c):
    if c:
        x = 1
    print(x)
```
`x` is bound in the `if` but used after it → `min_block` is shallower than the binding →
`levels_excess = max(0, …) = 0` (a possibly-unbound smell, out of scope) → **0**.

### EC13 — walrus `:=` is a binding, scoped tightly
```python
def f(data):
    if (n := len(data)) > 0:
        return n
    return 0
```
`n` is bound by `:=` and used right there → scope_debt **0**. Asserts walrus is recognized as a
binding.

### Loop-awareness — accumulator before a loop stays at 0
```python
def f(xs):
    acc = []
    for x in xs:
        acc.append(x)
    return acc
```
`acc` is used inside the loop, but a binding is never narrowed INTO a loop body (loop-carried state
legitimately lives outside) → levels_excess 0 → **0**.

## Declare-before-use (warning, no points)

### W1 — local use before assignment
```python
def f():
    print(x)
    x = 1
```
`x` read before its assignment, same function → **warning**.

### W2 — module-level forward ref (immediate)
```python
a = b
b = 2
```
`b` read at module level before declaration → **warning** on `b`.

### W3 — forward function reference (warning)
```python
def f():
    return g()

def g():
    return 1
```
`f` calls `g` defined later in the module (non-cyclic) → forward reference → **warning** (declare
callees before callers).

### W4 — mutual recursion (no warning)
```python
def a(n):
    return b(n)

def b(n):
    return a(n)
```
Deferred references both ways → an unavoidable cycle → **no warning**.

### W5 — default arg evaluated at def time
```python
def g(a=later):
    return a

later = 1
```
Default `later` is evaluated at def time → immediate use before declaration → **warning**.

### W6 — aug-assign before any binding
```python
def f():
    x += 1
    x = 0
```
`x += 1` reads `x` before it is bound → **warning**.

### W7 — call chain, declared bottom-up (good) — `fixtures/python/warn/w7_order_good.py`
`f1→f2→f3→f4→f5` with each callee declared ABOVE its caller. Every reference points to an
already-declared name → **0 warnings**.

### W8 — same chain, declared top-down (bad order) — `fixtures/python/warn/w8_order_bad.py`
The identical chain but each caller is declared above its callee, so every call is a forward
reference: `f1→f2`, `f2→f3`, `f3→f4`, `f4→f5` → **4 warnings**. Direct good-vs-bad contrast (the
only difference is 0 vs 4 warnings).

### W9 — class methods ordered (good) — `fixtures/python/warn/w9_class_order_good.py`
`handle` calls `self._validate`/`self._transform`, both declared ABOVE it → **0 warnings**.

### W10 — class methods random order (bad order) — `fixtures/python/warn/w10_class_order_bad.py`
Same class, but `handle` is declared above the helpers it calls → 2 forward references → **2
warnings**. Inherited methods (not in this class body) never warn; mutual recursion between methods
is exempt.

### EC14 — direct self-recursion → no warning
```python
def f(n):
    if n <= 0:
        return 0
    return f(n - 1)
```
`f` references itself (self-reference = cycle) → exempt → **no warning**.

## References model (`tests/m4_references.rs`)

Declare-before-use fires on ANY reference to a same-scope definition declared later — not only
calls. Header references (decorator / base / default) are attributed to the entity being defined;
type annotations are excluded.

### R1 — bare decorator, attributed to the entity
```python
@deco
def view():
    return 1
def deco(f):
    return f
```
`view`'s header references `deco` (defined below) → **warning ON `view`** (not `<module>`).
`@deco` and `@deco()` behave identically (the `()` is irrelevant to the reference).

### R2 — base class / default argument (forward)
```python
class C(Base): pass      # base below  -> warning on C
class Base: pass

def f(x=helper): return x   # default below -> warning on f
def helper(): return 1
```
A base class and a default argument are header references → warning on the defining entity.

### R3 — bare-name sibling reference (not a call)
```python
def build():
    return handler        # references handler (below) without calling it
def handler():
    return 1
```
`build` names `handler` below → **warning** (references, not just calls).

### R4 — `self.`/`cls.` member reference (not a call)
```python
class C:
    def a(self):
        cb = self.b       # references method b below, without calling it
        return cb
    def b(self):
        return 1
```
`self.b` (a read) references method `b` below → **warning on `C.a`**.

### R5 — recursive / forward type annotation → clean
```python
def link(n: Node) -> Node:
    return n
class Node:
    pass
```
Annotations are not references → **no warning** (recursive/forward types stay clean).

### R6 — header reference cycle → exempt
```python
@b
def a(): return 1
@a
def b(f): return f
```
`a`'s header references `b` and vice versa → an unavoidable cycle → **no warning** (like mutual
recursion).

## Declare-order direction — `--order` (`tests/m4_references.rs`)

The callee-vs-caller direction is the one conventional axis, a flag (`Weights.order`). Same code,
opposite verdicts:
```python
def callee():
    return 1
def caller():
    return callee()      # callee ABOVE caller
```
- **bottom-up** (default): callee above caller is in order → **0 warnings**.
- **top-down** (stepdown): wants the callee below → **warning on `caller`** (`DeclaredBeforeUse`).

Mirror the order (callee below caller) and it flips: bottom-up warns, top-down is clean. Mutual
recursion is exempt in BOTH directions. Locality/wedges and value use-before-binding never flip
(data flow, not convention).

## Data definitions (`tests/m4_scope.rs`, `tests/m4_references.rs`)

A module-/class-level value (a constant / attribute) is a DEFINITION, not a narrowed variable: it is
placed near its dependents and declared above its users, and it never narrows. Only function-LOCAL
variables narrow.

### D1 — function near shared data (placement)
```python
TIMEOUT = 30
def junk(): return 1        # unrelated, wedged
def a(): return get(TIMEOUT)
def b(): return get(TIMEOUT)
```
`a` and `b` read `TIMEOUT`; `junk` sits between `TIMEOUT` and them → each of `a`, `b` accrues one
gap-to-deps wedge → **20** total. (A function should sit next to the data it reads.)

### D2 — data declared below its user (declare-order)
```python
def fetch():
    return get(TIMEOUT)
TIMEOUT = 30
```
`fetch` reads `TIMEOUT` declared below → **declare-order warning on `fetch`**.

### D3 — module constant does NOT narrow
```python
CONFIG = 1
def outer():
    def inner():
        return CONFIG
    return inner
```
`CONFIG` is a data definition → **0** (not narrowed). Contrast a function-LOCAL used once in a
nested block, which DOES narrow.

### D4 — constant computed from a constant (its own gap-to-deps)
```python
DEFAULT = 10
JUNK = 99            # unrelated, wedged
TIMEOUT = DEFAULT * 2
```
`TIMEOUT`'s RHS depends on `DEFAULT`; `JUNK` sits between them → one gap-to-deps wedge on `TIMEOUT` →
**10**. A constant is placed next to the constants it is computed from, like a function next to its
callees.

### D5 — constant's RHS forward reference → ordered on the constant
```python
TIMEOUT = DEFAULT * 2
DEFAULT = 10
```
`TIMEOUT`'s RHS references `DEFAULT` declared below → **declare-order warning on `TIMEOUT`** (the
RHS is attributed to the constant it defines, not to `<module>`).

### D6 — a function-local's RHS belongs to the function
```python
def f():
    x = helper()
    return x
def helper(): return 1
```
`x` is a function-LOCAL (not a data definition), so `x = helper()` makes `f` — not `x` — depend on
`helper` → **declare-order warning on `f`** (and `f` is placed near `helper`). This is why value-RHS
attribution falls back to the enclosing scope inside a function.

## Suggestions — the metric naming the fix (`tests/core_defgraph.rs`, `tests/m4_scope.rs`)

Beyond the number, four `Category::Suggestion` findings (score 0) point at a safe, mechanical
refactor. Catalog: `tests/fixtures/python/catalog/`.

### ReorderBinding — push a scattered local down to its use
```python
def f():
    q = setup()          # bound here, first used only 3 definitions later
    a = step_one(); b = step_two(); c = step_three()
    log(a, b, c)
    return run(q)
```
`q`'s 3 use-side wedges are provably independent of it → suggest moving `q` to its first use →
`ReorderBinding{first_use, wedged: 3}` (plus the `Misplaced{3}` score). **Loop-aware:** if the first
use is inside a loop the declaration sits outside (a loop-carried accumulator), the move would reset
it each iteration → suggestion suppressed, the wedge debt stays.

### ExtractShared — pull shared infrastructure into its own module
```python
def shared(): return 1
def task_a(): return shared()
def task_b(): return shared()
def task_c(): return shared()
def task_d(): return shared()
```
`shared` is referenced by ≥4 distinct definitions in a file that carries placement debt → suggest
extracting it (per-file analysis then stops penalizing its now cross-file references). Self-recursion
is not sharing.

### CrowdedScope — bundle a bag of independent accumulators
```python
def crowded(rows):
    total = 0; count = 0; largest = 0; names = []; seen = set()
    for r in rows:
        total += r.value; count += 1; largest = max(largest, r.value)
        names.append(r.name); seen.add(r.id)
    return total, count, largest, names, seen
```
Independent accumulators wedging each other for ≥100 scope-debt → suggest one struct/dataclass
(N siblings collapse to one). Only INDEPENDENT-local debt counts; interdependent sequential locals
are not flagged (splitting wouldn't lower locality).

### NarrowToBlock — declare a too-high local at its use, inside the block
```python
def f(flag):
    x = compute()        # declared at the top, used only inside the `if`
    if flag:
        return use(x)
```
`x` is used one level deeper than it is declared → suggest declaring it at its first use inside the
block → `NarrowToBlock{first_use, levels: 1}` (alongside the `ExcessLevels{1}` score). The levels-term
twin of `ReorderBinding`; the narrow target caps above any loop, so the push-in is always safe
(loop-carried state has `levels` 0, so it is never flagged here).

## Findings model & rendering (decoupled)

Core emits a sorted `Vec<Finding>` and never formats display strings. Tests split in two:

- **Core (structured):** assert the exact findings. A scope-debt finding is
  `Finding{category=ScopeDebt, entity, kind=Binding, score, reason=ExcessLevels{n} | Misplaced{n}}`
  — sorted by (file, line, entity). Assert category, entity, kind, score, reason — no wording.
- **Render (golden):** feed a fixed `Vec<Finding>` into each renderer, snapshot the output.
  - `text` default layout; `json` schema; empty list → "clean" summary;
  - `--top N --by function|class|file` ranks by scope-debt (bindings roll into their function,
    methods into their class); sort stability (same input → identical bytes).
- **Parse error** (ROB1): a file with a syntax error → one `Finding{ParseError, severity=Error}`;
  other files are still analyzed and reported.

### Render / CLI cases

- **OUT1 — summary:** a project → Σ scope-debt headline + per-file rollup; warnings counted
  separately.
- **OUT2 — sort stability:** multiple files → findings sorted by (file, line), byte-stable.
- **OUT3 — json schema:** fixed `Vec<Finding>` → stable JSON (snapshot).
- **OUT4 — empty project:** no `.py` files → no findings, "clean" summary, exit 0.
- **OUT5 — exit codes:** default 0; `--error` → 1 if any Error-severity finding (default: ParseError
  only).
- **OUT7 — top-N ranking:** `--top 2 --by function` → the two highest scope-debt functions, sorted
  by score (desc). `--by class` / `--by file` rank those rollups.

### Z — non-default weight changes the score
With `[tool.ventouse.weights] scope_level = 5`, case S1 → `5 × 1` = **5**. Asserts the scoring constant
is wired from config.

## Example fixtures (full-file, end-to-end)

### Rule catalog — `fixtures/python/catalog/` (primary)
Tiny good-vs-bad examples, one per rule (`01_local_scope` · `02_definitions` · `03_free_and_crowded`
+ README). Each `*_bad` triggers exactly one named finding (in a comment), each `*_good` scores 0;
verified against real `--all` output. The authoritative, current reference.

### Good — `fixtures/python/examples/good.py`
A functional core declared bottom-up, every binding next to its uses → scope-debt **0**, warnings
**0** (the end-to-end "ideal code" check in `m3_render.rs`).

## Per-language axiom coverage

The core verdict is language-agnostic; per-language fixtures prove the locality axioms hold through
each language's SURFACE.

**Scope-debt in block-scoped languages** (Rust, C++ done; JS planned): the model is the same
everywhere — `levels` (narrowest block, loop-aware) + `wedges`. The only difference is that in block-
scoped languages the `levels` term narrows the REAL runtime scope, so it bites harder than in
function-scoped Python. There is no per-line liveness in any language.

**Declare-before-use**: a reading-convention warning in every language, including order-free ones
(Rust items, hoisted JS function decls); direction is the `--order` flag; cycles exempt.

**Rust** is the second working frontend (`tests/rust_lang.rs`): block-scoped `let`, `impl T` → class
scope `T` (`self.`/`Self::` member-resolved), `const`/`static` as data, and `self.field` correctly
treated as a field, not the same-named method. **C++** is the third working frontend
(`tests/cpp_lang.rs`), via libclang + `compile_commands.json`. JS/TS fixtures land with their milestone.

## Coverage checklist (rule → case)

- Scope-debt levels: S1, S2, S8, SB6, EC12, loop-awareness · wedges: S3, S-DEPS · free declarations:
  S4, S5, SB5 · introducer targets (no levels AND no wedges): SB1–4, EC13, loop-variable-no-wedge ·
  excluded: S6 (unused), S7 (params) · depth model: SB7.
- Global/nonlocal + scoping: G1–G5 (pinned shared state, cross-function, class-attr shadow, `self.x`
  edge-not-narrow). Data definitions: D1 (placement near shared data), D2 (declare-order on data),
  D3 (module constant not narrowed — only locals narrow), D4 (constant's own gap-to-deps), D5 (RHS
  forward ref ordered on the constant), D6 (function-local RHS belongs to the function).
- Declare-before-use: W1–W10, EC14 (self-rec); references model R1–R6 (decorator/base/default
  attribution, bare-name + self-member references, annotation exclusion, header cycle); direction
  flag (`--order` bottom-up/top-down, cycles exempt both ways).
- Suggestions: `ReorderBinding` (incl. loop-carried suppression), `NarrowToBlock` (levels-term twin),
  `ExtractShared` (≥4 distinct, self-rec excluded), `CrowdedScope` (independent-bag only).
  Catalog `01`–`03`.
- Findings/render/CLI/robustness: OUT1–5/7, Z, ROB1.

## Frontend lowering completeness

The Python frontend now lowers every expression/statement that carries references:
- `yield` / `yield from` and slice bounds (`a[i:j]`) — recurse children;
- f-string interpolations (`f"{x}"`) — the `{expr}` parts;
- comprehension / generator / lambda bodies — their own Py3 scope (first iterable in the enclosing
  scope; targets/guards/result inside), so a value read only there narrows toward it, like a closure
  capture (`value_used_only_in_comprehension_narrows_toward_it`);
- `match` / `case` — subject + per-case block; patterns bind captures and read literal/class/key
  references (`match_case_body_narrows_a_value_used_only_there`, `match_subject_is_a_reference`).

Residual (inherent, not a hole): references inside a lambda / comprehension / closure BODY are
attributed to that scope, so they don't appear in the call graph — captures still narrow. Function
data-dependencies (depending on a constant it READS) are handled by the data-definition model.
