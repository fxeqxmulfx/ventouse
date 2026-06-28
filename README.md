# ventouse

A **locality linter** for code — especially the code an LLM agent writes. It scores how far each
definition sits from what it connects to, and, more usefully, **names the concrete move that fixes
it** (reorder this binding, extract this helper, bundle these accumulators). The goal: lay code out
the way a human reviewer reads it.

> Status: locality-only, one graph (the scope graph). Language-agnostic core with direct unit tests
> at **100% coverage**. **Python (`ruff`), Rust (`syn`), C++ (`libclang`) and TS/JS (`oxc`, incl.
> JSX/React) frontends are done** ([todo.md](todo.md)).

## The problem

LLMs produce a lot of code that is *correct but sprawling*: a variable bound far above its first
use, a helper scattered mid-file, callees above their callers, a function holding a bag of unrelated
accumulators. It compiles and the tests pass — type-checkers and test suites say nothing about
**layout**. But a human (or the next agent) still has to read it, and poor locality is real
working-memory cost: how far back must you look to know what a name means?

ventouse measures exactly that, and points at the fix.

## What it measures — locality

A definition is *local* when it sits in the narrowest block that covers its uses, right **after**
what it depends on, and right **before** its first use. Distance from that ideal is **scope-debt**.
It's purely structural — no annotations, no markers.

- **`levels`** — a value nested less deeply than the block that actually covers its uses (loop-aware:
  loop-carried state stays outside the loop). Values only; declaring a function/class is free of
  nesting, so decomposition is never punished.
- **`wedges`** — unrelated definitions wedged between a thing and what it connects to (its
  dependencies above, its first use below). Junk between related code is the cost.
- **declare-order** (a warning, 0 points) — a definition referenced out of reading order. This is
  the one *conventional* axis; see [`--order`](#the-one-opinionated-axis) below.

Value-level rules follow **data flow** (you can't use a value before you compute it) and never flip.
Only function/definition *order* is convention.

## What it tells you to do — suggestions

The number tells you *where*; these tell you *what*. They are the point of the tool — each is a
safe, mechanical refactor the metric can justify:

| Suggestion | Fires when | The move |
|------------|-----------|----------|
| **ReorderBinding** | a local is declared ≥3 definitions above its first use | push the declaration down to its use (the things in between provably don't depend on it — and it won't suggest moving a loop-carried accumulator into its loop) |
| **NarrowToBlock** | a local is used only inside a block nested deeper than where it's declared | declare it at its first use, inside that block — its live range shrinks to the scope that actually needs it |
| **ExtractShared** | a definition is referenced all over a file that carries placement debt | pull it into its own module — per-file analysis then stops penalizing those (now cross-file) references and the rest localizes |
| **CrowdedScope** | a function holds a bag of *independent* mutable accumulators wedging each other | bundle them into one struct/dataclass — N siblings collapse to one |

## Reader = human, writer = agent

The reading model ventouse optimizes (linear, limited working memory) is a **human** one — and that's
the point: the agent is the *writer*, the human (or the reviewing agent) is the *reader*. ventouse is
the conscience that nudges AI-written code toward a layout a reviewer would accept. The validation
question is correspondingly human and tractable: does low-debt layout measurably speed comprehension?

### The one opinionated axis

Whether a callee should sit **above** its caller (define-before-use) or **below** it (stepdown /
overview-first) is a reading *habit*, not a law — both are widely held. So it's a flag, not a
hard-coded rule:

```
ventouse src                      # default: bottom-up (callees above callers)
ventouse src --order=top-down     # stepdown: callers above callees
```

The same source can read 0 warnings one way and dozens the other — the metric doesn't claim to know
which is "right". Everything else (locality, wedges, value use-before-binding) is direction-invariant.

## A taste

```python
def f():
    q = setup()        # bound here...
    a = step_one()
    b = step_two()
    c = step_three()
    log(a, b, c)
    return run(q)      # ...but first used only here — 3 unrelated definitions wedged between
```

```
f.q   [scope]    3 unrelated definition(s) between it and what it connects to
f.q   [suggest]  3 unrelated definition(s) before its first use (line 7) — move the declaration down to it
```

Move `q = setup()` down to just above `return run(q)` and both go to zero. A catalog of tiny
good-vs-bad examples, one per rule, lives in
[`tests/fixtures/python/catalog/`](tests/fixtures/python/catalog/).

## Design — one core, many languages

A single language-agnostic core; per-language frontends behind one trait. A frontend implements
`ScopeLang` — it lowers each syntax node into core `Action`s (`Bind` / `Use` / `OpenBlock` /
`OpenScope`); the core driver builds **one** `ScopeGraph` and derives everything from it (scope-debt,
the reference graph, the entity list, declare-order). Only the *surface* differs.

| Language | Parser | Status |
|----------|--------|--------|
| Python | `ruff_python_parser` | done |
| Rust | `syn` | done |
| C++ | `libclang` | done |
| JavaScript / TypeScript | `oxc` | done (incl. JSX) |

In block-scoped languages (Rust, JS `let`/`const`, C++) `levels` narrows the **real** runtime scope,
so it bites harder than in function-scoped Python. Dogfooding ventouse on its own source — and on
external projects — surfaced and fixed several core imprecisions (cross-branch wedges, nearest-dep
anchoring, field-vs-method conflation, loop-carried reordering), each now a regression test.

## Installing & running

ventouse is a single Rust binary (edition 2024 — needs a recent stable toolchain).

```
git clone <repo> && cd ventouse
cargo build --release          # binary at target/release/ventouse
cargo install --path .         # or install it onto your PATH as `ventouse`
```

Point it at a file or a directory; it discovers sources, analyzes the whole tree as one project, and
prints findings. The language is auto-detected from whichever extension dominates the path — override
with `--lang`.

```
ventouse                       # analyze ./  (auto-detect language)
ventouse src/                  # a directory
ventouse path/to/file.py       # a single file
ventouse src --lang=rust       # force a frontend
```

Without installing:

```
cargo run --release -- src --top=10 --by=function
```

### Per language

- **Python** — works out of the box (`.py`).
- **Rust** — works out of the box (`.rs`).
- **C++** — needs **libclang** available at run time (any system `libclang-*.so`; no build-time
  linking, no `LIBCLANG_PATH` if it sits on the default search path). For accurate include paths and
  flags, run from a build dir that has a **`compile_commands.json`** (CMake: `-DCMAKE_EXPORT_COMPILE_COMMANDS=ON`);
  ventouse reads it automatically. Without it the parse is best-effort. See [HACKS.md](HACKS.md) for
  the libclang specifics.
- **TS/JS** — works out of the box (`.ts`/`.tsx`/`.js`/`.jsx`, via `oxc`; `.d.ts` skipped). `.tsx`/
  `.jsx` enable JSX, so component references and `{ … }` containers in returned markup feed the graph.
  An arrow assigned to a name (`const App = () => …`) is treated as a named definition; React hook
  results (`useState`, `useSelector`, …) are positionally pinned (rules of hooks), so they aren't
  flagged for reorder.

## CLI

```
ventouse [PATH] [--lang=python|rust|cpp|ts] [--order=bottom-up|top-down] \
       [--format=text|json] [--summary | --all | --top=N --by=function|class|file] [--error]
```

| Flag | Meaning |
|------|---------|
| `PATH` | file or directory to analyze (default `.`) |
| `--lang=python\|rust\|cpp\|ts` | force a frontend (default: auto-detect by file count) |
| `--order=bottom-up\|top-down` | declare-order convention (default `bottom-up`; see [above](#the-one-opinionated-axis)) |
| `--format=text\|json` | output format (default `text`) |
| `--summary` | headline total + per-file rollup (**default**) |
| `--all` | list every finding |
| `--top=N --by=function\|class\|file` | rank the N worst offenders, grouped by source |
| `--error` | exit non-zero if any parse error occurred (for CI) |

**Measure, don't judge**: raw numbers, no built-in pass/fail thresholds — a gradient is more useful
than a gate. All scoring constants and suggestion thresholds are overridable via
`[tool.ventouse.weights]` in `pyproject.toml` (in the analyzed project's root), so you can tune
sensitivity without rebuilding.

## What it is not

One axis, honestly scoped. ventouse measures *locality*; it says nothing about naming, function size,
abstraction level, or correctness. Drive scope-debt to zero and you can still have unreadable code —
so it's **one signal among several and a final layout pass**, not the whole picture, and best used
**advisory** (the suggestions are reliable; the headline number is a guide, not a target to game).

## Repository

- [RULES.md](RULES.md) — the complete "if … then …" decision logic of the core, every rule with a
  minimal bad-vs-good example and the default thresholds.
- [todo.md](todo.md) — the specification: locality rules, scoring, per-language mapping, test matrix.
- [tests/DESIGN.md](tests/DESIGN.md) — every case with exact expected numbers + a rule→case checklist.
- [tests/fixtures/python/catalog/](tests/fixtures/python/catalog/) — tiny good-vs-bad examples per rule.
- `src/` — `core/` (language-agnostic), `lang/` (frontends), `render/` (output).

## License

MIT — see [LICENSE](LICENSE).
