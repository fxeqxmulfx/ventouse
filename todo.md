# ventouse — plan (v5, locality-only, single-graph)

Multi-language code analyzer in Rust (Python, Rust, C++ done; JS/TS planned): a **locality** metric
(scope-debt) + a declaration-order warning + actionable refactor suggestions. Language-agnostic core
+ frontends on native parsers behind a common trait.

Purpose: a **locality linter for code an LLM agent writes** — the agent is the *writer*, a human (or
the reviewing agent) the *reader*. The reading model it optimizes (linear, limited working memory) is
a human one; that's the point — it nudges AI-written code toward a layout a reviewer accepts. It
scores how far each definition sits from what it connects to (per-entity → a project total + a
worst-offenders ranking) and, the real product, emits **suggestions that name the fix** (reorder a
binding, extract a shared helper, bundle a crowded scope). Deterministic, raw numbers, no pass/fail
thresholds — a guide, not a gate, used advisory (one signal, not the whole of readability). The one
genuinely conventional axis (callee-vs-caller order) is a **flag** (`--order`), not a law.

> History: ventouse originally also scored a "dirt" (side-effect/purity) metric, removed in full. The
> separate call-graph walk + name resolver it used were removed too — the locality analysis now runs
> on ONE structure (the scope graph), from which the definition-reference graph is projected. The
> core has direct language-agnostic unit tests and 100% coverage.

## Axioms

Everything is DERIVED from four irreducible axioms; two named corollaries follow.

- **L0 — no rule, driven to 0, may make the code worse.** The governing meta-axiom (the
  anti-contortion guard, a.k.a. "the metric must not punish good patterns"). The score is a guide the
  writer optimizes *toward*, not a gate to game: legitimate structure stays free, and a "fix" the
  metric rewards must be a real improvement (every suggestion is a safe, mechanical refactor —
  decomposition, hoisted invariants, and entry-point ordering are never penalized into worse code).
- **L1 — locality is the quality measured.** A definition's cost is its displacement from the
  position that minimizes distance to what it connects to: the narrowest scope covering its uses,
  right after the definitions it depends on, right before its first use.
- **L2 — substrate + completeness.** The program is (bindings, references, a scope/block tree,
  textual order); a frontend emits exactly that, COMPLETELY. No construct is exempt — every binding
  and every reference is in the graph. (A language supplies only a profile mapping its syntax to
  this substrate.)
- **L5 — definition order is a reading convention (configurable).** A reference to a definition on
  the "wrong" side of the caller is a warning, unless part of an unavoidable cycle. The direction is
  the one preference-laden axis and is a flag (`--order`): **bottom-up** (default — callees above
  callers, define-before-use) or **top-down** (stepdown / overview-first — callees below). Everything
  else (locality, wedges, value use-before-binding) is data flow and never flips.

Corollaries (derived, not independent):
- **C1 — structural, not lines** (from L0): displacement is counted in scope levels + wedged
  unrelated definitions, never raw lines, so legitimate spacing (accumulators, init-before-`with`)
  is free.
- **C2 — definitions are free of nesting; only LOCAL variables narrow** (from L0): a definition is a
  function/class OR a module-/class-level value (a constant / attribute). Definitions don't narrow —
  they are placed near their dependencies and declared above their users (placement + declare-order).
  Only a FUNCTION-LOCAL variable narrows toward its use. (⟹ a module function or constant used once
  is free / placed; only a local variable moves into the block that uses it.)

⟹ scope-debt = **levels** (narrowest block, values only, loop-aware) + **wedges** (unrelated
definitions wedged between a definition and its dependencies above / first use below; values both
sides, defs the dependency side only) + **declare-before-use** (forward refs warn, cycles exempt).
One unit (`SCOPE_LEVEL`) per level/wedge.

Output policy (from the Purpose, not an axiom): **Measure, don't judge** — raw numbers, NO
thresholds; per-entity → totals + rankings; the agent/human minimizes. Core emits a structured
`Vec<Finding>`; rendering is separate.

The unit of analysis is a **named entity** — function / method / class / synthetic `<module>` (plus
`Binding` for the scope analysis).

## Locked decisions

| Topic | Decision |
|-------|----------|
| Language/foundation | Rust; language-agnostic core + frontends behind a `Frontend` trait |
| Parsers | Python: **ruff** (`ruff_python_parser`, git-pinned to `astral-sh/ruff` tag `0.15.8`); JS/TS: `oxc`; Rust: `syn`; C++: `libclang` (clang crate) |
| Scope of analysis | Per-module (locality is same-file). The whole-module scope graph is the single source; cross-file references don't affect locality |
| Single graph | One walk builds the `ScopeGraph` (bindings + references over a block/scope tree). From it the core derives: value scope-debt, the definition-reference edges, the entity list, and value declare-before-use — all with ONE lexical resolution (no divergent resolver). `placement`/`declorder` consume a thin per-module `DefGraph` (entities + edges) projected from it |
| Reference edges | The **references model**: an edge exists for ANY reference to a same-module DEFINITION (call, bare name, decorator, base, default, `self.`/`cls.` member) — not only calls. A definition is a function/class OR a module-/class-level value (a constant / attribute = DATA definition); a function-local variable is not. So a function reading a module constant is an edge (gap-to-deps + declare-order on the data). Header references are attributed to the defined entity (`OpenAttrib`); type annotations are excluded. Both `declorder` and `placement` use this one edge set |
| Data definitions | A module-/class-level value (constant / attribute) is a full definition: placed near its dependents AND its own dependencies (its RHS is attributed to it via `OpenAttrib { fallback_to_scope }`), declared above its users, NOT narrowed. Function-LOCAL variables narrow; their RHS belongs to the enclosing function, not the local |
| Scope metric | Locality: **levels** (narrowest block, VALUES, loop-aware) + **wedges** (unrelated definitions between a definition and its dependencies/first use). Values get both wedge sides; functions/classes get the dependency side only — declaring code is free of nesting (C2). One unit (`SCOPE_LEVEL`) per level/wedge; no per-line liveness |
| Scope coverage | Variables (levels + wedges); functions/classes (dependency-side wedges only, via `core::placement`); imports & parameters excluded |
| Declaration order | A reading-convention warning, ALL languages, even where the compiler is order-free (Rust items). Direction is a flag (`Weights.order` / `--order`): **bottom-up** default (callees above callers) or **top-down** (stepdown — callees below). Cycles (mutual recursion / self-reference) exempt either way. Value use-before-binding (a read before assignment — a runtime hazard) is invariant, not part of this flag |
| Output / totals | Core computes per-entity findings; scores roll up to file/class/project totals + a single combined headline number (Σ scope-debt), top-N rankings (worst file/class/function). The VIEW (total / top-N / full; text/json) is flag-selected. Warnings counted separately |
| Scoring constant | `SCOPE_LEVEL` named, arbitrary default (10); overridable via `[tool.ventouse.weights]` config |
| Findings vs rendering | Core emits a structured `Vec<Finding>`; display (text/json) is a separate module |
| Thresholds | NONE — report every finding with its raw score; the tool measures, the user judges |

## Rule semantics (specification)

scope-debt is a penalty (points); declare-before-use is a warning (no points).

### Locality / scope-debt (⟸ L1, C1, C2)

```
scope_debt(binding) = SCOPE_LEVEL × (levels_excess + wedges)
```
A binding with ZERO uses is NOT scored — unused detection is out of scope (flake8/ruff handle it).
Parameters are excluded. Two parts, same unit (`SCOPE_LEVEL` per level/wedge — no per-line liveness):

**`levels_excess` — narrowest block (FUNCTION-LOCAL variables only).** How many block levels a local
variable's declaration could be pushed DOWN toward its uses. `min_block` = the lowest common block
(LCA) covering all uses; `levels_excess = depth(target) − depth(decl_block)`, clamped to 0 when
`min_block` is not strictly inside `decl_block` (a possibly-unbound smell, not debt). Only a variable
INSIDE a function narrows — a module-/class-level value is a DATA definition (see below), not narrowed.
**Loop-aware:** a binding is never narrowed INTO a loop body (for/while) — loop-carried state and
loop-invariants legitimately live outside the loop, so `target` is the deepest block reachable from
`decl_block` without crossing a loop boundary. Only nesting into CONDITIONALS
(if/elif/else/try/except/with) is penalized.
**Nonlocal pinned:** a closure variable referenced via `nonlocal` (shared state of an enclosing
function) is PINNED (levels 0), never narrowed into the inner function. `global`/`nonlocal` names are
not local bindings: a write to one is a use of the outer binding, so state mutated in one function
and read in another is seen as cross-function.

**`wedges` — unrelated definitions between a binding and what it connects to.** A definition should
sit right after its dependencies (sources) and right before its first use; an unrelated sibling
definition wedged into either gap is one unit of debt.
- *dependency side* (between the binding's NEAREST dependency above and the binding): a wedge `G`
  counts unless `G` is a dependency of the binding or SHARES a dependency with it (same cluster →
  order free). Anchoring to the *nearest* (closest) dependency — not the topmost — is deliberate: a
  far-away SHARED dependency (a utility called by many definitions, which cannot sit adjacent to all
  of them) must not stretch the span across the whole file. So a definition tight below its nearest
  dependency scores 0 even if it also uses a utility declared far above.
- *use side* (between the binding and its first use): same, plus `G` is exempt if it is CO-USED at
  the binding's first-use site (several values used together are one cluster).
- Variables get both sides; functions/classes get the dependency side only (computed on the call
  graph in `core::placement`). A shared/co-used sibling is never a wedge, so clean interleaved code
  and functional decomposition stay at 0.
- *cousin-block exemption*: a candidate sibling wedges a variable only if it is in the SAME block or
  an ENCLOSING one. A binding in a cousin block — a different, mutually-exclusive branch (another
  `match`/`if` arm) — is not on any execution path from the variable to its dependency/use, so it is
  not a wedge. (Surfaced by dogfooding the Rust frontend on idiomatic big-`match` code; it improves
  Python too — a binding inside one `if` arm no longer wedges across it.)

What counts as a binding: assignment, `for … in`/`for … as`, `with … as`, `except … as`, walrus
`:=`, comprehension target, `def`/`class`, `import`. With multiple/conditional assignments,
`decl_block` = the block of the FIRST (textual) binding. Introducer targets (loop/`with`/`except`/
walrus) are positionally FIXED by their construct → no `levels` penalty AND no `wedges` (you can't
reorder a loop variable; an in-loop sibling must not "wedge" it). A `for` target is bound INSIDE the
loop block (the iterable evaluates in the enclosing scope) — same function scope, deeper block — so it
never wedges a binding sitting right before the loop. Comprehension targets have their own scope (Py3).

Per-language note: in Python `if/for/while/with` are logical blocks (function scope), so `levels`
is a readability nudge. In block-scoped languages (JS `let`/`const`, Rust, C++) the same `levels`
narrows the REAL runtime scope — same metric, just more impactful. There is no separate liveness in
any language. Lambda/comprehension have their own scope; `async def` = `def`.

### Declare-before-use (warning) (⟸ L5) — the references model

A reference to a same-scope definition on the convention's "wrong" side → warning. "Reference" is ANY
mention of the name, not only a call: a call `g()`, a bare name `x = g`, a decorator `@g`, a base
class `class C(g)`, a default argument `def f(x=g)`, or a `self.`/`cls.` member. Order matters for
HUMAN readability — so this applies even where the compiler is order-free (Rust items, hoisted JS
function decls). The **direction is a flag** (`Weights.order`): bottom-up (default) warns when the
callee is BELOW its caller (`Reason::UseBeforeDecl`, "used before its declaration"); top-down warns
when it is ABOVE (`Reason::DeclaredBeforeUse`, "declared above the code that uses it"). The cycle
exemption and attribution rules below are identical in both directions.

- **Exemption:** references that are part of an unavoidable definition cycle (mutual recursion /
  self-reference / mutual header references), detected by reachability over the reference graph.
- **Attribution:** a reference in a definition's HEADER (decorator / base / default) resolves in the
  enclosing scope but is attributed to the entity being defined — `@g` over `view` with `g` below
  warns ON `view`, not `<module>`. `@g` and `@g()` behave identically.
- **Type annotations are excluded** — `def f(x: T) -> T` does not reference `T`, so recursive and
  forward-declared types stay clean.
- **Same-scope only:** a reference whose target lives in a different scope (a method naming a module
  function, a nested function naming an outer sibling) creates a graph edge but is filtered out — no
  warning. Not applied cross-module (imports). Inherited methods never warn.

(Variable use-before-binding — a local read before its first assignment in the same scope — is the
same model applied to value bindings.)

## Per-language mapping (⟸ same axioms; only the SURFACE differs)

The core is identical across languages — entities (function/method/class), the reference graph (same-
module references), scope-debt, and declare-before-use. A language implements `ScopeLang` (lowers its
AST into `Action`s); only the SURFACE differs: block-scoping, what introduces a binding, and what a
method/class/constructor is.

### Python (ruff — `ruff_python_parser`) — baseline
The reference language, fully specified above.

### JavaScript / TypeScript (oxc)
- Entities: `function` decls, arrow/function expressions, methods, `class`.
- Scope: `let`/`const` are block-scoped → `levels` narrows the REAL runtime scope (more impactful
  than Python). `var` is function-scoped/hoisted → like Python. Same metric (levels + wedges).
- Declare-before-use: `let`/`const` TDZ and `var` hoisting both make use-before-decl a bug →
  warning. Function decls hoist, but the textual "callees before callers" rule still applies
  (cycles exempt).
- TS adds no new locality surface beyond JS (type-only constructs — interface/type/declare/abstract/
  overload — are erased and are never entities).

### Rust (syn) — DONE (`src/lang/rust/`: `prim` vocabulary · `lower` rules · `mod` pipeline)
- Entities: `fn`, methods in `impl` blocks; `struct`/`enum`/`union`/`trait` are the class-like unit;
  `const`/`static` (module + associated) are DATA definitions. `impl T` opens a class scope `T` so
  methods are `T::method` and `self.`/`Self::` are member-resolved.
- Scope: block-scoped, expression-oriented; `let` narrows its REAL enclosing block (more impactful
  than Python). `if let`/`while let`/`match`/`for` bind in the opened block; closures are own scopes.
- Declare-order: the "callees before callers" warning is emitted for items even though Rust resolves
  them order-free (it is a HUMAN top-down signal); cycles exempt. A `let`'s RHS is attributed to the
  binding only when it is a data definition (module/impl `const`), else to the function.
- Macros whose body parses as an expression list (`vec!`, `format!`, `assert_eq!`, …) have their
  references recovered (`lower::macro_uses` parses the token stream); only non-expr-list macros
  (`matches!`, custom DSLs) remain a hole. Type references (signatures/generics) are excluded.
- Dogfood: `ventouse <dir> --lang=rust` ran on ventouse's own `src/` AND external projects, driving core
  fixes (cousin-block wedge exemption, nearest-dep anchoring, let-chain lowering, `self.field`-is-not-
  a-method, loop-carried `ReorderBinding`) + all four suggestions. Acting on `ExtractShared` is
  exactly why `lang/rust` is split (`prim` vs `lower`): the vocabulary's references became cross-file
  (un-penalized), cutting the frontend's placement debt.

### C++ (libclang — the `clang` crate)
- Parser: libclang gives a real, semantically-resolved AST. Cost: a system libclang dependency, and
  a file usually needs the right flags (`-std`, `-I`) — feed it `compile_commands.json` or defaults;
  files that won't parse → `ParseError`, analysis continues.
- Entities: functions, methods, classes/structs.
- Scope: block-scoped → `levels` narrows the real runtime scope.
- Declare-before-use: free functions/variables already require a prior declaration (compiler error →
  skip). Inside a class, methods may call members defined later → the ordering warning applies there.

## Architecture

```
src/
  core/    raw.rs (IR + Frontend trait) · scopelang.rs (ScopeLang trait + Action driver)
           scopegraph.rs (the ONE graph: bindings/refs/blocks + scorer → ScopeOutput)
           defgraph.rs (entities+edges projection) · placement.rs · declorder.rs
           wedge.rs (ONE gap-to-deps implementation, shared by values + definitions)
           model.rs (EntityKind/Reason) · score.rs (Weights) · finding.rs · analyze.rs (pipeline)
  render/  mod.rs · text.rs · json.rs                                          (display only)
  lang/    mod.rs · python/{prim,lower,mod}.rs (ruff AST → Actions)
           rust/{prim,lower,mod}.rs (syn AST → Actions) · cpp/{prim,lower,mod,compdb}.rs (libclang)
           [later] javascript/ (oxc)
  config.rs · discover.rs · main.rs
tests/     core_*.rs (language-agnostic core, no parser) · m*.rs (Python e2e) · fixtures/<lang>/...
```

**One walk, one graph.** A frontend implements `ScopeLang` — it lowers each of its syntax nodes
into language-agnostic `core::scopelang::Action`s (`Bind` / `Use` / `OpenBlock` / `OpenScope` /
`Close`, plus `OpenAttrib`/`CloseAttrib` to re-attribute a definition's header references to the
entity). The core driver interprets them, managing all block/scope/depth/attribution bookkeeping,
and builds a `ScopeGraph`. `ScopeGraph::score()` then derives EVERYTHING in one pass (`ScopeOutput`):
- value **scope-debt** (levels + wedges);
- the **definition-reference edges** — ONE edge per reference to a same-module DEFINITION (code or
  module/class data; the references model); `member` uses resolve in the enclosing class; one lexical
  resolution, so shadowing is consistent;
- the **entity list** (functions/methods/classes + module/class data definitions);
- value **declare-before-use** (a same-scope use before the first binding).

Pipeline: discover → frontend (lower → `ScopeGraph` → `ScopeOutput`, carried on `RawModule`) →
`core::analyze`: scope-debt + value warnings come straight from `ScopeOutput`; a per-module
`DefGraph` (entities + edges) drives `placement` (gap-to-deps) + `declorder` (forward refs) →
**sorted `Vec<Finding>`** → render. No separate call-graph walk, resolver, or entity arena.

**Findings (core output, render-agnostic).** A `Finding` carries:
- `file`, `line`, `entity` (qualname), `entity_kind` (Function/Method/Class/Module/Binding);
- `category`: `ScopeDebt` | `DeclBeforeUse` | `Suggestion` | `ParseError`;
- `severity`: `Error` | `Warning` | `Info`. Defaults: ScopeDebt = Info; DeclBeforeUse = Warning;
  Suggestion = Info; ParseError = Error;
- `score: u32` (for ScopeDebt; 0 for DeclBeforeUse/Suggestion/ParseError);
- `reason`: a stable reason code + structured detail — `ExcessLevels{n}` (nesting),
  `Misplaced{n}` (n unrelated definitions wedged), `UseBeforeDecl` (bottom-up order / value read
  before binding), `DeclaredBeforeUse` (top-down order), `ExtractShared{n}`, `CrowdedScope{n}`,
  `ReorderBinding{first_use, wedged}`, `NarrowToBlock{first_use, levels}`, `ParseError`. The human
  wording lives in `render`, not core.

**Suggestions (actionable, not penalties).** Beyond scoring, the analysis emits suggestions — the
metric pointing at *what to do*, not just the number:
- `ExtractShared{n}`: a definition referenced by ≥4 DISTINCT others (shared infrastructure — a node
  type, a universal helper) in a file that carries placement debt. Moving it into its own module
  turns those references cross-file (per-module analysis doesn't penalize them) and localizes the
  rest. Self-recursion is not sharing (distinct referrers, excluding self).
- `CrowdedScope{n}`: a function holding a bag of ≥100 scope-debt worth of INDEPENDENT mutable
  accumulators (each reads nothing, yet they wedge each other). Group them into a struct — that
  collapses N siblings into one and removes the mutual wedging (a `let mut` soup is a real smell, not
  a floor). Only independent-local debt counts: a function crowded by interdependent, SEQUENTIAL
  locals isn't flagged — splitting it is a maintainability call that *raises* placement debt (its new
  helpers sit far from shared resolvers), so it doesn't lower locality. (Found by acting on the
  suggestion: bundling `parse_args` dropped it 180→20; splitting `score` raised the total — so the
  suggestion was sharpened to fire only where the fix reliably reduces the metric.)
- `ReorderBinding{first_use, wedged}`: a local declared far above its first use, with ≥3 unrelated
  definitions in between (the use-side wedge). Those wedges are provably independent of the binding —
  anything that used it would pull `first_use` up — so pushing the declaration DOWN to its first use
  is a safe, mechanical move that erases exactly that gap. The actionable face of a large use-side
  wedge, and the mirror of `UseBeforeDecl` (a definition below its user) at the other extreme: both
  are "definition not where it connects." Score is unchanged; the suggestion just names the fix.
  **Loop-aware:** suppressed when the first use can't be reached without crossing a loop boundary — a
  loop-carried accumulator's seed (`total = 0`) can't move into the loop (it would reset each
  iteration). The wedge debt still stands there; bundling (`CrowdedScope`) is the right fix, not
  reordering. (Surfaced by writing the example catalog.)
- `NarrowToBlock{first_use, levels}`: a local used only inside a block nested `levels` deeper than its
  declaration → declare it at its first use, inside that block, shrinking its live range. The
  levels-term twin of `ReorderBinding`; the narrow target already caps above any loop boundary, so a
  flagged binding is always safe to push in (loop-carried state has `levels` 0).

**Report / output.** The renderer presents the VIEW the caller asks for, via flags:
- `--format text|json` (default text);
- view: `--summary` (Σ scope-debt headline + per-file rollup), `--top N --by function|class|file`
  (ranked worst offenders), `--all` (every finding). Default = summary.
- warnings are counted, not summed into the points total.
Sorting: by (file, line) for `--all`; by score for `--top`. No thresholds.
Exit code: 0 by default; `--error` → 1 if any **Error**-severity finding (by default only
`ParseError`).

**Scoring constant** (default arbitrary — tune later). Held in a `Weights` struct in core (with
`Default`); overridable from config.

| Const | Default | Meaning |
|-------|---------|---------|
| `SCOPE_LEVEL` | 10 | per scope-debt unit (excess nesting level, or unrelated wedged definition) |

```toml
[tool.ventouse.weights]
scope_level = 10
```

## Performance

Stance: build on perf-friendly foundations from day one, defer heavy optimization until benchmarks.
- Parallel per-module lowering via `rayon` (file = task) — each module's scope graph is independent.
- ID/arena model: the scope graph + `DefGraph` use `Vec`s + `usize` indices (no `Rc`/pointer graphs).
- `FxHashMap`/`FxHashSet` (rustc-hash); intern names/strings.
- Drop the AST after lowering to `RawModule`.
Deferred: salsa/incrementality, persistent caches, custom allocators, SIMD.

## Test matrix (expected numbers = specification)

Default weight: `SCOPE_LEVEL = 10`. (Tests assert the defaults.) See `tests/DESIGN.md` for the
worked cases.

### Scope metric

| # | Case | Expected |
|---|------|----------|
| S1 | var at function top, used only inside an `if` | levels_excess 1 → 10 |
| S2 | var declared and used in the narrowest block | 0 |
| S3 | var used late, an unrelated def wedged before its use | wedge → 10 |
| S4 | module function used in only one function | 0 (declaring code is free of nesting) |
| S5 | import used in only one function | 0 (declarations are free) |
| S6 | var assigned, never used | NOT scored (unused is out of scope) |
| S7 | a function parameter | NOT scored (excluded) |
| S8 | var used in two sibling branches | min_block = common ancestor → 0 |
| S-DEPS | a definition with an unrelated sibling between it and its dependencies | wedge → 10 |
| SB1–4 | introducer targets (for/with/except/walrus) at their natural block | 0 |
| SB5 | declaring a function is free of nesting | 0 |
| SB6 | first-binding rule with conditional rebind | levels 0 |
| SB7 | depth model (each scope-introducing body +1; module = 0) | — |
| EC12 | declared deeper than used → clamp | 0 |
| EC13 | walrus `:=` recognized as a binding, scoped tightly | 0 |

### Declare-before-use (warning, no points)

| # | Case | Expected |
|---|------|----------|
| W1 | in a function: `print(x); x = 1` | warning (use before decl) |
| W2 | module-level: `a = b; b = 2` | warning on `b` |
| W3 | `f` calls `g` defined later (non-cyclic) | warning (declare callee first) |
| W4 | mutual recursion at module level | no warning (unavoidable cycle) |
| W5 | `def g(a=later): ...; later = 1` | warning (default evaluated at def) |
| W6 | `x += 1` before `x = 0` (aug-assign before binding) | warning (use before decl) |
| W7 | chain f1→…→f5, callees declared above callers (bottom-up) | 0 warnings (good) |
| W8 | same chain declared top-down (random order) | 4 warnings (one per forward ref) |
| W9 | class: methods ordered (callee method above caller) | 0 warnings (good) |
| W10 | class: methods in random order (`self.m` calls a method below) | warning per forward ref |
| EC14 | direct self-recursion | no warning (cycle) |

### References model + shared state (⟸ L5, C2) — see `tests/m4_references.rs`, `tests/m4_scope.rs`

| # | Case | Expected |
|---|------|----------|
| R1 | bare `@deco` / `@deco()` referencing a function below | warning ON the decorated entity (not `<module>`) |
| R2 | base class / default arg referencing a definition below | warning on the defining entity |
| R3 | `x = helper` (referenced, not called) with `helper` below | warning (references, not just calls) |
| R4 | `cb = self.b` (member read, not call) with `b` below | warning on the method |
| R5 | recursive / forward type annotation (`x: Node`) | no warning (annotations excluded) |
| R6 | mutual header references (`@b`/`@a`) | no warning (cycle) |
| G1 | module var mutated via `global` in ONE function | 0 (pinned — intentional shared state) |
| G2 | `global` state set in one function, read in another | 0 (cross-function, not narrowed) |
| G3 | bare name in a method shadowed by a class attribute | resolves to the module → class attr unused, 0 |
| G4 | plain module constant used once | 0 (data definition — placed/ordered, NOT narrowed) |
| G5 | `self.x` reading a class attribute | edge only — NOT narrowed into the method |
| D1 | shared module constant + unrelated def wedged before its readers | wedge on each reader function |
| D2 | function reads a module constant declared BELOW it | declare-order warning on the function |
| D3 | local variable used once in a nested block | narrows (only locals narrow) |
| D4 | constant computed from another, unrelated const wedged between | wedge on the computed constant |
| D5 | constant computed from another declared BELOW it | declare-order warning on the constant (not `<module>`) |
| D6 | function-local `x = helper()` | the function (not `x`) depends on `helper` |

### Findings / render / CLI

- OUT1 — summary: Σ scope-debt total + per-file rollup; warnings counted separately.
- OUT2 — sort stability: findings sorted by (file, line), byte-stable.
- OUT3 — json schema: stable JSON (snapshot-able).
- OUT4 — empty project: no findings, "clean" summary, exit 0.
- OUT5 — exit codes: default 0; `--error` → 1 if any Error-severity (ParseError) finding.
- OUT7 — top-N ranking: `--top N --by function|class|file` ranks by scope-debt (bindings roll into
  their function, methods into their class).
- ROB1 — syntax error → one `ParseError` finding; other files still analyzed.
- ROB2 — non-UTF8 → skip/ParseError; analysis continues.

## Work plan

- [x] **Core (locality)** — `scopelang` (the ScopeLang trait + Action driver: walk + nesting +
      attribution bookkeeping) + `scopegraph` (the one graph + scorer → `ScopeOutput`) + `defgraph`
      (entities+edges projection) + `wedge` (shared gap-to-deps) + `placement` + `declorder` +
      `score` + `finding` + `analyze` pipeline. **100% test coverage**, via direct language-agnostic
      unit tests (no parser): `core_scopegraph.rs` (16), `core_defgraph.rs` (11), `core_driver.rs`
      (6, a toy `ScopeLang`), `core_wedge.rs` (6).
- [x] **Python frontend (ruff)** — `PyLang`: lowers each ruff node into `Action`s (the only
      per-language code). One walk yields scope-debt + edges + entities + declare-before-use. No
      effect/purity analysis. `tests/m4_scope.rs` (40) + `tests/m4_warn.rs` (15) + `tests/m4_references.rs`
      (19, the references model + attribution + global/nonlocal).
- [x] **CLI + render + discovery** — `render/{text,json}` (Summary / All / Top views), `discover`
      (walk, skip venv/__pycache__/…), `main` CLI (`PATH --format --summary|--all|--top --by --error`),
      config `[tool.ventouse.weights] scope_level`. `tests/m3_render.rs` (14): rollup, text/json views,
      ranking, discovery, end-to-end (good.py → 0 scope-debt), ParseError wiring.
- [x] **L2 completeness (Python frontend lowering)** — DONE. The Python `expr_actions`/`stmt_actions`
      now lower `yield`/`yield from` and slice bounds (recurse children), f-string interpolations,
      comprehension / generator / lambda bodies (their own Py3 scope — first iterable in the enclosing
      scope, targets/guards/result inside), and `match`/`case` (subject + per-case block; patterns
      bind captures and read literal/class/key references). Rust likewise recovers references inside
      expression-list macros (`vec!`/`format!`/…). The catch-alls now drop only ref-less nodes
      (literals, `pass`/`break`/`continue`; `global`/`nonlocal` are collected separately). Residual:
      references inside a lambda/comprehension/closure BODY are attributed to that scope, so they
      aren't in the call graph (captures still narrow) — the one inherent limit of scoped bodies.
- [x] **Rust frontend (syn)** — `src/lang/rust/{prim,lower,mod}`: `ScopeLang` over syn; block-scoped
      `let`, `impl T` → class scope `T`, `const`/`static` as data, expr-list macro reference recovery.
      `tests/rust_lang.rs`. Dogfooded on its own source AND external projects.
- [x] **Suggestions** — `ExtractShared` / `CrowdedScope` / `ReorderBinding` / `NarrowToBlock` in `core::analyze`
      (`tests/core_defgraph.rs`, `tests/m4_scope.rs`). The actionable layer: the metric naming the fix.
- [x] **Declare-order direction flag** — `--order=bottom-up|top-down` (`Weights.order`); the one
      conventional axis made configurable (`tests/m4_references.rs`).
- [x] **Example catalog** — `tests/fixtures/python/catalog/` (01 local · 02 definitions · 03 free /
      crowded + README): tiny good-vs-bad per rule, each verified against real output.
- [x] **Dogfood fixes** (each a regression test) — cross-branch (cousin-block) wedges, nearest-dep
      anchoring, Rust `self.field`-is-not-a-method conflation, `ReorderBinding` on loop-carried
      accumulators, loop-variable phantom-wedge (for-target bound inside the loop + `intro` accrues no
      wedges). Self-scan + external-project scans drive these.
- [ ] **JS/TS frontend (oxc)** — a `ScopeLang` impl over the oxc AST. `let`/`const` block-scoping
      makes `levels` narrow real runtime scope; `var` is function-scoped. Fixtures: `…/js/`, `…/ts/`.
- [x] **C++ frontend (libclang)** — `ScopeLang` over libclang; scope + in-class method-order
      warnings; reads `compile_commands.json` for flags. `tests/cpp_lang.rs` + `fixtures/cpp/`.

## Implementation notes

- Cargo: the **ruff** parser via git, pinned to a tag — `ruff_python_parser`, `ruff_python_ast`,
  `ruff_text_size` `{ git = "https://github.com/astral-sh/ruff", tag = "0.15.8" }`.
- `parse_module(src)?.syntax().body` → `Vec<Stmt>`. Ranges are byte offsets (`ruff_text_size`); a
  small `LineIndex` (byte→row) gives line numbers. ruff folds `async` into `is_async`, `elif`/`else`
  are `ElifElseClause`s, walrus = `Expr::Named`, ifexp = `Expr::If`, genexp = `Expr::Generator`.
- `PyLang::actions` lowers one ruff node (statement or expression) into a balanced list of scope
  actions; the core driver (`scopelang::build`) threads a (scope, block) stack so the frontend never
  manages ids itself. Adding a language = a new `ScopeLang` impl; the metric is inherited unchanged.
- The scope graph is whole-module: a scope tree with lexical name resolution, a connected block tree
  (for `min_block`/levels, loop-aware), and per binding its declaration position, uses, and RHS
  dependencies (for wedges). Textual order drives declare-before-use; `self.X` member references
  resolve in the enclosing class scope (no fall-through).
- async/lambda/comprehension/generator: async-def = a regular def; lambda/comprehension have their
  own scope.
