# Core rules — "if … then …"

The complete decision logic of the language-agnostic core, extracted from the source:
[scopegraph.rs](src/core/scopegraph.rs), [wedge.rs](src/core/wedge.rs),
[placement.rs](src/core/placement.rs), [declorder.rs](src/core/declorder.rs),
[analyze.rs](src/core/analyze.rs), [score.rs](src/core/score.rs).

## Binding classification (what counts as what)

- **If** a name is bound at module / class / namespace (`mod`) scope → **then** it is a *data
  definition* (constant/attribute): placed and ordered, but **not** narrowed (free of nesting).
- **If** the binding is a `def`/`class` → **then** it is a *Decl* (code): free of nesting, never
  penalized for levels, and becomes an entity (function/method/class) that `placement`/`declorder`
  place and order.
- **If** the binding is an `import` → **then** it is a distinct *Import* kind: also free of nesting,
  but **no** entity is created — it is neither placed nor ordered.
- **If** the binding is a local variable inside a function → **then** it is a *Value*: scored as
  `levels_excess + wedges`.
- **If** a name is declared `global`/`nonlocal` → **then** a write to it is a *use* of the outer
  binding (not a local one), and it is **pinned** — never narrowed.
- **If** the binding is a parameter → **then** it is excluded from analysis entirely.
- **If** one name is bound several times → **then** the earliest declaration is kept (first-binding
  rule).

## Locality metric (per Value binding)

**levels_excess** (excess nesting):

- **If** the variable is declared higher than the narrowest block (LCA) covering all its uses →
  **then** `levels_excess = depth(narrow_target) − depth(decl_block)`.
- **If** the narrow target coincides with the declaration block (nowhere to narrow to — e.g. the LCA
  of the uses is already at the declaration's level) → **then** `levels = 0`.
- **If** there is a loop boundary on the path down toward the use → **then** the narrow target is
  capped **above** the loop (loop-carried state is not pushed inside).
- **If** the binding is an *introducer* (loop target / `with` / `except` / walrus) **or** pinned →
  **then** `levels = 0` (positionally fixed by the construct).

**wedges** (junk between a thing and what it connects to):

- **If** an unrelated sibling sits between the variable and its *nearest dependency above* → **then**
  +1 wedge (dep-side).
- **If** an unrelated sibling sits between the variable and its *first use below* → **then** +1 wedge
  (use-side).
- **If** the variable has no dependency above → **then** dep-side = 0 (free).
- **If** a sibling is itself a dependency, **or** shares a dependency with the target, **or** is
  co-used at the target's first-use site → **then** it is *not* a wedge (one cluster).
- **If** a sibling is in a "cousin" block (a different, mutually-exclusive `match`/`if` branch) →
  **then** it is not on any execution path → not a wedge.
- **If** the binding is an introducer → **then** wedges = 0 (otherwise junk inside a loop would
  "wedge" the loop variable, which cannot be moved).

## Emitted findings

- **If** `levels_excess > 0` → **then** a `ScopeDebt` finding with `score = scope_level * levels_excess`.
- **If** `wedges > 0` → **then** a `ScopeDebt` finding with `score = scope_level * wedges`. (One
  binding can produce both.)
- **If** `levels == 0 && wedges == 0` → **then** no finding.
- **If** a name is read before its first binding in the same scope → **then** a `DeclBeforeUse`
  warning (`UseBeforeDecl`, score 0).

## Definition placement (placement / gap-to-deps)

- **If** unrelated siblings sit between a definition (function/class/data) and its nearest *real*
  dependency above → **then** a `ScopeDebt` finding (Misplaced) with the wedge count.
- **If** there is no real dependency above the definition (only ubiquitous accessors) → **then**
  count = 0.
- **If** a class-member method has fan-in ≥ `accessor_fanin` (3) and depends only on other accessors
  (fixpoint seeded by the leaves) → **then** it is **pinned**: it does not anchor a span, is not
  counted as a wedge, and is never reordered (a "field dressed as a method").

## Declare-before-use (the one conventional axis)

- **If** a caller references a callee declared in the same scope → the direction is checked:
  - **If** `order = bottom-up` and the callee is **below** the caller → **then** a `ForwardRef`
    warning (callee below).
  - **If** `order = top-down` and the callee is **above** the caller → **then** a `ForwardRef`
    warning (callee above).
- **If** the reference is cross-module (another file / import) → **then** it is not checked.
- **If** the callee can reach the caller (a cycle / mutual recursion / self-reference) → **then** it
  is exempt (no warning).
- **If** one caller references one callee multiple times → **then** one warning per pair.

## Suggestions (actionable advice)

- **`ExtractShared`** — **if** the file carries placement debt **and** a definition (not a method, not
  a module) is referenced by ≥ `shared_callers` (4) distinct others, **or** ≥ `shared_uses` (8) times
  total by ≥2 others → **then** advise "extract into its own module".
  - **If** the file carries no placement debt → **then** ExtractShared is not emitted (a tightly
    clustered helper needs no extraction).
- **`CrowdedScope`** — **if** a function's *independent* locals (empty deps) together carry ≥
  `crowded_scope` (100) debt → **then** advise "bundle them into a struct/dataclass".
  - **If** the debt comes from interdependent, sequential locals → **then** the function is **not**
    flagged (nothing to bundle).
- **`ReorderBinding`** (local) — **if** `levels_excess == 0`, use-side wedges ≥ `reorder_wedges` (3),
  and the first use is below the declaration → **then** advise "push the declaration down to its use".
- **`ReorderBinding`** (definition) — **if** a function/class is declared above its first use with ≥
  `reorder_wedges` unrelated siblings in between → **then** the same advice at the definition level.
  - **If** the reference is *above* the definition → **then** it is a forward reference (a `declorder`
    matter), not a reorder → skipped.
- **`NarrowToBlock`** — **if** `levels_excess >= narrow_levels` (1) and the first use is below the
  declaration → **then** advise "declare it at its first use, inside the block".
- **If** a local's first use is *inside a loop* (the move would cross a loop boundary) → **then**
  use_wedges is zeroed (the seed cannot be pushed down — it would reset every iteration); the wedge
  debt still stands, fixed via CrowdedScope.

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
