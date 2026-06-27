# Rule catalog — good vs. bad, by rule

Tiny, self-contained Python examples for every ventouse rule. Each `*_bad` triggers
exactly one finding (named in a comment); each `*_good` scores 0. Run any file:

```
ventouse tests/fixtures/python/catalog/01_local_scope.py --all
```

| Rule | Kind | Example | What it flags |
|------|------|---------|---------------|
| `ExcessLevels` | scope-debt | `01` `levels_*` | a value declared higher than the narrowest block that covers its uses |
| `Misplaced` (wedge) | scope-debt | `01` `wedge_*` | unrelated definitions between a value and what it connects to |
| `UseBeforeDecl` (value) | warning | `01` `use_before_*` | a name read before it is bound, same scope |
| `ReorderBinding` | suggestion | `01` `reorder_*` | a value declared ≥3 definitions above its first use → move it down |
| `UseBeforeDecl` (function) | warning | `02` `caller_*` | a callee defined below its caller (forward reference) |
| `Misplaced` (placement) | scope-debt | `02` `weight`/`gravity` | a definition wedged away from what it references |
| `ExtractShared` | suggestion | `02` `shared`/`task_*` | shared infrastructure (≥4 callers) → extract into its own module |
| `CrowdedScope` | suggestion | `03` `crowded_bad` | a bag of independent accumulators → bundle into a struct |

## Not penalized (the metric must not punish good code) — file `03`

| Pattern | Why it scores 0 |
|---------|-----------------|
| single loop-carried accumulator (`running_sum_ok`) | loop-carried state can't narrow into the loop; nothing wedges it |
| module constant read by one function (`clamp`/`LIMIT`) | a DATA definition is placed/ordered, never narrowed into its reader |
| `global` state written in one function (`bump`/`COUNTER`) | shared-by-intent → pinned, never narrowed |
| parameters (`add`) | not a placement the function chose → excluded |

Note: a loop-carried accumulator can still carry wedge debt when many of them
crowd one scope (that is the `CrowdedScope` signal) — but `ReorderBinding` does
**not** fire on it, because moving its seed into the loop would reset it.
