# Hacks, workarounds & known limitations

An honest inventory of the crutches across the frontends (C++ §1–§2, Rust §6, Python §7, shared §8)
and the suggestion layer (§3–§4). Each entry: **what**, **why**, **where**, and the **path to remove
it**. Read this before trusting a result or before "cleaning up".

§1–§2 dominate because **libclang is a heavy external tool**: it degrades the AST when types are
unresolved (a missing `#include` / external dependency), silently dropping references, and it forces
the threading/loading gymnastics. The Rust (`syn`) and Python (`ruff`) frontends parse a clean,
complete native AST in-process — they carry **none** of that, only the static-analysis approximation
every frontend shares (§8): no type resolution, so receivers/attributes are guessed or dropped.

---

## 1. Parsing / libclang

### 1.1 `-x c++` forced on every file
- **What:** every input is parsed with `-x c++` (`BASE_ARGS` in `src/lang/cpp/mod.rs`).
- **Why:** without it libclang infers the language from the extension and treats a `.h`/`.hpp` as a
  *header to precompile*, which fails standalone with `AST deserialization failed`. Headers are where
  C++ class/method declarations live, so they must parse as ordinary translation units.
- **Remove:** never — this is correct, not a hack. Listed for context.

### 1.2 Runtime libclang shared across threads via TLS poke
- **What:** parsing is parallel; each worker thread calls `clang_sys::set_library(get_library())` to
  point its thread-local at the one already loaded on the main thread. `src/lang/cpp/mod.rs`,
  `parse_all`.
- **Why:** `clang-sys`'s `runtime` feature stores the loaded library in **thread-local storage**, so
  a fresh worker thread has *no* libclang and panics (`a libclang shared library is not loaded on this
  thread`).
- **Risk:** depends on `clang`/`clang-sys` internal loader behaviour; a major version bump could change
  the TLS contract.
- **Remove:** link libclang at build time (drop the `runtime` feature) — but that needs a `libclang.so`
  on the linker path; this box only has versioned `libclang-*.so.N`, which is why we use `runtime`.

### 1.3 `unsafe impl Sync for SharedClang`
- **What:** `struct SharedClang<'c>(&'c Clang); unsafe impl Sync`. `src/lang/cpp/mod.rs`.
- **Why:** the `clang` crate marks `Clang` `!Send`/`!Sync` to keep single-threaded users safe. We opt
  into the **documented** libclang multi-index threading (one `Index` per thread, distinct TUs). Only
  `&Clang` crosses the thread boundary; no cursor/TU does.
- **Validation:** an empirical test confirmed parallel output == serial output before this shipped.
- **Risk:** relies on libclang actually being thread-safe per-index (it is, per its docs and clangd/
  ccls usage). If that ever breaks, this is UB.
- **Remove:** a `Send`/`Sync` libclang wrapper, or single-threaded parsing (≈8× slower).

### 1.4 Process-global `Mutex<()>` around `Clang::new`
- **What:** `CLANG_LOCK` serializes the whole `extract`/`parse_all`. `src/lang/cpp/mod.rs`.
- **Why:** the `clang` crate allows only **one** `Clang` instance per process at a time
  (`Clang::new` errors otherwise). Tests run in parallel threads → would collide.
- **Remove:** unavoidable while using the `clang` crate's singleton guard.

### 1.5 `clang_10_0` API level against a libclang-18
- **What:** `Cargo.toml` pins the `clang` crate feature to `clang_10_0` (its newest), although the
  system library is libclang-18.
- **Why:** the `clang` crate (v2) exposes no newer feature flag; `runtime` loads the real (newer) lib
  dynamically, and libclang is backward-compatible, so the 3.5-baseline API we use is fine.
- **Risk:** any API added after clang 10 is unavailable through the crate.

### 1.6 Heuristic include paths (fallback when there is no `compile_commands.json`)
- **What:** `compile_commands.json` is now read when present (`src/lang/cpp/compdb.rs`): its precise
  `-I`/`-isystem`/`-D`/`-std` are extracted (relative dirs resolved against each entry's `directory`,
  aggregated + deduped across entries) and passed to libclang. **Only** when no database is found does
  the heuristic kick in: `-I` = the files' common-ancestor dir plus `include/`/`src/` subdirs
  (`src/lang/cpp/mod.rs`, `include_dirs`).
- **Remaining crutch (the heuristic fallback only):**
  - **External deps** (e.g. `boost/...`) are not found → that file's AST degrades (§4.2). A real
    `compile_commands.json` resolves them and removes this.
  - **Single-file runs** root at the file's own directory, so project-relative `#include "pkg/foo.h"`
    may not resolve; whole-project runs are fine.
  - No `-D`/`-std` is inferred; `-std=c++17` is hard-coded.
- **Remove:** for a project WITH a `compile_commands.json` this is already gone. The fallback stays
  for projects that lack one.

### 1.7 Parse from an in-memory `Unsaved` buffer
- **What:** the file content is handed to libclang as an unsaved buffer; the path need not exist.
- **Why:** lets tests pass `("test.cpp", src)` without touching disk, and matches how the pipeline
  already holds sources in memory. Not really a hack — noted for completeness.

---

## 2. Frontend lowering (`src/lang/cpp/lower.rs`)

### 2.1 Instance fields are dropped
- **What:** `FieldDecl` lowers to nothing (not bound); a `this->field` member reference simply does
  not resolve and is discarded.
- **Why:** binding fields as entities would reintroduce field-order declare-warnings (noise; a field
  used by a method above it is idiomatic C++) and make `this->field` collide with same-named methods.
  Matching Rust, fields are invisible.
- **Remove:** a first-class `Field` entity kind excluded from declare-order/placement/extract —
  deliberately avoided (only the now-removed god-class analysis ever needed field connectivity).

### 2.2 Loop bodies unwrapped to avoid a phantom nesting level
- **What:** for `for`/`while`/`do`/range-`for`, the body `CompoundStmt` is unwrapped into the single
  loop block rather than nested, and the function body's outer `CompoundStmt` is unwrapped into the
  scope's entry block.
- **Why:** otherwise every brace adds an extra `levels` of nesting that doesn't exist semantically.
- **Remove:** not a hack — it's the correct block model; noted because the unwrap logic is subtle.

---

## 3. Heuristic thresholds & gates (the real crutches)

> **All numeric thresholds below are now config-overridable** via `[tool.ventouse.weights]`
> (`src/core/score.rs` `Weights`, parsed in `src/config.rs`). The values quoted are the *defaults*;
> a project can retune any of them without code changes. The crutch is that the defaults are tuned,
> not derived — the config makes them honest knobs rather than buried magic numbers.

### 3.1 Accessor pinning: `ACCESSOR_FANIN = 3` + leaf fixpoint
- **What:** a class-member method that is a leaf (or transitively calls only accessors) and is called
  by **≥3** distinct siblings is treated as de-facto data ("pinned"): excluded from placement spans
  and never `ExtractShared`-suggested. `src/core/placement.rs`, `accessors`.
- **Why:** trivial accessors (`begin()`, `readableBytes()`) inflate the placement debt of every method
  that uses them, and `ExtractShared` nonsensically suggested "extract `begin()` into its own module".
- **Crutch:** `3` is a magic number; a real accessor used by 2 siblings is missed. The graph cannot
  tell a 1-line field reader from a real leaf method (no body-size in a locality-only model).
- **Subtle bit:** a pinned accessor is removed from the **anchor** of a span but KEPT in `target_deps`
  for the "shares a dependency" exemption — splitting these two uses was needed, the naive version
  *raised* scores.

### 3.2 Reorder vs narrow split on `levels_excess == 0`
- **What:** `reorder_suggestions` only fires when `levels_excess == 0`; the deeper-block case is ceded
  to `narrow_suggestions`. `src/core/analyze.rs`.
- **Why:** both would otherwise suggest moving the same binding to the same line — redundant. The
  narrow framing ("declare it inside the block") is more specific, so it wins when there's a level to
  narrow.
- **Crutch:** mild — a coordination rule between two suggestions that share the `first_use` target.

### 3.3 `ReorderBinding` reason reused for definitions
- **What:** the definition-level reorder reuses `Reason::ReorderBinding { first_use, wedged }` rather
  than a dedicated variant. `src/core/analyze.rs`, `def_reorder_suggestions`.
- **Why:** the wording ("move the declaration down to it") fits a definition too.
- **Crutch:** minor — "declaration" reads slightly oddly for a function, and the two sources share a
  reason code.

---

## 4. Known false positives / accepted limitations

### 4.1 Public accessor gets a def-level reorder hint
- **What:** a trivial public accessor (`StringPiece::data()`/`size()`) used once *in-file* is flagged
  "declared above its first use — move down", which is bad advice for a public API method.
- **Why:** per-module analysis sees only in-file uses; the accessor's real (external) callers are
  invisible, so its in-file usage is unrepresentative. By the metric's own axiom the suggestion is
  *consistent* (used once in-file → move to that use); it's "wrong" only against an external-API view
  the tool deliberately doesn't model.
- **Status:** NOT fixed. A graph-only fix would also kill the genuine cases (e.g. `swap`), since a
  trivial accessor and a real used-once leaf are indistinguishable. Documented, accepted.

### 4.2 Unknown-type `return var` drops the use → false `levels`
- **What:** `return content;` where `content` has an unresolved type lowers to a `ReturnStmt` with an
  **empty** `UnexposedExpr` — the reference to `content` is gone. The variable then looks "used only
  in a nested block", yielding a false `levels`/narrow finding.
- **Why:** libclang's error-recovery AST for an unknown-typed expression is degraded.
- **Status:** fixed for any project with a `compile_commands.json` (§1.6, all deps resolve). With only
  the heuristic fallback, the residual remains on files that pull in unresolved **external** deps. Not
  gated for `narrow` (it degrades gracefully).

---

## 5. Test / corpus debt

### 5.1 Stale C++ fixtures
- **What:** `tests/fixtures/cpp/{dirt,mutation,annot,self_class,...}.cpp` describe a removed
  dirt/purity model (their comments are outdated). They mirror the equally-stale Rust fixtures.
- **Why:** kept for parity with the Rust fixture set; they're valid C++ the frontend parses, and no
  test asserts on them.
- **Remove:** rewrite (or delete) once the fixture sets are refreshed project-wide.

### 5.2 Thresholds tuned against one corpus (muduo)
- `accessor_fanin`, `reorder_wedges`, `narrow_levels`, `shared_callers`/`shared_uses`, `crowded_scope`,
  and the include-path heuristic were all validated by dogfooding **muduo** parsed **without a build**.
  They are not derived from a principle and may need revisiting on other codebases / with full compile
  flags. All the numeric ones are now overridable in `[tool.ventouse.weights]` (§3 banner), so retuning
  needs no code change — but the *defaults* are still one-corpus guesses.

---

## 6. Rust frontend (`syn`, `src/lang/rust/lower.rs`)

`syn` gives a complete native AST, so there is no degradation/threading/include machinery — just
lowering approximations. None of these is unsound (they UNDER-report references, never invent one).

### 6.1 Macro bodies are tokens, not AST
- **What:** a macro invocation's references are recovered ONLY if its body parses as a comma-separated
  expression list (`vec!`, `format!`, `assert_eq!`, …); a body that doesn't (`matches!(x, Pat)`, a
  custom DSL, `macro_rules!` arms) parses-fails and is silently skipped. `lower::macro_uses`.
- **Why:** `syn` hands macro bodies back as raw token streams, not an AST to walk.
- **Remove:** nothing general short of expanding macros (needs a compiler); the expr-list recovery is
  the pragmatic 80%.

### 6.2 Macro references all take the macro's line
- **What:** every reference recovered from a macro is attributed to the macro call's line.
- **Why:** the recovered sub-expressions have spans, but using the call line is "good enough" for
  placement/declare-order (a macro call is one logical site).

### 6.3 `EXTERNAL_ROOTS` allow-list
- **What:** a path rooted at `crate`/`super`/`std`/`core`/`alloc` is treated as cross-module and
  dropped; any other multi-segment path references its **leading** segment. `lower::path_use`.
- **Why:** a cheap "is this local?" test without name resolution.
- **Crutch:** a local item that happens to be named `std`/`core`/… would be wrongly dropped (rare),
  and `Foo::bar` (an associated fn) is conflated with the type `Foo` (§6.4).

### 6.4 Multi-segment path → leading segment only
- **What:** `Type::assoc`/`module::item` resolve to the leading `Type`/`module`, not the tail.
- **Why:** no path resolution; the head is the in-file entity we can name.

### 6.5 Trait bodies are not walked
- **What:** `Item::Trait` emits only a `bind_decl` — default-method bodies and associated items inside
  a trait are invisible (no scope opened). Same for `enum` variants / `struct` fields (not bound).
- **Why:** not yet lowered; traits-with-defaults are rarer than impls.

### 6.6 Field access dropped
- **What:** `self.x` (an `Expr::Field`) emits no member reference — only the base is recursed.
- **Why:** in Rust a field is always a field; without types it can't be told from a same-named
  method, and conflating them produced phantom declare-order warnings. (Same family as C++ §2.1 and
  Python §7.2.)

---

## 7. Python frontend (`ruff`, `src/lang/python/lower.rs`)

Like Rust: a clean native AST, so the hacks are all "no type info → approximate".

### 7.1 `self`/`cls` member resolution is NAME-based
- **What:** `obj.attr` resolves as a class member only when `obj` is literally the name `self` or
  `cls`; a method written `def m(this): this.x` won't resolve `x`.
- **Why:** Python has no `self` keyword — it's a PEP 8 convention, not enforced. Matching on the
  conventional names is the only signal available without type resolution.

### 7.2 `obj.attr` drops the attribute (non-self/cls)
- **What:** any other `obj.attr` (e.g. `mod.func`, `self.field.method`) contributes only a use of the
  receiver; the attribute/method name is not a reference.
- **Why:** the receiver's type is unknown, so the attribute can't be resolved to a definition.

### 7.3 Import binds only the top segment; `import *` skipped
- **What:** `import a.b.c` binds `a`; `from x import *` is skipped entirely, so star-imported names
  never resolve (their references silently don't become edges).
- **Why:** submodule/attribute access is dropped (§7.2), and a glob brings unknown names.

### 7.4 f-string `format_spec` expressions not walked
- **What:** `{x}` interpolations are references, but expressions inside a `format_spec` (`{x:{width}}`)
  are not recursed.
- **Why:** rare; a residual hole.

### 7.5 A defaulted class field `x: T = v` still binds as data
- **What:** a BARE class annotation `x: T` is correctly treated as a field declaration (not bound —
  the fix from dogfooding `requests`), but an annotated assignment WITH a value (`x: T = v`, the
  common `@dataclass`/`NamedTuple` field-with-default form) still binds as a class datum, so a method
  reading `self.x` accrues placement debt for its distance to the field.
- **Why:** the lowering can tell "annotated" from "plain" but NOT "class scope" from "module scope"
  (where `MAX: Final[int] = 100` IS a real typed constant that should bind) — distinguishing them
  needs scope-aware lowering. So the conservative rule only drops the unambiguous no-value case.
- **Impact:** small in practice — the "shares a dependency" exemption dampens it (in a cohesive class
  many methods use the same fields, so they don't wedge each other; e.g. `rich`'s `Table._render`
  scored 10, not hundreds). Documented, not fixed.
- **Remove:** thread scope into the Python lowering so a class-scope annotated assignment is dropped
  like a field while a module-scope one stays a constant.

---

## 7b. TypeScript / JavaScript frontend (`oxc`, `src/lang/ts/lower.rs`)

### 7b.1 An arrow/function/class assigned to a name becomes a NAMED definition
- **What:** `const Foo = () => …` / `= function …` / `= class …` is lowered as a `Decl` named `Foo`
  with a scope named `Foo` (not an anonymous `<arrow>`). Same for a class-field arrow (`onClick =
  () => …` → a method).
- **Why:** it is the dominant JS/React shape (components, hooks, handlers). Without it, nearly all
  real-world debt piles onto one anonymous `<arrow>` bucket and the per-entity attribution is useless
  (dogfooding takenote: the top offender went from `<arrow>` 9160 to real component names).
- **Remove:** nothing to remove — this IS the right model; a truly anonymous arrow (a `.map(x => …)`
  callback) still opens an `<arrow>` scope, and its references are attributed to the enclosing named
  definition by the core's `attribution_scope`.

### 7b.2 React hook results are pinned by a `use*` name heuristic
- **What:** a declarator whose initializer is a call to `useFoo(…)` / `X.useFoo(…)` binds its names
  as `intro` (positionally fixed — no levels, no wedges, no reorder), like a loop-carried seed.
- **Why:** the rules of hooks REQUIRE hooks at the top of a component, unconditionally — they cannot
  move down, so flagging `const [x] = useState()` for `ReorderBinding` is a guaranteed false alarm
  (dogfooding takenote: hook pinning cut total debt 8710 → 3710, all of it false positives).
- **Risk:** a non-hook function named `useThing()` is also pinned (rare; the `use` + capital
  convention is near-universal). A hook NOT named `use*` (unusual) is missed.
- **Remove:** real binding-level dataflow would be needed to know a value is rules-of-hooks-pinned;
  the name convention is the cheap, reliable proxy React itself relies on (eslint-plugin-react-hooks).

### 7b.3 `this.x` is a member reference; `a.b` drops the property
- **What:** `this.x` / `this.x()` emit a member use resolved in the class scope; any other `a.b`
  recurses only the base (like Rust/Python). Optional chaining (`a?.b`) is unwrapped first.
- **Why:** members of another object aren't local entities; only own-class members are. Matches the
  cross-frontend member rule.

### 7b.4 Types are excluded; `var` treated as block-scoped
- **What:** TS type aliases / interfaces / `enum` / annotations / generics are not lowered (no runtime
  locality). `var` is lowered like `let`/`const` (block-scoped), not hoisted to the function.
- **Why:** annotations are excluded by the references model (§8.4); function-scoped `var` is rare in
  modern TS and treating it as block-local at worst slightly over-narrows a legacy `var`.

---

## 8. Shared static-analysis approximations (all frontends)

These are the same across C++/Rust/Python. The first two are real under-reporting; the rest are
deliberate locality-model design, listed so they aren't mistaken for bugs.

### 8.1 `collect_loads` is shallow *(real limitation)*
- **What:** a binding's RHS dependencies (used for wedges) are collected without descending into a
  nested closure/lambda/block on the RHS. `let x = || dep()` doesn't record `dep` as a dep of `x`.

### 8.2 Scoped-body references leave the call graph *(real limitation, documented)*
- **What:** references inside a lambda / closure / comprehension body are attributed to that inner
  scope, so their call edges aren't in the module's definition graph (captures still narrow
  correctly). The one inherent limit of scoped bodies.

### 8.3 No type resolution → guessed/dropped receivers *(by design)*
- The root cause of §6.4, §6.6, §7.1, §7.2 and the C++ explicit-object member drop. A locality-only
  metric deliberately carries no type system.

### 8.4 Annotations / signatures / generics excluded *(by design)*
- Type positions are not references — uniform across languages ("annotations are excluded").

### 8.5 Per-module analysis *(by design)*
- Locality is same-file: references to other files are dropped. The fundamental scope of the tool.

---

## Priority to pay down

1. ~~`compile_commands.json` support~~ — **done** (§1.6, `src/lang/cpp/compdb.rs`). The remaining
   heuristic is now only a fallback for projects without a database.
2. Derive the (now config-overridable) thresholds (§3.1, §5.2) from a principle instead of one-corpus
   defaults — the knobs exist, the defaults are still guesses.
3. `-D`/`-std` for the heuristic fallback (only `-I` is guessed; the database path already has them).

> **Removed:** the god-class / cohesion analysis (and its `member_accesses`/`parse_complete`
> infrastructure) was cut — most plumbing of any feature, a field-connectivity *proxy* rather than
> real resolution, and **0** true hits on muduo. Reintroduce only with first-class `Field` entities
> and full type resolution (so it is sound, not gated-into-silence).
